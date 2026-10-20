//! Queues: paused or open, under holds or not, and how many items wait in each (docs/api.md, "Queues").

use bmo_job_client::{EventKind, Hold, Page, Queue, QueueChange, QueueQuery, QueueState};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::events::{EventKeys, emit};
use super::{Store, Tx, filled, from_json, page_limit, paged, stored, to_json};
use crate::refusal::Refusal;

const QUEUE_COLUMNS: &str = "name, state, holds, changed_at, \
  (SELECT COUNT(*) FROM items WHERE items.queue_name = queues.name AND items.state = 'open') AS open_count, \
  (SELECT COUNT(*) FROM items WHERE items.queue_name = queues.name AND items.state = 'leased') AS leased_count";

fn queue_from(row: &SqliteRow) -> Result<Queue, Refusal> {
  Ok(Queue {
    name: row.try_get("name")?,
    state: stored(row.try_get("state")?, QueueState::parse)?,
    holds: from_json(row.try_get::<&str, _>("holds")?)?,
    open_count: row.try_get("open_count")?,
    leased_count: row.try_get("leased_count")?,
    changed_at: row.try_get("changed_at")?,
  })
}

/// Makes the queue stand where it does not yet.
pub(crate) async fn ensure_queue(tx: &mut Tx, name: &str, now: i64) -> Result<(), Refusal> {
  sqlx::query("INSERT INTO queues (name, state, holds, changed_at) VALUES ($1, 'open', '[]', $2) ON CONFLICT DO NOTHING")
    .bind(name)
    .bind(now)
    .execute(&mut **tx)
    .await?;
  Ok(())
}

/// The queue named, or none.
pub(crate) async fn find_queue(tx: &mut Tx, name: &str) -> Result<Option<Queue>, Refusal> {
  let row = sqlx::query(&format!("SELECT {QUEUE_COLUMNS} FROM queues WHERE name = $1"))
    .bind(name)
    .fetch_optional(&mut **tx)
    .await?;
  row.as_ref().map(queue_from).transpose()
}

async fn queue_at(tx: &mut Tx, name: &str) -> Result<Queue, Refusal> {
  find_queue(tx, name)
    .await?
    .ok_or_else(|| Refusal::not_found(format!("There is no queue {name:?}.")))
}

/// Whether a hold stands at `now`: it has no end, or its end has not come.
pub(crate) fn hold_stands(hold: &Hold, now: i64) -> bool {
  hold.until.is_none_or(|until| until > now)
}

/// Whether the queue gives leases at `now`: open, and under no standing hold.
pub(crate) fn gives_leases(queue: &Queue, now: i64) -> bool {
  queue.state == QueueState::Open && !queue.holds.iter().any(|hold| hold_stands(hold, now))
}

/// Tells that the queue changed.
pub(crate) async fn told_queue(tx: &mut Tx, at: i64, queue: &Queue) -> Result<(), Refusal> {
  let keys = EventKeys {
    queue_name: Some(queue.name.clone()),
    ..EventKeys::default()
  };
  emit(tx, at, EventKind::QueueChanged.as_str(), queue, &keys).await?;
  Ok(())
}

impl Store {
  /// Every queue, by name.
  pub async fn queues(&self, query: &QueueQuery) -> Result<Page<Queue>, Refusal> {
    let limit = page_limit(query.limit)?;
    let mut tx = self.begin().await?;
    let rows =
      sqlx::query(&format!("SELECT {QUEUE_COLUMNS} FROM queues WHERE name > $1 ORDER BY name LIMIT $2"))
        .bind(query.after.clone().unwrap_or_default())
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let queues = rows.iter().map(queue_from).collect::<Result<Vec<_>, _>>()?;
    Ok(paged(queues, limit))
  }

  pub async fn queue(&self, name: &str) -> Result<Queue, Refusal> {
    let mut tx = self.begin().await?;
    let queue = queue_at(&mut tx, name).await?;
    tx.commit().await?;
    Ok(queue)
  }

  /// Pauses or opens a queue, and replaces its holds whole.
  pub async fn change_queue(&self, name: &str, change: QueueChange) -> Result<Queue, Refusal> {
    if let Some(holds) = &change.holds {
      for hold in holds {
        filled("hold's reason", &hold.reason)?;
        filled("hold's actorName", &hold.actor_name)?;
      }
    }
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut queue = queue_at(&mut tx, name).await?;
    if let Some(state) = change.state {
      queue.state = state;
    }
    if let Some(holds) = change.holds {
      queue.holds = holds;
    }
    queue.changed_at = now;
    sqlx::query("UPDATE queues SET state = $1, holds = $2, changed_at = $3 WHERE name = $4")
      .bind(queue.state.as_str())
      .bind(to_json(&queue.holds))
      .bind(now)
      .bind(&queue.name)
      .execute(&mut *tx)
      .await?;
    told_queue(&mut tx, now, &queue).await?;
    self.commit(tx).await?;
    Ok(queue)
  }
}
