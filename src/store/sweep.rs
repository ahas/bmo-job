//! What happens at a time: leases and subscriptions run out, questions come due, holds end, and old events
//! leave the window. Each is told as an event; nothing is done for anyone.

use bmo_job_client::{Hold, Queue};
use sqlx::Row;

use super::events::EVENT_WINDOW_MS;
use super::leases::expire_leases;
use super::questions::tell_due_questions;
use super::queues::{find_queue, hold_stands, told_queue};
use super::subscriptions::expire_subscriptions;
use super::{Store, Tx, from_json, to_json};
use crate::refusal::Refusal;

/// Drops every hold whose end has come by `now`, telling each queue it changed.
async fn end_holds(tx: &mut Tx, now: i64) -> Result<usize, Refusal> {
  let rows = sqlx::query("SELECT name, holds FROM queues WHERE holds != '[]'")
    .fetch_all(&mut **tx)
    .await?;
  let mut ended = 0;
  for row in &rows {
    let name: String = row.try_get("name")?;
    let holds: Vec<Hold> = from_json(row.try_get::<&str, _>("holds")?)?;
    let standing: Vec<Hold> = holds.iter().filter(|hold| hold_stands(hold, now)).cloned().collect();
    if standing.len() == holds.len() {
      continue;
    }
    sqlx::query("UPDATE queues SET holds = $1, changed_at = $2 WHERE name = $3")
      .bind(to_json(&standing))
      .bind(now)
      .bind(&name)
      .execute(&mut **tx)
      .await?;
    let queue: Queue = find_queue(tx, &name).await?.expect("the queue just read stands");
    told_queue(tx, now, &queue).await?;
    ended += 1;
  }
  Ok(ended)
}

impl Store {
  /// Tells everything whose time has come by now, in one change.
  pub async fn sweep(&self) -> Result<(), Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let told = expire_leases(&mut tx, now).await?
      + tell_due_questions(&mut tx, now, self.question_wait_ms).await?
      + end_holds(&mut tx, now).await?
      + expire_subscriptions(&mut tx, now).await?;
    sqlx::query("DELETE FROM events WHERE at < $1")
      .bind(now - EVENT_WINDOW_MS)
      .execute(&mut *tx)
      .await?;
    if told > 0 { self.commit(tx).await } else { Ok(tx.commit().await?) }
  }
}
