//! What the MCP server keeps for its harness (docs/api.md, "What the server keeps for its harness" and
//! "Notifications"): the leases and subscriptions it took, renewed at half their span and let go when the
//! session ends, and each subscription read in the stream form, its events sent to the harness.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bmo_job_client::{
  ClientFailure, Event, EventKind, EventQuery, JobClient, LeaseRenewal, NewSubscription, Subscription,
  SubscriptionChange,
};
use futures_util::StreamExt;
use rmcp::model::{CustomNotification, ResourceUpdatedNotificationParam, ServerNotification};
use rmcp::{Peer, RoleServer};
use serde_json::{Map, Value, json};
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

/// How often the server looks for what to renew.
const LOOK_EVERY: Duration = Duration::from_secs(1);

/// How long the server waits before reading a stream again after it ended or failed.
const READ_AGAIN: Duration = Duration::from_secs(1);

/// The span of the subscription the server makes for queues named when it starts.
const START_SPAN_MS: i64 = 60_000;

/// A member the server renews: its span and when it was last renewed.
#[derive(Debug, Clone, Copy)]
struct Span {
  ttl_ms: Option<i64>,
  renewed: Instant,
}

impl Span {
  fn new(ttl_ms: Option<i64>) -> Self {
    Self {
      ttl_ms,
      renewed: Instant::now(),
    }
  }

  /// Whether half the span has passed since it was last renewed.
  fn due(&self, now: Instant) -> Option<i64> {
    let ttl_ms = self.ttl_ms?;
    (now.duration_since(self.renewed) >= Duration::from_millis(ttl_ms as u64 / 2)).then_some(ttl_ms)
  }
}

#[derive(Debug, Default)]
struct Kept {
  leases: HashMap<i64, Span>,
  subscriptions: HashMap<i64, Span>,
  reading: HashMap<i64, JoinHandle<()>>,
}

/// The server's side of one harness session.
#[derive(Debug)]
pub struct Harness {
  pub client: JobClient,
  kept: Mutex<Kept>,
  peer: watch::Sender<Option<Peer<RoleServer>>>,
  channel: Option<String>,
}

/// Whether a failure says the member is no longer there to renew or read.
fn ended(failure: &ClientFailure) -> bool {
  matches!(failure.status(), Some(404 | 409 | 410))
}

impl Harness {
  /// The server's side of a session reaching the service through `client`, pushing events on `channel`
  /// where one is named.
  pub fn new(client: JobClient, channel: Option<String>) -> Arc<Self> {
    Arc::new(Self {
      client,
      kept: Mutex::new(Kept::default()),
      peer: watch::channel(None).0,
      channel,
    })
  }

  /// The channel events are pushed on as messages, where the server was started with one.
  pub fn channel(&self) -> Option<&str> {
    self.channel.as_deref()
  }

  /// The harness the notifications go to, once its session has started.
  pub fn reached(&self, peer: Peer<RoleServer>) {
    self.peer.send_replace(Some(peer));
  }

  fn kept(&self) -> std::sync::MutexGuard<'_, Kept> {
    self.kept.lock().expect("the kept members are never left half written")
  }

  /// Keeps a lease the harness took, renewing it until it ends.
  pub fn keep_lease(&self, id: i64, ttl_ms: i64) {
    self.kept().leases.insert(id, Span::new(Some(ttl_ms)));
  }

  /// Notes a lease renewed by the harness itself.
  pub fn renewed_lease(&self, id: i64, ttl_ms: i64) {
    if let Some(span) = self.kept().leases.get_mut(&id) {
      *span = Span::new(Some(ttl_ms));
    }
  }

  /// Stops keeping a lease the harness gave back.
  pub fn let_lease_go(&self, id: i64) {
    self.kept().leases.remove(&id);
  }

  /// Keeps a subscription the harness made, renewing it while it has a span, and reads it as a stream where
  /// it has no url.
  pub fn keep_subscription(self: &Arc<Self>, subscription: &Subscription) {
    let mut kept = self.kept();
    kept.subscriptions.insert(subscription.id, Span::new(subscription.ttl_ms));
    if subscription.url.is_none() {
      let reading = tokio::spawn(Arc::clone(self).read(subscription.id));
      if let Some(earlier) = kept.reading.insert(subscription.id, reading) {
        earlier.abort();
      }
    }
  }

  /// Stops keeping a subscription the harness ended.
  pub fn let_subscription_go(&self, id: i64) {
    let mut kept = self.kept();
    kept.subscriptions.remove(&id);
    if let Some(reading) = kept.reading.remove(&id) {
      reading.abort();
    }
  }

  /// Subscribes, for the holder, to items made and changed in the queues named when the server started.
  pub async fn subscribe_to_queues(
    self: &Arc<Self>,
    holder_name: &str,
    queue_names: &[String],
  ) -> Result<(), ClientFailure> {
    let subscription = self
      .client
      .subscribe(&NewSubscription {
        holder_name: holder_name.to_owned(),
        kinds: vec![EventKind::ItemCreated.as_str().to_owned(), EventKind::ItemChanged.as_str().to_owned()],
        queue_names: queue_names.to_vec(),
        ttl_ms: Some(START_SPAN_MS),
        ..NewSubscription::default()
      })
      .await?;
    self.keep_subscription(&subscription);
    Ok(())
  }

  /// Renews what is due, for as long as the session lasts.
  pub async fn renew(self: Arc<Self>) {
    let mut every = tokio::time::interval(LOOK_EVERY);
    loop {
      every.tick().await;
      let now = Instant::now();
      let (leases, subscriptions): (Vec<(i64, i64)>, Vec<i64>) = {
        let kept = self.kept();
        (
          kept
            .leases
            .iter()
            .filter_map(|(id, span)| span.due(now).map(|ttl| (*id, ttl)))
            .collect(),
          kept.subscriptions.iter().filter_map(|(id, span)| span.due(now).map(|_| *id)).collect(),
        )
      };
      for (id, ttl_ms) in leases {
        match self.client.renew_lease(id, &LeaseRenewal { ttl_ms }).await {
          Ok(_) => self.renewed_lease(id, ttl_ms),
          Err(failure) if ended(&failure) => self.let_lease_go(id),
          Err(_) => {}
        }
      }
      for id in subscriptions {
        match self.client.change_subscription(id, &SubscriptionChange::default()).await {
          Ok(renewed) => {
            if let Some(span) = self.kept().subscriptions.get_mut(&id) {
              *span = Span::new(renewed.ttl_ms);
            }
          }
          Err(failure) if ended(&failure) => self.let_subscription_go(id),
          Err(_) => {}
        }
      }
    }
  }

  /// Gives back every lease and deletes every subscription the server kept, as the session ends.
  pub async fn let_go(&self) {
    let (leases, subscriptions): (Vec<i64>, Vec<i64>) = {
      let mut kept = self.kept();
      kept.reading.drain().for_each(|(_, reading)| reading.abort());
      (
        kept.leases.drain().map(|(id, _)| id).collect(),
        kept.subscriptions.drain().map(|(id, _)| id).collect(),
      )
    };
    for id in leases {
      let _ = self.client.release_lease(id).await;
    }
    for id in subscriptions {
      let _ = self.client.unsubscribe(id).await;
    }
  }

  /// Reads a subscription in the stream form and sends each event to the harness, until it ends.
  async fn read(self: Arc<Self>, id: i64) {
    let query = EventQuery {
      subscription_id: Some(id),
      ..EventQuery::default()
    };
    loop {
      match self.client.event_stream(&query, None).await {
        Ok(events) => {
          let mut events = Box::pin(events);
          while let Some(Ok(event)) = events.next().await {
            self.tell(&event).await;
          }
        }
        Err(failure) if ended(&failure) => {
          self.kept().subscriptions.remove(&id);
          return;
        }
        Err(_) => {}
      }
      tokio::time::sleep(READ_AGAIN).await;
    }
  }

  /// Sends one event to the harness: the resources it updates, and the event itself on the channel.
  async fn tell(&self, event: &Event) {
    let mut reached = self.peer.subscribe();
    let peer = loop {
      if let Some(peer) = reached.borrow_and_update().clone() {
        break peer;
      }
      if reached.changed().await.is_err() {
        return;
      }
    };
    for uri in updated_resources(event) {
      let _ = peer.notify_resource_updated(ResourceUpdatedNotificationParam::new(uri)).await;
    }
    if let Some(channel) = &self.channel {
      let message = pushed_message(event);
      let notification = CustomNotification::new(format!("notifications/{channel}"), Some(message));
      let _ = peer.send_notification(ServerNotification::CustomNotification(notification)).await;
    }
  }
}

/// The resources an event updates: its item's and its queue's.
pub fn updated_resources(event: &Event) -> Vec<String> {
  let mut uris = Vec::new();
  if let Some(item_id) = event.item_id() {
    uris.push(format!("job://items/{item_id}"));
  }
  if let Some(queue_name) = event.queue_name() {
    uris.push(format!("job://queues/{queue_name}"));
  }
  uris
}

/// An event as a message pushed into the session: the event as JSON text, and its kind, number, item and
/// queue as the message's metadata.
pub fn pushed_message(event: &Event) -> Value {
  let mut meta = Map::new();
  meta.insert("kind".into(), json!(event.kind));
  meta.insert("eventId".into(), json!(event.id.to_string()));
  if let Some(item_id) = event.item_id() {
    meta.insert("itemId".into(), json!(item_id.to_string()));
  }
  if let Some(queue_name) = event.queue_name() {
    meta.insert("queueName".into(), json!(queue_name));
  }
  json!({
    "content": serde_json::to_string(event).expect("an event renders as JSON"),
    "meta": meta,
  })
}
