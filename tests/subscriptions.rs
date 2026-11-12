//! Subscriptions through the real server: read as a stream, posted to a webhook and signed, moved to failing
//! and resumed, and run out (docs/api.md, "Subscriptions").

use crate::support;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::routing::post;
use bmo_job_client::{
  Event, EventQuery, NewSubscription, SIGNATURE_HEADER, SUBSCRIPTION_HEADER, Subscription,
  SubscriptionChange, SubscriptionState, signed_by,
};
use futures_util::StreamExt;
use tokio::net::TcpListener;
use tokio::sync::mpsc;

use support::Service;

fn to_build(holder: &str) -> NewSubscription {
  NewSubscription {
    holder_name: holder.to_owned(),
    kinds: vec!["item.created".to_owned()],
    queue_names: vec!["build".to_owned()],
    ..NewSubscription::default()
  }
}

fn page(subscription: &Subscription) -> EventQuery {
  EventQuery {
    subscription_id: Some(subscription.id),
    ..EventQuery::default()
  }
}

/// A webhook receiver on loopback: each post it is sent, as its headers and body, and the status it answers,
/// which the test sets.
struct Receiver {
  url: String,
  posts: mpsc::UnboundedReceiver<(HeaderMap, Bytes)>,
  answer: Arc<Mutex<StatusCode>>,
  task: tokio::task::JoinHandle<()>,
}

impl Drop for Receiver {
  fn drop(&mut self) {
    self.task.abort();
  }
}

async fn receiver(answer: StatusCode) -> Receiver {
  let (sent, posts) = mpsc::unbounded_channel();
  let answer = Arc::new(Mutex::new(answer));
  let answering = Arc::clone(&answer);
  let app = Router::new().route(
    "/hook",
    post(move |headers: HeaderMap, body: Bytes| {
      let sent = sent.clone();
      let answering = Arc::clone(&answering);
      async move {
        let _ = sent.send((headers, body));
        *answering.lock().expect("the answer is readable")
      }
    }),
  );
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
  let url = format!("http://{}/hook", listener.local_addr().expect("an address"));
  let task = tokio::spawn(async move {
    let _ = axum::serve(listener, app).await;
  });
  Receiver {
    url,
    posts,
    answer,
    task,
  }
}

impl Receiver {
  async fn next(&mut self) -> (HeaderMap, Event) {
    let (headers, body) = tokio::time::timeout(Duration::from_secs(10), self.posts.recv())
      .await
      .expect("a post comes")
      .expect("posted");
    (headers, serde_json::from_slice(&body).expect("an event"))
  }
}

/// Waits, within a bound, until the subscription stands in `state`.
async fn until_state(service: &Service, id: i64, state: SubscriptionState) -> Subscription {
  for _ in 0..400 {
    let subscription = service.client().subscription(id).await.expect("read");
    if subscription.state == state {
      return subscription;
    }
    tokio::time::sleep(Duration::from_millis(5)).await;
  }
  panic!("the subscription never stood {state}");
}

#[tokio::test]
async fn a_stream_subscription_hands_over_what_it_matches_once() {
  let service = Service::start().await;
  let client = service.client();
  service.open_item("build", "before").await;
  let subscription = client.subscribe(&to_build("harness")).await.expect("subscribed");
  let wanted = service.open_item("build", "wanted").await;
  service.open_item("docs", "elsewhere").await;

  let events = client.events_page(&page(&subscription)).await.expect("read").items;
  assert_eq!(events.len(), 1);
  assert_eq!((events[0].kind.as_str(), events[0].item_id()), ("item.created", Some(wanted.id)));
  assert!(
    client.events_page(&page(&subscription)).await.expect("read").items.is_empty(),
    "handed over once"
  );
  assert_eq!(
    client.subscription(subscription.id).await.expect("read").delivered_event_id,
    Some(events[0].id)
  );

  let mut stream =
    Box::pin(client.event_stream(&page(&subscription), None).await.expect("the stream opens"));
  let next = service.open_item("build", "streamed").await;
  let event = tokio::time::timeout(Duration::from_secs(10), stream.next())
    .await
    .expect("an event comes");
  assert_eq!(event.expect("an event").expect("read").item_id(), Some(next.id));
}

#[tokio::test]
async fn a_webhook_is_posted_each_event_signed() {
  let service = Service::start().await;
  let client = service.client();
  let mut hook = receiver(StatusCode::NO_CONTENT).await;
  let subscription = client
    .subscribe(&NewSubscription {
      url: Some(hook.url.clone()),
      secret: Some("shared".to_owned()),
      ..to_build("harness")
    })
    .await
    .expect("subscribed");
  let item = service.open_item("build", "posted").await;

  let (headers, event) = hook.next().await;
  assert_eq!(event.item_id(), Some(item.id));
  assert_eq!(headers[SUBSCRIPTION_HEADER], subscription.id.to_string().as_str());
  let body = serde_json::to_vec(&event).expect("an event renders");
  assert!(signed_by("shared", &body, headers[SIGNATURE_HEADER].to_str().expect("a header")));
  assert!(!signed_by("other", &body, headers[SIGNATURE_HEADER].to_str().expect("a header")));

  let stream = client.events_page(&page(&subscription)).await.expect_err("delivered to its url");
  assert_eq!(stream.code(), Some("conflict"));
}

#[tokio::test]
async fn a_failing_webhook_stops_and_resumes_from_where_it_stood() {
  let service = Service::start().await;
  let client = service.client();
  let mut hook = receiver(StatusCode::INTERNAL_SERVER_ERROR).await;
  let subscription = client
    .subscribe(&NewSubscription {
      url: Some(hook.url.clone()),
      ..to_build("harness")
    })
    .await
    .expect("subscribed");
  let item = service.open_item("build", "refused").await;
  for _ in 0..3 {
    assert_eq!(hook.next().await.1.item_id(), Some(item.id));
  }
  let failing = until_state(&service, subscription.id, SubscriptionState::Failing).await;
  assert_eq!((failing.failure_count, failing.delivered_event_id), (3, None));

  let refused = SubscriptionChange {
    state: Some(SubscriptionState::Failing),
    ..SubscriptionChange::default()
  };
  assert_eq!(
    client
      .change_subscription(subscription.id, &refused)
      .await
      .expect_err("only delivery")
      .code(),
    Some("conflict")
  );

  *hook.answer.lock().expect("the answer is writable") = StatusCode::OK;
  let active = SubscriptionChange {
    state: Some(SubscriptionState::Active),
    ..SubscriptionChange::default()
  };
  client.change_subscription(subscription.id, &active).await.expect("active again");
  assert_eq!(
    hook.next().await.1.item_id(),
    Some(item.id),
    "delivery starts again at the event it stopped at"
  );
}

#[tokio::test]
async fn a_failing_webhook_is_still_tried_and_the_first_post_taken_makes_it_active() {
  let service = Service::start_with(bmo_job::DeliveryPolicy {
    failing_retry: Duration::from_millis(20),
    ..support::quick_delivery()
  })
  .await;
  let client = service.client();
  let mut hook = receiver(StatusCode::SERVICE_UNAVAILABLE).await;
  let subscription = client
    .subscribe(&NewSubscription {
      url: Some(hook.url.clone()),
      ..to_build("harness")
    })
    .await
    .expect("subscribed");
  let item = service.open_item("build", "refused while down").await;
  for _ in 0..3 {
    assert_eq!(hook.next().await.1.item_id(), Some(item.id));
  }
  until_state(&service, subscription.id, SubscriptionState::Failing).await;
  assert_eq!(hook.next().await.1.item_id(), Some(item.id), "a failing subscription is tried again");

  *hook.answer.lock().expect("the answer is writable") = StatusCode::OK;
  let active = until_state(&service, subscription.id, SubscriptionState::Active).await;
  assert_eq!((active.failure_count, active.delivered_event_id.is_some()), (0, true));
  let told = EventQuery {
    kinds: Some("subscription.changed".to_owned()),
    ..EventQuery::default()
  };
  let changes = client.events_page(&told).await.expect("read").items;
  assert_eq!(changes.len(), 2, "told as failing, and as active again");
}

#[tokio::test]
async fn a_subscriber_down_longer_than_the_retries_gets_the_event_once_it_is_back() {
  let service = Service::start_with(bmo_job::DeliveryPolicy {
    failing_retry: Duration::from_millis(20),
    ..support::quick_delivery()
  })
  .await;
  let client = service.client();
  let mut hook = receiver(StatusCode::OK).await;
  let relay = support::Relay::before(&hook.url).await;
  let subscription = client
    .subscribe(&NewSubscription {
      url: Some(relay.url.clone()),
      ..to_build("harness")
    })
    .await
    .expect("subscribed");

  relay.go_down();

  let item = service.open_item("build", "told while the subscriber restarts").await;

  until_state(&service, subscription.id, SubscriptionState::Failing).await;
  relay.until_cut(4).await;
  relay.come_up();

  assert_eq!(
    hook.next().await.1.item_id(),
    Some(item.id),
    "the event goes out once the subscriber is back"
  );
  let active = until_state(&service, subscription.id, SubscriptionState::Active).await;
  assert!(active.delivered_event_id.is_some());
}

#[tokio::test]
async fn a_post_left_unanswered_counts_as_a_failure() {
  let service = Service::start_with(bmo_job::DeliveryPolicy {
    answer_within: Duration::from_millis(50),
    ..support::quick_delivery()
  })
  .await;
  let client = service.client();
  let (sent, mut posts) = mpsc::unbounded_channel();
  let app = Router::new().route(
    "/hook",
    post(move || {
      let sent = sent.clone();
      async move {
        let _ = sent.send(());
        std::future::pending::<StatusCode>().await
      }
    }),
  );
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
  let url = format!("http://{}/hook", listener.local_addr().expect("an address"));
  let task = tokio::spawn(async move {
    let _ = axum::serve(listener, app).await;
  });
  let subscription = client
    .subscribe(&NewSubscription {
      url: Some(url),
      ..to_build("harness")
    })
    .await
    .expect("subscribed");
  service.open_item("build", "never answered").await;
  for _ in 0..3 {
    tokio::time::timeout(Duration::from_secs(10), posts.recv()).await.expect("a post comes");
  }
  let failing = until_state(&service, subscription.id, SubscriptionState::Failing).await;
  assert_eq!(failing.failure_count, 3);
  task.abort();
}

#[tokio::test]
async fn a_subscription_runs_out_unless_renewed() {
  let service = Service::start().await;
  let client = service.client();
  let subscription = client
    .subscribe(&NewSubscription {
      ttl_ms: Some(10_000),
      ..to_build("harness")
    })
    .await
    .expect("subscribed");
  service.pass(9_000).await;
  let renewed = client
    .change_subscription(subscription.id, &SubscriptionChange::default())
    .await
    .expect("renewed");
  assert_eq!(renewed.expires_at, Some(support::START + 19_000));
  service.pass(10_000).await;
  assert_eq!(client.subscription(subscription.id).await.expect_err("ran out").status(), Some(410));
  let deleted = EventQuery {
    kinds: Some("subscription.deleted".to_owned()),
    ..EventQuery::default()
  };
  assert_eq!(client.events_page(&deleted).await.expect("read").items.len(), 1);

  let refused = NewSubscription {
    kinds: vec!["item.vanished".to_owned()],
    ..to_build("harness")
  };
  assert_eq!(client.subscribe(&refused).await.expect_err("no such kind").code(), Some("bad-request"));
  let file = NewSubscription {
    url: Some("file:///tmp/x".to_owned()),
    ..to_build("harness")
  };
  assert_eq!(client.subscribe(&file).await.expect_err("not http").code(), Some("bad-request"));
}
