//! The branch places a work order's branches run in (docs/queue.md, "Branch places"). bmo-agency keeps them
//! as its standing checkouts and chooses among them; the service says which places its tasks sit in now,
//! keeps the number of the place a task took on the task, and asks bmo-agency to take a place, lay the
//! checkout into it, remember what a finished branch left there and retire what an ended order left
//! standing.

use std::collections::HashSet;

use job_agency::{HeldPlace, PlaceAsk, PlaceRecord, PlacesRetirement};
use serde_json::Value;

use super::pool::branch_task;
use super::{Fault, Ground, faulted};
use crate::all;
use crate::queue::turn::orders_in;
use crate::queue::work_orders::{self as orders, places};

/// A place taken for a branch: its number, and the directory the branch works in.
pub struct Taken {
  pub slot: i64,
  pub cwd: String,
}

/// The places a task is sitting in, read off the job rows rather than a flag of their own, so a restart frees
/// nothing by hand: a place is taken exactly as long as the run holding it has not finished. The order in
/// hand counts with the stored ones, since a stage's launches hand out places one task after another; each
/// is named with its order's checkout, so bmo-agency counts every pool's within its own.
pub async fn held_places(ground: &Ground<'_>, order: Option<&Value>) -> Result<Vec<HeldPlace>, Fault> {
  let unfinished: HashSet<i64> =
    all(ground.db, r#"SELECT "id" FROM "jobs" WHERE "finished_at" IS NULL"#, vec![])
      .await
      .map_err(faulted)?
      .iter()
      .filter_map(|row| row.try_get::<i64>("", "id").ok())
      .collect();
  let todo = order.and_then(|order| orders::number(order, "todoId"));
  let mut stored: Vec<Value> = orders_in(ground.db, &["active", "paused"])
    .await
    .map_err(faulted)?
    .into_iter()
    .filter(|one| todo.is_none() || orders::number(one, "todoId") != todo)
    .collect();

  stored.extend(order.cloned());

  let mut held = vec![];

  for one in &stored {
    let cwd = orders::text(one, "cwd").unwrap_or_default();

    if cwd.is_empty() {
      continue;
    }

    for (_, task) in places(one) {
      let Some(slot) = task.get("slot").and_then(Value::as_i64) else { continue };
      let running = orders::text(task, "state") == Some("running");

      if running || task.get("jobId").and_then(Value::as_i64).is_some_and(|job| unfinished.contains(&job)) {
        held.push(HeldPlace {
          cwd: cwd.to_owned(),
          slot,
        });
      }
    }
  }

  Ok(held)
}

/// Which place a branch task works in, as bmo-agency chooses it for the order's checkout, the task's model
/// selection, role and tier, against the places taken now: its number and its directory.
pub async fn take_slot(ground: &Ground<'_>, order: &Value, task: &Value) -> Result<Taken, Fault> {
  let ask = PlaceAsk {
    cwd: orders::text(order, "cwd").unwrap_or_default().to_owned(),
    selection: orders::text(task, "model").unwrap_or_default().to_owned(),
    role: orders::text(task, "role").unwrap_or_default().to_owned(),
    tier: orders::text(task, "tier").map(str::to_owned),
    held: held_places(ground, Some(order)).await?,
  };
  let taken = ground.units.take_place(&ask).await.map_err(faulted)?;

  Ok(Taken {
    slot: taken.slot,
    cwd: taken.cwd.to_string_lossy().into_owned(),
  })
}

/// A place brought back to the shape a task starts in, as bmo-agency lays it: rewound to the checkout's
/// commit with the checkout's open change and new files laid on top.
pub async fn prepare_slot(ground: &Ground<'_>, source: &str, cwd: &str) -> Result<(), Fault> {
  ground.units.prepare_place(source, cwd).await.map_err(faulted)
}

/// What a finished branch leaves in bmo-agency's list: the place it worked in, its role, and the provider,
/// model, effort and upstream that answered there. A builder turn holds no place and leaves nothing.
pub async fn remember_slot(
  ground: &Ground<'_>,
  order: &Value,
  task: &Value,
  (mode, model, effort, upstream): (Option<&str>, Option<&str>, Option<&str>, Option<&str>),
) -> Result<(), Fault> {
  let Some(slot) = task.get("slot").and_then(Value::as_i64) else { return Ok(()) };

  if !branch_task(task) {
    return Ok(());
  }

  let record = PlaceRecord {
    cwd: orders::text(order, "cwd").unwrap_or_default().to_owned(),
    slot,
    role: orders::text(task, "role").unwrap_or_default().to_owned(),
    mode: mode.map(str::to_owned),
    model: model.map(str::to_owned),
    effort: effort.map(str::to_owned),
    upstream: upstream.map(str::to_owned),
  };

  ground.units.remember_place(&record).await.map_err(faulted)
}

/// The places an order worked in, by their numbers, each once.
pub fn used_slots(order: &Value) -> Vec<i64> {
  let mut used: Vec<i64> = places(order)
    .iter()
    .filter_map(|(_, task)| task.get("slot").and_then(Value::as_i64))
    .collect();

  used.sort_unstable();
  used.dedup();
  used
}

/// What stands in bmo-agency for the places a done or stopped order worked in taken down, those a task still
/// sits in kept; a retirement bmo-agency refuses is left for the next time, since a stage must never fail over
/// tidying.
pub async fn retire_places(ground: &Ground<'_>, order: &Value) -> Result<(), Fault> {
  if orders::text(order, "status") == Some("active") {
    return Ok(());
  }

  let cwd = orders::text(order, "cwd").unwrap_or_default();
  let used = used_slots(order);

  if cwd.is_empty() || used.is_empty() {
    return Ok(());
  }

  let retirement = PlacesRetirement {
    cwd: cwd.to_owned(),
    used,
    held: held_places(ground, None).await?,
  };

  if let Err(failed) = ground.units.retire_places(&retirement).await {
    eprintln!("bmo-job: places of {cwd}: {}", failed.error);
  }

  Ok(())
}
