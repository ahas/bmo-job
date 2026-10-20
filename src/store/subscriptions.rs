//! Subscriptions: what each subscriber hears of, where its delivery stands, and its span (docs/api.md,
//! "Subscriptions").

use bmo_job_client::{
  Event, EventKind, NewSubscription, Page, Subscription, SubscriptionChange, SubscriptionQuery,
  SubscriptionState,
};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::events::{EventFilter, EventKeys, emit, key_alone, last_event_id, read_events};
use super::{Store, Tx, filled, from_json, listed, page_limit, paged, queue_name, stored, to_json};
use crate::refusal::Refusal;

/// The span a subscription may be made or renewed for.
const SUBSCRIPTION_SPAN_MS: std::ops::RangeInclusive<i64> = 1_000..=86_400_000;

const SUBSCRIPTION_COLUMNS: &str = "id, holder_name, kinds, queue_names, scope, item_ids, url, secret, state, ttl_ms, \
  expires_at, failure_count, delivered_event_id, start_event_id, labels, created_at, changed_at, ended_at";

/// A subscription as it is kept: the member, its secret, where its delivery starts and when it ended.
#[derive(Debug, Clone)]
pub struct Kept {
  pub subscription: Subscription,
  pub secret: Option<String>,
  start_event_id: i64,
  ended_at: Option<i64>,
}

impl Kept {
  /// The number of the last event handed over, or of the last before the subscription was made.
  pub fn cursor(&self) -> i64 {
    self.subscription.delivered_event_id.unwrap_or(0).max(self.start_event_id)
  }

  /// What the subscription matches.
  pub fn filter(&self) -> EventFilter {
    EventFilter {
      kinds: self.subscription.kinds.clone(),
      queue_names: self.subscription.queue_names.clone(),
      scope: self.subscription.scope.clone(),
      item_ids: self.subscription.item_ids.clone(),
    }
  }
}

/// Where one webhook's delivery stands after an attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delivery {
  /// The subscription goes on being delivered.
  Going,
  /// The subscription ended, was paused, moved to failing, or no longer posts to a url.
  Stopped,
}

fn kept_from(row: &SqliteRow) -> Result<Kept, Refusal> {
  Ok(Kept {
    subscription: Subscription {
      id: row.try_get("id")?,
      holder_name: row.try_get("holder_name")?,
      kinds: from_json(row.try_get::<&str, _>("kinds")?)?,
      queue_names: from_json(row.try_get::<&str, _>("queue_names")?)?,
      scope: row.try_get("scope")?,
      item_ids: from_json(row.try_get::<&str, _>("item_ids")?)?,
      url: row.try_get("url")?,
      state: stored(row.try_get("state")?, SubscriptionState::parse)?,
      ttl_ms: row.try_get("ttl_ms")?,
      expires_at: row.try_get("expires_at")?,
      failure_count: row.try_get("failure_count")?,
      delivered_event_id: row.try_get("delivered_event_id")?,
      labels: from_json(row.try_get::<&str, _>("labels")?)?,
      created_at: row.try_get("created_at")?,
      changed_at: row.try_get("changed_at")?,
    },
    secret: row.try_get("secret")?,
    start_event_id: row.try_get("start_event_id")?,
    ended_at: row.try_get("ended_at")?,
  })
}

async fn kept_at(tx: &mut Tx, id: i64) -> Result<Kept, Refusal> {
  let row = sqlx::query(&format!("SELECT {SUBSCRIPTION_COLUMNS} FROM subscriptions WHERE id = $1"))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Refusal::not_found(format!("There is no subscription {id}.")))?;
  kept_from(&row)
}

/// The subscription at `id`, standing; one that ended is gone.
async fn standing(tx: &mut Tx, id: i64) -> Result<Kept, Refusal> {
  let kept = kept_at(tx, id).await?;
  if kept.ended_at.is_some() {
    return Err(Refusal::gone(format!("The subscription {id} has ended.")));
  }
  Ok(kept)
}

async fn save(tx: &mut Tx, kept: &Kept) -> Result<(), Refusal> {
  let subscription = &kept.subscription;
  sqlx::query(
    "UPDATE subscriptions SET kinds = $1, queue_names = $2, scope = $3, item_ids = $4, state = $5, ttl_ms = $6, \
     expires_at = $7, failure_count = $8, delivered_event_id = $9, changed_at = $10, ended_at = $11 WHERE id = $12",
  )
  .bind(to_json(&subscription.kinds))
  .bind(to_json(&subscription.queue_names))
  .bind(&subscription.scope)
  .bind(to_json(&subscription.item_ids))
  .bind(subscription.state.as_str())
  .bind(subscription.ttl_ms)
  .bind(subscription.expires_at)
  .bind(subscription.failure_count)
  .bind(subscription.delivered_event_id)
  .bind(subscription.changed_at)
  .bind(kept.ended_at)
  .bind(subscription.id)
  .execute(&mut **tx)
  .await?;
  Ok(())
}

fn keys_of(subscription: &Subscription) -> EventKeys {
  EventKeys {
    scope: subscription.scope.clone(),
    ..EventKeys::default()
  }
}

async fn told(tx: &mut Tx, at: i64, kind: EventKind, subscription: &Subscription) -> Result<(), Refusal> {
  emit(tx, at, kind.as_str(), subscription, &keys_of(subscription)).await?;
  Ok(())
}

/// Ends a standing subscription and tells it as deleted.
pub(crate) async fn end_subscription(tx: &mut Tx, kept: &mut Kept, now: i64) -> Result<(), Refusal> {
  kept.ended_at = Some(now);
  kept.subscription.changed_at = now;
  save(tx, kept).await?;
  let keys = keys_of(&kept.subscription);
  emit(
    tx,
    now,
    EventKind::SubscriptionDeleted.as_str(),
    &key_alone(kept.subscription.id, None),
    &keys,
  )
  .await?;
  Ok(())
}

/// Ends every subscription past its span at `now`.
pub(crate) async fn expire_subscriptions(tx: &mut Tx, now: i64) -> Result<usize, Refusal> {
  let rows = sqlx::query(&format!(
    "SELECT {SUBSCRIPTION_COLUMNS} FROM subscriptions WHERE ended_at IS NULL AND expires_at IS NOT NULL \
     AND expires_at <= $1"
  ))
  .bind(now)
  .fetch_all(&mut **tx)
  .await?;
  for row in &rows {
    let mut kept = kept_from(row)?;
    end_subscription(tx, &mut kept, now).await?;
  }
  Ok(rows.len())
}

/// The standing subscriptions held under `scope`.
pub(crate) async fn under_scope(tx: &mut Tx, scope: &str) -> Result<Vec<Kept>, Refusal> {
  let rows = sqlx::query(&format!(
    "SELECT {SUBSCRIPTION_COLUMNS} FROM subscriptions WHERE ended_at IS NULL AND scope = $1"
  ))
  .bind(scope)
  .fetch_all(&mut **tx)
  .await?;
  rows.iter().map(kept_from).collect()
}

fn kinds_read(kinds: &[String]) -> Result<(), Refusal> {
  for kind in kinds {
    if EventKind::parse(kind).is_none() {
      return Err(Refusal::bad_request(format!("The kind {kind:?} is not one of the service's events.")));
    }
  }
  Ok(())
}

fn queues_read(names: &[String]) -> Result<(), Refusal> {
  names.iter().try_for_each(|name| queue_name(name))
}

fn span(ttl_ms: i64) -> Result<(), Refusal> {
  if !SUBSCRIPTION_SPAN_MS.contains(&ttl_ms) {
    return Err(Refusal::bad_request("A subscription's ttlMs is 1000 to 86400000."));
  }
  Ok(())
}

fn url_read(url: &str) -> Result<(), Refusal> {
  match url::Url::parse(url) {
    Ok(parsed) if matches!(parsed.scheme(), "http" | "https") && parsed.has_host() => Ok(()),
    _ => Err(Refusal::bad_request(format!(
      "The url {url:?} is not an absolute http or https address."
    ))),
  }
}

impl Store {
  /// Subscriptions standing now, filtered as the query asks.
  pub async fn subscriptions(&self, query: &SubscriptionQuery) -> Result<Page<Subscription>, Refusal> {
    let limit = page_limit(query.limit)?;
    let states = listed("state", query.state.as_deref(), SubscriptionState::parse)?;
    let states: Vec<&str> = states.iter().map(|state| state.as_str()).collect();
    let mut tx = self.begin().await?;
    let rows = sqlx::query(&format!(
      "SELECT {SUBSCRIPTION_COLUMNS} FROM subscriptions WHERE ended_at IS NULL AND id > $1 \
       AND ($2 IS NULL OR holder_name = $2) AND ($3 = '[]' OR state IN (SELECT value FROM json_each($3))) \
       ORDER BY id LIMIT $4"
    ))
    .bind(query.after.unwrap_or(0))
    .bind(&query.holder_name)
    .bind(to_json(&states))
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let subscriptions = rows
      .iter()
      .map(|row| kept_from(row).map(|kept| kept.subscription))
      .collect::<Result<Vec<_>, _>>()?;
    Ok(paged(subscriptions, limit))
  }

  /// Makes a subscription, delivered from the first event after its own making.
  pub async fn subscribe(&self, new: NewSubscription) -> Result<Subscription, Refusal> {
    filled("holderName", &new.holder_name)?;
    kinds_read(&new.kinds)?;
    queues_read(&new.queue_names)?;
    if let Some(url) = &new.url {
      url_read(url)?;
    }
    if let Some(ttl_ms) = new.ttl_ms {
      span(ttl_ms)?;
    }
    let now = self.now();
    let mut tx = self.begin().await?;
    let id: i64 = sqlx::query_scalar(
      "INSERT INTO subscriptions (holder_name, kinds, queue_names, scope, item_ids, url, secret, state, ttl_ms, \
       expires_at, failure_count, start_event_id, labels, created_at, changed_at) \
       VALUES ($1, $2, $3, $4, $5, $6, $7, 'active', $8, $9, 0, 0, $10, $11, $11) RETURNING id",
    )
    .bind(&new.holder_name)
    .bind(to_json(&new.kinds))
    .bind(to_json(&new.queue_names))
    .bind(&new.scope)
    .bind(to_json(&new.item_ids))
    .bind(&new.url)
    .bind(&new.secret)
    .bind(new.ttl_ms)
    .bind(new.ttl_ms.map(|ttl| now + ttl))
    .bind(to_json(&new.labels))
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    let subscription = kept_at(&mut tx, id).await?.subscription;
    told(&mut tx, now, EventKind::SubscriptionCreated, &subscription).await?;
    let start = last_event_id(&mut tx).await?;
    sqlx::query("UPDATE subscriptions SET start_event_id = $1 WHERE id = $2")
      .bind(start)
      .bind(id)
      .execute(&mut *tx)
      .await?;
    self.commit(tx).await?;
    Ok(subscription)
  }

  pub async fn subscription(&self, id: i64) -> Result<Subscription, Refusal> {
    let mut tx = self.begin().await?;
    let kept = standing(&mut tx, id).await?;
    tx.commit().await?;
    Ok(kept.subscription)
  }

  /// Renews a subscription, changes what it hears of, or moves its state.
  pub async fn change_subscription(
    &self,
    id: i64,
    change: SubscriptionChange,
  ) -> Result<Subscription, Refusal> {
    if let Some(kinds) = &change.kinds {
      kinds_read(kinds)?;
    }
    if let Some(names) = &change.queue_names {
      queues_read(names)?;
    }
    if let Some(ttl_ms) = change.ttl_ms {
      span(ttl_ms)?;
    }
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut kept = standing(&mut tx, id).await?;
    let subscription = &mut kept.subscription;
    if change.state == Some(SubscriptionState::Failing) {
      return Err(Refusal::conflict("Only delivery moves a subscription to failing."));
    }
    if let Some(state) = change.state
      && state != subscription.state
    {
      if subscription.state == SubscriptionState::Failing {
        subscription.failure_count = 0;
      }
      subscription.state = state;
    }
    if let Some(kinds) = change.kinds {
      subscription.kinds = kinds;
    }
    if let Some(names) = change.queue_names {
      subscription.queue_names = names;
    }
    if let Some(ids) = change.item_ids {
      subscription.item_ids = ids;
    }
    if let Some(scope) = change.scope {
      subscription.scope = scope;
    }
    if let Some(ttl_ms) = change.ttl_ms {
      subscription.ttl_ms = Some(ttl_ms);
    }
    subscription.expires_at = subscription.ttl_ms.map(|ttl| now + ttl);
    subscription.changed_at = now;
    save(&mut tx, &kept).await?;
    told(&mut tx, now, EventKind::SubscriptionChanged, &kept.subscription).await?;
    self.commit(tx).await?;
    Ok(kept.subscription)
  }

  /// Ends a subscription.
  pub async fn unsubscribe(&self, id: i64) -> Result<(), Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut kept = standing(&mut tx, id).await?;
    end_subscription(&mut tx, &mut kept, now).await?;
    self.commit(tx).await?;
    Ok(())
  }

  /// The standing subscription at `id` as it is kept, its secret with it.
  pub async fn kept_subscription(&self, id: i64) -> Result<Kept, Refusal> {
    let mut tx = self.begin().await?;
    let kept = standing(&mut tx, id).await?;
    tx.commit().await?;
    Ok(kept)
  }

  /// The keys of every active subscription that posts to a url.
  pub async fn webhook_subscriptions(&self) -> Result<Vec<i64>, Refusal> {
    Ok(
      sqlx::query_scalar(
        "SELECT id FROM subscriptions WHERE ended_at IS NULL AND url IS NOT NULL AND state = 'active' ORDER BY id",
      )
      .fetch_all(&self.pool)
      .await?,
    )
  }

  /// The events a subscription matches past `after`, or past where its delivery stands, narrowed further by
  /// `narrower`.
  pub async fn events_for(
    &self,
    kept: &Kept,
    narrower: &EventFilter,
    after: Option<i64>,
    limit: i64,
  ) -> Result<Vec<Event>, Refusal> {
    let mut tx = self.begin().await?;
    let filter = kept.filter();
    let events = read_events(&mut tx, &[&filter, narrower], after.unwrap_or(kept.cursor()), limit).await?;
    tx.commit().await?;
    Ok(events)
  }

  /// Records that the events up to `event_id` were handed over, and that delivery is no longer failing.
  pub async fn delivered(&self, id: i64, event_id: i64) -> Result<(), Refusal> {
    let mut tx = self.begin().await?;
    sqlx::query(
      "UPDATE subscriptions SET delivered_event_id = MAX(COALESCE(delivered_event_id, 0), $1), failure_count = 0 \
       WHERE id = $2",
    )
    .bind(event_id)
    .bind(id)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
  }

  /// Records a failed delivery; the `most`th in a row moves the subscription to failing. Answers whether
  /// delivery goes on.
  pub async fn delivery_failed(&self, id: i64, most: i64) -> Result<Delivery, Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut kept = standing(&mut tx, id).await?;
    if kept.subscription.state != SubscriptionState::Active {
      tx.commit().await?;
      return Ok(Delivery::Stopped);
    }
    kept.subscription.failure_count += 1;
    let failing = kept.subscription.failure_count >= most;
    if failing {
      kept.subscription.state = SubscriptionState::Failing;
      kept.subscription.changed_at = now;
    }
    save(&mut tx, &kept).await?;
    if failing {
      told(&mut tx, now, EventKind::SubscriptionChanged, &kept.subscription).await?;
      self.commit(tx).await?;
      return Ok(Delivery::Stopped);
    }
    self.commit(tx).await?;
    Ok(Delivery::Going)
  }
}
