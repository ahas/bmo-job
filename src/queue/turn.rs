//! Whose turn it is in one worker's line (docs/queue.md, "A worker's turn"): an item the agent may launch,
//! a stopped work order for its runner to resume, or nothing now. The queue's telling and the work-order
//! runner both ask it here, by the one judgement, so the agent is never told a worker is open while a
//! stopped order is about to resume there.

use job_codes::work_lines::PassReason;
use queue_read::lines::BUILTIN_WORKER;
use queue_rows::todos::Item;
use sea_orm::ConnectionTrait;
use serde_json::Value;

use super::launches::queue_for;
use super::workers::Worker;
use crate::{Refusal, all, one};

/// How long a stopped order with nothing behind it waits before it is resumed.
pub const STOPPED_ORDER_WAIT_MS: i64 = 60_000;

/// A stopped order a turn went past, and why.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Passed {
  pub todo_id: i64,
  pub why: PassReason,
}

/// Whose turn it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Turn {
  /// An item the agent may launch.
  Launch { item: i64, passed: Vec<Passed> },
  /// A stopped order its runner resumes.
  Resume { item: i64, passed: Vec<Passed> },
  /// Nothing now.
  Nothing { passed: Vec<Passed> },
}

impl Turn {
  pub fn is_launch(&self) -> bool {
    matches!(self, Turn::Launch { .. })
  }

  pub fn is_resume(&self) -> bool {
    matches!(self, Turn::Resume { .. })
  }

  /// The item it names, where it names one.
  pub fn item(&self) -> Option<i64> {
    match self {
      Turn::Launch { item, .. } | Turn::Resume { item, .. } => Some(*item),
      Turn::Nothing { .. } => None,
    }
  }

  /// The stopped orders it went past.
  pub fn passed(&self) -> &[Passed] {
    match self {
      Turn::Launch { passed, .. } | Turn::Resume { passed, .. } | Turn::Nothing { passed } => passed,
    }
  }
}

/// What judges whether an item has a route to run on now.
pub trait Routable {
  fn routable(&self, item: &Item) -> impl Future<Output = bool> + Send;
}

/// Whether a job of some other item has been launched on a worker since a moment.
async fn launched_since(
  db: &impl ConnectionTrait,
  worker: &str,
  since: i64,
  except: i64,
) -> Result<bool, Refusal> {
  let row = one(
    db,
    r#"SELECT 1 AS "found" FROM "jobs" WHERE COALESCE("worker", $1) = $2 AND "launched_at" >= $3 AND COALESCE("todo_id", 0) != $4 LIMIT 1"#,
    vec![BUILTIN_WORKER.into(), worker.into(), since.into(), except.into()],
  )
  .await?;

  Ok(row.is_some())
}

fn whole(order: &Value, key: &str) -> Option<i64> {
  order.get(key).and_then(Value::as_i64)
}

/// The one judgement of whose turn it is, at `now`. A stopped order stands first unless it is being
/// closed, has no route open, or has only just stopped: while something else waits behind a stop with no
/// other job launched on the worker since, that goes first, once; with nothing behind it the order waits
/// out the minute.
pub async fn worker_queue_turn<C: ConnectionTrait>(
  db: &C,
  items: &[Item],
  worker: &str,
  known: &[Worker],
  routes: &impl Routable,
  now: i64,
) -> Result<Turn, Refusal> {
  let line: Vec<&Item> = queue_for(items, worker, known);

  turn_of(db, &line, worker, routes, now).await
}

async fn turn_of<C: ConnectionTrait>(
  db: &C,
  line: &[&Item],
  worker: &str,
  routes: &impl Routable,
  now: i64,
) -> Result<Turn, Refusal> {
  let mut passed = vec![];

  for (at, item) in line.iter().enumerate() {
    let routable = routes.routable(item).await;
    let order = queue_rows::work_orders::read(db, item.id).await?;
    let paused = order
      .as_ref()
      .filter(|order| order.get("status").and_then(Value::as_str) == Some("paused"));

    let Some(order) = paused else {
      if routable {
        return Ok(Turn::Launch {
          item: item.id,
          passed,
        });
      }

      continue;
    };

    if order.get("closing").is_some() {
      passed.push(Passed {
        todo_id: item.id,
        why: PassReason::Closing,
      });
      continue;
    }

    if !routable {
      passed.push(Passed {
        todo_id: item.id,
        why: PassReason::NoRoute,
      });
      continue;
    }

    if let Some(stopped_at) = whole(order, "stoppedAt")
      && !launched_since(db, worker, stopped_at, item.id).await?
    {
      let behind = Box::pin(turn_of(db, &line[at + 1..], worker, routes, now)).await?;
      let mut ahead = passed.clone();
      let just_stopped = Passed {
        todo_id: item.id,
        why: PassReason::JustStopped,
      };

      match behind {
        Turn::Launch { item, passed: more } => {
          ahead.push(just_stopped);
          ahead.extend(more);

          return Ok(Turn::Launch {
            item,
            passed: ahead,
          });
        }
        Turn::Resume { item, passed: more } => {
          ahead.push(just_stopped);
          ahead.extend(more);

          return Ok(Turn::Resume {
            item,
            passed: ahead,
          });
        }
        Turn::Nothing { passed: more } if now - stopped_at < STOPPED_ORDER_WAIT_MS => {
          ahead.push(Passed {
            todo_id: item.id,
            why: PassReason::WaitingAfterStop,
          });
          ahead.extend(more);

          return Ok(Turn::Nothing { passed: ahead });
        }
        Turn::Nothing { .. } => {}
      }
    }

    return Ok(Turn::Resume {
      item: item.id,
      passed,
    });
  }

  Ok(Turn::Nothing { passed })
}

/// The work orders whose status is one of these, each body as it is stored.
pub async fn orders_in(db: &impl ConnectionTrait, statuses: &[&str]) -> Result<Vec<Value>, Refusal> {
  let rows = all(db, r#"SELECT "body" FROM "work_orders" ORDER BY "todo_id""#, vec![]).await?;
  let mut orders = vec![];

  for row in rows {
    let body: String = row.try_get("", "body")?;
    let Ok(order) = serde_json::from_str::<Value>(&body) else { continue };

    if order
      .get("status")
      .and_then(Value::as_str)
      .is_some_and(|status| statuses.contains(&status))
    {
      orders.push(order);
    }
  }

  Ok(orders)
}

/// The queue as the database holds it, less the items whose work order is under way: an order's item is
/// its runner's while it runs, and one whose order stopped keeps its place in the line.
pub async fn open_items(db: &impl ConnectionTrait) -> Result<Vec<Item>, Refusal> {
  let under_way: Vec<i64> = orders_in(db, &["active"])
    .await?
    .iter()
    .filter_map(|order| whole(order, "todoId"))
    .collect();

  Ok(
    queue_rows::todos::open_items(db)
      .await?
      .into_iter()
      .filter(|item| !under_way.contains(&item.id))
      .collect(),
  )
}
