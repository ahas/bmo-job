//! Webhook delivery: each active subscription with a url has its matching events posted there in event order,
//! at least once, signed where it has a secret, and moved to failing after a run of failures; a failing one is
//! still tried at a slow pace, and moved back to active by the first post taken (docs/api.md, "Subscriptions").
//! It is the one call the service makes.

use std::collections::HashMap;
use std::time::Duration;

use bmo_job_client::{Code, Event, SIGNATURE_HEADER, SUBSCRIPTION_HEADER, SubscriptionState, signature};
use tokio::task::JoinHandle;

use crate::store::{Delivery, EventFilter, Store};

/// How delivery paces its retries and when it counts a subscription failing.
#[derive(Debug, Clone)]
pub struct DeliveryPolicy {
  /// The wait before the first retry, doubled after each failure.
  pub first_retry: Duration,
  /// The longest wait between retries.
  pub longest_retry: Duration,
  /// How many failures in a row move a subscription to failing.
  pub failures_to_failing: i64,
  /// How long a post waits for its answer before it counts as a failure.
  pub answer_within: Duration,
  /// The wait between posts to a failing subscription.
  pub failing_retry: Duration,
}

impl Default for DeliveryPolicy {
  fn default() -> Self {
    Self {
      first_retry: Duration::from_secs(1),
      longest_retry: Duration::from_secs(60),
      failures_to_failing: 10,
      answer_within: Duration::from_secs(60),
      failing_retry: Duration::from_secs(300),
    }
  }
}

/// How many events one delivery reading takes.
const DELIVERY_BATCH: i64 = 50;

/// How often the deliverer looks again for subscriptions to deliver where nothing changed.
const LOOK_AGAIN: Duration = Duration::from_secs(1);

/// How often a quiet connection to a subscriber is probed, so a subscriber gone is told from one still answering.
const KEEP_ALIVE: Duration = Duration::from_secs(30);

/// Keeps one delivery running for each active or failing subscription with a url, until the service stops.
pub async fn deliver(store: Store, policy: DeliveryPolicy) {
  let http = reqwest::Client::builder()
    .tcp_keepalive(KEEP_ALIVE)
    .build()
    .expect("an HTTP client builds");
  let mut running: HashMap<i64, JoinHandle<()>> = HashMap::new();
  let mut changes = store.changes();
  loop {
    changes.borrow_and_update();
    running.retain(|_, delivery| !delivery.is_finished());
    if let Ok(ids) = store.webhook_subscriptions().await {
      for id in ids {
        running
          .entry(id)
          .or_insert_with(|| tokio::spawn(deliver_one(store.clone(), http.clone(), policy.clone(), id)));
      }
    }
    let _ = tokio::time::timeout(LOOK_AGAIN, changes.changed()).await;
  }
}

/// What one event's posting came to.
enum Posted {
  /// The subscriber took it.
  Taken,
  /// The subscription is failing, and its delivery has rested the slow pace before trying again.
  Rested,
  /// Delivery of the subscription stops: it ended, was paused, or its record could not be written.
  Stopped,
}

/// Delivers one subscription's events until it ends, pauses or loses its url. A failing one goes on at the slow
/// pace.
async fn deliver_one(store: Store, http: reqwest::Client, policy: DeliveryPolicy, id: i64) {
  let mut changes = store.changes();
  loop {
    changes.borrow_and_update();
    let Ok(kept) = store.kept_subscription(id).await else {
      return;
    };
    let Some(url) = kept.subscription.url.clone() else {
      return;
    };
    if !matches!(kept.subscription.state, SubscriptionState::Active | SubscriptionState::Failing) {
      return;
    }
    let events = match store.events_for(&kept, &EventFilter::default(), None, DELIVERY_BATCH).await {
      Ok(events) => events,
      Err(refusal) if refusal.code == Code::Gone => {
        // The next event has left the window: nothing can be delivered in order any more, and it stays so.
        if !matches!(store.delivery_failed(id, 1).await, Ok(Delivery::Failing)) {
          return;
        }
        rest_while_failing(&store, id, policy.failing_retry).await;
        continue;
      }
      Err(_) => {
        let _ = tokio::time::timeout(LOOK_AGAIN, changes.changed()).await;
        continue;
      }
    };
    if events.is_empty() {
      let _ = tokio::time::timeout(LOOK_AGAIN, changes.changed()).await;
      continue;
    }
    for event in events.iter().take(DELIVERY_BATCH as usize) {
      match post_until_taken(&store, &http, &policy, id, &url, kept.secret.as_deref(), event).await {
        Posted::Taken => {}
        Posted::Rested => break,
        Posted::Stopped => return,
      }
    }
  }
}

/// Posts one event until it is acknowledged, at the quick pace while the subscription is active; once it is
/// failing, rests the slow pace and hands back, for its state to be read again before the next try.
async fn post_until_taken(
  store: &Store,
  http: &reqwest::Client,
  policy: &DeliveryPolicy,
  id: i64,
  url: &str,
  secret: Option<&str>,
  event: &Event,
) -> Posted {
  let body = serde_json::to_vec(event).expect("an event renders as JSON");
  let mut retry = policy.first_retry;
  loop {
    let mut request = http
      .post(url)
      .timeout(policy.answer_within)
      .header(reqwest::header::CONTENT_TYPE, "application/json")
      .header(SUBSCRIPTION_HEADER, id.to_string())
      .body(body.clone());
    if let Some(secret) = secret {
      request = request.header(SIGNATURE_HEADER, signature(secret, &body));
    }
    let taken = matches!(request.send().await, Ok(answer) if answer.status().is_success());
    if taken {
      return match store.delivered(id, event.id).await {
        Ok(()) => Posted::Taken,
        Err(_) => Posted::Stopped,
      };
    }
    match store.delivery_failed(id, policy.failures_to_failing).await {
      Ok(Delivery::Going) => {}
      Ok(Delivery::Failing) => {
        rest_while_failing(store, id, policy.failing_retry).await;
        return Posted::Rested;
      }
      _ => return Posted::Stopped,
    }
    tokio::time::sleep(retry).await;
    retry = (retry * 2).min(policy.longest_retry);
  }
}

/// Rests a failing subscription's delivery for `pace`, or until its state is moved from failing.
async fn rest_while_failing(store: &Store, id: i64, pace: Duration) {
  let mut changes = store.changes();
  let until = tokio::time::Instant::now() + pace;
  loop {
    changes.borrow_and_update();
    let failing = store
      .kept_subscription(id)
      .await
      .is_ok_and(|kept| kept.subscription.state == SubscriptionState::Failing);
    if !failing {
      return;
    }
    match tokio::time::timeout_at(until, changes.changed()).await {
      Ok(Ok(())) => {}
      Ok(Err(_)) => {
        tokio::time::sleep_until(until).await;
        return;
      }
      Err(_) => return,
    }
  }
}
