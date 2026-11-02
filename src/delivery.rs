//! Webhook delivery: each active subscription with a url has its matching events posted there in event order,
//! at least once, signed where it has a secret, and moved to failing after a run of failures (docs/api.md,
//! "Subscriptions"). It is the one call the service makes.

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
}

impl Default for DeliveryPolicy {
  fn default() -> Self {
    Self {
      first_retry: Duration::from_secs(1),
      longest_retry: Duration::from_secs(60),
      failures_to_failing: 10,
    }
  }
}

/// How many events one delivery reading takes.
const DELIVERY_BATCH: i64 = 50;

/// How often the deliverer looks again for subscriptions to deliver where nothing changed.
const LOOK_AGAIN: Duration = Duration::from_secs(1);

/// How often a quiet connection to a subscriber is probed, so a subscriber gone is told from one still answering.
const KEEP_ALIVE: Duration = Duration::from_secs(30);

/// Keeps one delivery running for each active subscription with a url, until the service stops.
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

/// Delivers one subscription's events until it ends, pauses, moves to failing or loses its url.
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
    if kept.subscription.state != SubscriptionState::Active {
      return;
    }
    let events = match store.events_for(&kept, &EventFilter::default(), None, DELIVERY_BATCH).await {
      Ok(events) => events,
      Err(refusal) if refusal.code == Code::Gone => {
        // The next event has left the window: nothing can be delivered in order any more.
        let _ = store.delivery_failed(id, 1).await;
        return;
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
      if !post_until_taken(&store, &http, &policy, id, &url, kept.secret.as_deref(), event).await {
        return;
      }
    }
  }
}

/// Posts one event until it is acknowledged; false where delivery stops first.
async fn post_until_taken(
  store: &Store,
  http: &reqwest::Client,
  policy: &DeliveryPolicy,
  id: i64,
  url: &str,
  secret: Option<&str>,
  event: &Event,
) -> bool {
  let body = serde_json::to_vec(event).expect("an event renders as JSON");
  let mut retry = policy.first_retry;
  loop {
    let mut request = http
      .post(url)
      .header(reqwest::header::CONTENT_TYPE, "application/json")
      .header(SUBSCRIPTION_HEADER, id.to_string())
      .body(body.clone());
    if let Some(secret) = secret {
      request = request.header(SIGNATURE_HEADER, signature(secret, &body));
    }
    let taken = matches!(request.send().await, Ok(answer) if answer.status().is_success());
    if taken {
      return store.delivered(id, event.id).await.is_ok();
    }
    match store.delivery_failed(id, policy.failures_to_failing).await {
      Ok(Delivery::Going) => {}
      _ => return false,
    }
    tokio::time::sleep(retry).await;
    retry = (retry * 2).min(policy.longest_retry);
  }
}
