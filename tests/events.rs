//! The event stream through the real server: narrowed by kind and scope, published by the service's own
//! process, and kept for its window (docs/api.md, "Events").

use crate::support;

use bmo_job::store::EVENT_WINDOW_MS;
use bmo_job_client::{EventQuery, NewEvent, NewItem};
use serde_json::json;

use support::Service;

#[tokio::test]
async fn events_are_narrowed_by_kind_and_scope() {
  let service = Service::start().await;
  let client = service.client();
  let scoped = client
    .create_item(&NewItem {
      title: "scoped".into(),
      queue_name: "build".into(),
      scope: Some("project-7".into()),
      ..NewItem::default()
    })
    .await
    .expect("raised");
  service.open_item("build", "unscoped").await;

  let created = EventQuery {
    kinds: Some("item.created".into()),
    ..EventQuery::default()
  };
  assert_eq!(client.events_page(&created).await.expect("read").items.len(), 2);
  let in_scope = EventQuery {
    scope: Some("project-7".into()),
    ..EventQuery::default()
  };
  let events = client.events_page(&in_scope).await.expect("read").items;
  assert_eq!(events.len(), 1);
  assert_eq!((events[0].kind.as_str(), events[0].item_id()), ("item.created", Some(scoped.id)));

  let all = client.events_page(&EventQuery::default()).await.expect("read").items;
  let past = EventQuery {
    after: Some(all[0].id),
    limit: Some(1),
    ..EventQuery::default()
  };
  let page = client.events_page(&past).await.expect("read");
  assert_eq!((page.items[0].id, page.more), (all[1].id, true));
}

#[tokio::test]
async fn a_process_of_the_service_publishes_an_event_narrowed_by_its_data() {
  let service = Service::start().await;
  let client = service.client();
  let data = json!({ "queueName": "build", "scope": "project-7", "note": "moved by hand" });
  let event = client
    .publish_event(&NewEvent {
      kind: "queue.moved".into(),
      data: data.as_object().expect("an object").clone(),
    })
    .await
    .expect("published");
  assert_eq!(event.queue_name(), Some("build"));
  let in_scope = EventQuery {
    scope: Some("project-7".into()),
    ..EventQuery::default()
  };
  assert_eq!(client.events_page(&in_scope).await.expect("read").items, [event]);
  let refused = NewEvent {
    kind: "Moved".into(),
    data: Default::default(),
  };
  assert_eq!(
    client.publish_event(&refused).await.expect_err("not a kind").code(),
    Some("bad-request")
  );
}

#[tokio::test]
async fn a_cursor_before_the_window_is_gone() {
  let service = Service::start().await;
  let client = service.client();
  service.open_item("build", "old").await;
  service.pass(EVENT_WINDOW_MS + 1).await;
  service.open_item("build", "new").await;
  let from_start = EventQuery {
    after: Some(0),
    ..EventQuery::default()
  };
  let refused = client.events_page(&from_start).await.expect_err("before the window");
  assert_eq!((refused.status(), refused.code()), (Some(410), Some("gone")));
  let kept = client
    .events_page(&EventQuery::default())
    .await
    .expect("a page with no cursor reads the window");
  assert!(
    kept.items.iter().all(|event| event.at > support::START),
    "only what the window holds: {kept:?}"
  );
  assert_eq!(kept.items.len(), 2, "the new item's making and opening");
}

#[tokio::test]
async fn the_stream_opens_on_events_still_to_come() {
  let service = Service::start().await;
  let client = service.client();
  service.open_item("build", "before").await;
  let mut stream = Box::pin(client.event_stream(&EventQuery::default(), None).await.expect("opens"));
  let item = service.open_item("build", "after").await;
  let first =
    tokio::time::timeout(std::time::Duration::from_secs(10), futures_util::StreamExt::next(&mut stream))
      .await
      .expect("an event comes")
      .expect("an event")
      .expect("read");
  assert_eq!((first.kind.as_str(), first.item_id()), ("item.created", Some(item.id)));
}
