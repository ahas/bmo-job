//! Events: told in the same transaction as the change they tell of, kept for the window, and read past a
//! cursor, narrowed by kind, queue, scope and item (docs/api.md, "Events").

use bmo_job_client::{Event, Page};
use serde::Serialize;
use serde_json::{Map, Value};
use sqlx::{QueryBuilder, Row, Sqlite};

use super::{Store, Tx, from_json, page_limit, paged, to_json};
use crate::refusal::Refusal;

/// How long an event is kept: seven days.
pub const EVENT_WINDOW_MS: i64 = 7 * 24 * 60 * 60 * 1000;

/// What an event is narrowed by: the queue, scope and item its member belongs to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventKeys {
  pub queue_name: Option<String>,
  pub scope: Option<String>,
  pub item_id: Option<i64>,
}

/// What a reading of events is narrowed to: each list empty, or the scope none, matches every event on that
/// count.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EventFilter {
  pub kinds: Vec<String>,
  pub queue_names: Vec<String>,
  pub scope: Option<String>,
  pub item_ids: Vec<i64>,
}

/// Tells an event of `kind` about `data`, within the change's transaction, and answers its number.
pub(crate) async fn emit<T: Serialize>(
  tx: &mut Tx,
  at: i64,
  kind: &str,
  data: &T,
  keys: &EventKeys,
) -> Result<i64, Refusal> {
  let data = serde_json::to_value(data)?;
  debug_assert!(data.is_object(), "an event's data is an object");
  let id: i64 = sqlx::query_scalar(
    "INSERT INTO events (kind, at, data, queue_name, scope, item_id) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
  )
  .bind(kind)
  .bind(at)
  .bind(to_json(&data))
  .bind(&keys.queue_name)
  .bind(&keys.scope)
  .bind(keys.item_id)
  .fetch_one(&mut **tx)
  .await?;
  Ok(id)
}

/// The data of a deleted kind: the member's key alone, with its owner's where it nests under one.
pub(crate) fn key_alone(id: i64, item_id: Option<i64>) -> Value {
  let mut key = Map::new();
  key.insert("id".into(), id.into());
  if let Some(item_id) = item_id {
    key.insert("itemId".into(), item_id.into());
  }
  Value::Object(key)
}

/// The number of the last event told, or 0.
pub(crate) async fn last_event_id(tx: &mut Tx) -> Result<i64, Refusal> {
  let last: Option<i64> = sqlx::query_scalar("SELECT seq FROM sqlite_sequence WHERE name = 'events'")
    .fetch_optional(&mut **tx)
    .await?;
  Ok(last.unwrap_or(0))
}

/// The number of the oldest event the window still holds; a cursor before it has lost events.
async fn window_floor(tx: &mut Tx) -> Result<i64, Refusal> {
  let oldest: Option<i64> = sqlx::query_scalar("SELECT MIN(id) FROM events").fetch_one(&mut **tx).await?;
  match oldest {
    Some(oldest) => Ok(oldest),
    None => Ok(last_event_id(tx).await? + 1),
  }
}

/// Events past `after` matching every one of `filters`, `limit` and one more read.
pub(crate) async fn read_events(
  tx: &mut Tx,
  filters: &[&EventFilter],
  after: i64,
  limit: i64,
) -> Result<Vec<Event>, Refusal> {
  if after < window_floor(tx).await? - 1 {
    return Err(Refusal::gone(
      "The cursor stands before the events the service still keeps; read the collections again.",
    ));
  }
  let mut query: QueryBuilder<Sqlite> =
    QueryBuilder::new("SELECT id, kind, at, data FROM events WHERE id > ");
  query.push_bind(after);
  for filter in filters {
    if !filter.kinds.is_empty() {
      query.push(" AND kind IN (");
      let mut kinds = query.separated(", ");
      for kind in &filter.kinds {
        kinds.push_bind(kind.clone());
      }
      query.push(")");
    }
    if !filter.queue_names.is_empty() {
      query.push(" AND queue_name IN (");
      let mut names = query.separated(", ");
      for name in &filter.queue_names {
        names.push_bind(name.clone());
      }
      query.push(")");
    }
    if let Some(scope) = &filter.scope {
      query.push(" AND scope = ").push_bind(scope.clone());
    }
    if !filter.item_ids.is_empty() {
      query.push(" AND item_id IN (");
      let mut ids = query.separated(", ");
      for id in &filter.item_ids {
        ids.push_bind(*id);
      }
      query.push(")");
    }
  }
  query.push(" ORDER BY id LIMIT ").push_bind(limit + 1);
  let rows = query.build().fetch_all(&mut **tx).await?;
  rows
    .iter()
    .map(|row| {
      Ok(Event {
        id: row.try_get("id")?,
        kind: row.try_get("kind")?,
        at: row.try_get("at")?,
        data: from_json(row.try_get::<&str, _>("data")?)?,
      })
    })
    .collect()
}

/// Whether `kind` is of the shape `<resource>.<change>`: two words of lower-case letters and hyphens.
fn event_kind_shape(kind: &str) -> bool {
  let word = |part: &str| {
    part.starts_with(|c: char| c.is_ascii_lowercase())
      && part.chars().all(|c| c.is_ascii_lowercase() || c == '-')
  };
  matches!(kind.split_once('.'), Some((resource, change)) if word(resource) && word(change))
}

impl Store {
  /// A page of events past `after`.
  pub async fn events(
    &self,
    filter: &EventFilter,
    after: Option<i64>,
    limit: Option<i64>,
  ) -> Result<Page<Event>, Refusal> {
    let limit = page_limit(limit)?;
    let mut tx = self.begin().await?;
    let events = read_events(&mut tx, &[filter], after.unwrap_or(0), limit).await?;
    tx.commit().await?;
    Ok(paged(events, limit))
  }

  /// The cursor that reads from the oldest event the window still holds.
  pub async fn window_start(&self) -> Result<i64, Refusal> {
    let mut tx = self.begin().await?;
    let floor = window_floor(&mut tx).await?;
    tx.commit().await?;
    Ok(floor - 1)
  }

  /// The number of the last event told, or 0.
  pub async fn last_event(&self) -> Result<i64, Refusal> {
    let mut tx = self.begin().await?;
    let last = last_event_id(&mut tx).await?;
    tx.commit().await?;
    Ok(last)
  }

  /// Publishes an event of a process of the service's own, narrowed by its data's queue, scope and item.
  pub async fn publish(&self, kind: &str, data: Map<String, Value>) -> Result<Event, Refusal> {
    if !event_kind_shape(kind) {
      return Err(Refusal::bad_request(format!(
        "The kind {kind:?} is not of the shape <resource>.<change>."
      )));
    }
    let keys = EventKeys {
      queue_name: data.get("queueName").and_then(Value::as_str).map(str::to_owned),
      scope: data.get("scope").and_then(Value::as_str).map(str::to_owned),
      item_id: data.get("itemId").and_then(Value::as_i64),
    };
    let at = self.now();
    let mut tx = self.begin().await?;
    let id = emit(&mut tx, at, kind, &data, &keys).await?;
    self.commit(tx).await?;
    Ok(Event {
      id,
      kind: kind.to_owned(),
      at,
      data,
    })
  }
}
