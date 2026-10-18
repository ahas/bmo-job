//! One step of the runner's pieces asked from outside, for the checks that drive them in `tests/`: a step
//! named by `op` with what it reads, carried on the database, and what it came to as JSON — whether a place
//! was given back, what a whole tick asked of the world past the database, or the refusal in the runner's own
//! words. The pool's own steps are bmo-agency's and are asked of it.

use std::sync::{Mutex, PoisonError};

use serde_json::{Value, json};

use super::hold::Hold;
use super::launch::Spawn;
use super::tick::{Hands, Runner, paused_fact};
use super::{Fault, Ground, faulted, places, pool, stops};
use crate::job_run::plan_check::Roster;
use crate::queue::work_orders::{self as orders, places as order_places, task_at_mut};

fn text<'v>(asked: &'v Value, key: &str) -> Option<&'v str> {
  asked.get(key).and_then(Value::as_str)
}

fn number(asked: &Value, key: &str) -> i64 {
  asked.get(key).and_then(Value::as_i64).unwrap_or_default()
}

/// One step carried with the hold the runner keeps across its ticks: `{ "ok": <what it came to> }`, or
/// `{ "error": <why> }`.
pub async fn carry(ground: &Ground<'_>, hold: &Hold, asked: &Value) -> Value {
  match step(ground, hold, asked).await {
    Ok(came) => json!({ "ok": came }),
    Err(why) => json!({ "error": why }),
  }
}

async fn step(ground: &Ground<'_>, hold: &Hold, asked: &Value) -> Result<Value, Fault> {
  let roster = || Roster::read(ground.units);

  match text(asked, "op").unwrap_or_default() {
    "reserve-next" => {
      let order = read(ground, asked).await?;
      let roster = roster().await.map_err(faulted)?;

      pool::reserve_next_stage(ground, &roster, &order).await.map(|()| Value::Null)
    }
    "settle" => {
      let order = read(ground, asked).await?;
      let roster = roster().await.map_err(faulted)?;

      pool::settle_reservations(ground, &roster, &order).await.map(|()| Value::Null)
    }
    "orphans" => pool::release_orphan_reservations(ground).await.map(|()| Value::Null),
    "turn-places" => pool::release_turn_places(ground).await.map(|()| Value::Null),
    "cancel-pending" => {
      let mut order = read(ground, asked).await?;
      let changed =
        pool::cancel_pending_pools(ground, &mut order, text(asked, "outcome").unwrap_or_default()).await?;

      save(ground, &mut order).await?;
      Ok(json!(changed))
    }
    "finish-pool" => {
      let mut order = read(ground, asked).await?;
      let place = place_of(&order, asked)?;
      let cancel = asked.get("cancel").and_then(Value::as_bool).unwrap_or(false);

      pool::finish_pool(
        ground,
        task_at_mut(&mut order, place),
        text(asked, "outcome").unwrap_or_default(),
        cancel,
      )
      .await?;
      save(ground, &mut order).await?;
      Ok(Value::Null)
    }
    "hold" => {
      let mut came = vec![];

      for one in asked.get("steps").and_then(Value::as_array).into_iter().flatten() {
        came.push(match hold_step(ground, hold, one).await {
          Ok(value) => json!({ "ok": value }),
          Err(why) => json!({ "error": why }),
        });
      }

      Ok(Value::Array(came))
    }
    "watch" => {
      let mut order = read(ground, asked).await?;
      let place = place_of(&order, asked)?;
      let held = order.clone();
      let stopped = Mutex::new(vec![]);

      stops::watch(
        ground,
        &held,
        task_at_mut(&mut order, place),
        asked.get("jobId").and_then(Value::as_i64),
        async |unit: &str| {
          stopped.lock().unwrap_or_else(PoisonError::into_inner).push(unit.to_owned());
        },
      )
      .await?;
      save(ground, &mut order).await?;

      Ok(json!(stopped.into_inner().unwrap_or_default()))
    }
    "count-stop" => {
      let mut order = read(ground, asked).await?;
      let refused = asked.get("refused").and_then(Value::as_bool).unwrap_or(false);

      stops::count_stop(ground, &mut order, refused).await?;
      save(ground, &mut order).await?;

      Ok(Value::Null)
    }
    "resume" => {
      let mut order = read(ground, asked).await?;
      let roster = roster().await.map_err(faulted)?;

      stops::resume_at_turn(ground, &roster, hold, &mut order).await?;

      Ok(Value::Null)
    }
    "take-slot" => {
      let order = read(ground, asked).await?;
      let task = task_of(&order, asked)?;

      let taken = places::take_slot(ground, &order, task).await?;

      Ok(json!({ "slot": taken.slot, "cwd": taken.cwd }))
    }
    "prepare-slot" => {
      let order = read(ground, asked).await?;

      places::prepare_slot(
        ground,
        orders::text(&order, "cwd").unwrap_or_default(),
        text(asked, "cwd").unwrap_or_default(),
      )
      .await
      .map(|()| Value::Null)
    }
    "remember-slot" => {
      let order = read(ground, asked).await?;
      let task = task_of(&order, asked)?;

      places::remember_slot(
        ground,
        &order,
        task,
        (
          text(asked, "mode"),
          text(asked, "model"),
          text(asked, "effort"),
          text(asked, "upstream"),
        ),
      )
      .await
      .map(|()| Value::Null)
    }
    "retire-places" => {
      let order = read(ground, asked).await?;

      places::retire_places(ground, &order).await.map(|()| Value::Null)
    }
    "tick" => {
      let hands = Recorded {
        ground,
        refuse_spawn: asked.get("refuseSpawn").and_then(Value::as_bool).unwrap_or(false),
        seen: Mutex::default(),
      };
      let runner = Runner::new(ground, hold.clone(), hands);

      for _ in 0..number(asked, "ticks").max(1) {
        runner.tick().await?;
      }

      runner.settled().await;

      Ok(Value::Array(runner.hands().seen.lock().unwrap_or_else(PoisonError::into_inner).clone()))
    }
    other => Err(format!("no runner step is called {other}")),
  }
}

// What a tick asked of the world, written down in the order it was asked: each unit started, with the
// arguments its runner was given, each unit stopped, each order told and each worker freed.
struct Recorded<'g> {
  ground: &'g Ground<'g>,
  refuse_spawn: bool,
  seen: Mutex<Vec<Value>>,
}

impl Recorded<'_> {
  fn saw(&self, what: Value) {
    self.seen.lock().unwrap_or_else(PoisonError::into_inner).push(what);
  }
}

impl Spawn for Recorded<'_> {
  async fn spawn(&self, args: &[String]) -> Result<(), String> {
    let runner: Vec<&String> = args.iter().skip_while(|arg| *arg != "--").skip(1).collect();

    self.saw(json!({ "launch": runner }));

    if self.refuse_spawn { Err("the unit was refused".to_owned()) } else { Ok(()) }
  }
}

impl Hands for Recorded<'_> {
  async fn stop(&self, unit: &str) {
    self.saw(json!({ "stop": unit }));
  }

  async fn notify(&self, order: &Value) {
    if order.get("status").and_then(Value::as_str) == Some("paused") {
      self.ground.tell(paused_fact(self.ground.policy, order)).await;
    }

    self.saw(json!({ "notify": order.get("todoId"), "status": order.get("status") }));
  }

  async fn freed(&self) {
    self.saw(json!("freed"));
  }

  async fn changed(&self, _todo_id: i64, _order: &Value) {}
}

async fn hold_step(ground: &Ground<'_>, hold: &Hold, asked: &Value) -> Result<Value, Fault> {
  if text(asked, "do") == Some("settle") {
    hold.settled().await;
    return Ok(Value::Null);
  }

  let mut order = read(ground, asked).await?;

  match text(asked, "do").unwrap_or_default() {
    "take" => Ok(json!(hold.take_checkout(ground, &mut order).await?)),
    "release" => Ok(json!(hold.release(ground, &mut order).await?)),
    "busy" => Ok(json!(hold.checkout_busy(ground, &order).await?)),
    other => Err(format!("no hold step is called {other}")),
  }
}

fn place_of(order: &Value, asked: &Value) -> Result<orders::Place, Fault> {
  let id = text(asked, "task").unwrap_or_default();

  order_places(order)
    .into_iter()
    .find(|(_, task)| text(task, "id") == Some(id))
    .map(|(place, _)| place)
    .ok_or_else(|| "no such task".to_owned())
}

fn task_of<'o>(order: &'o Value, asked: &Value) -> Result<&'o Value, Fault> {
  let id = text(asked, "task").unwrap_or_default();

  order_places(order)
    .into_iter()
    .find(|(_, task)| text(task, "id") == Some(id))
    .map(|(_, task)| task)
    .ok_or_else(|| "no such task".to_owned())
}

async fn save(ground: &Ground<'_>, order: &mut Value) -> Result<(), Fault> {
  queue_rows::work_orders::save(ground.db, order).await.map_err(faulted)
}

async fn read(ground: &Ground<'_>, asked: &Value) -> Result<Value, Fault> {
  queue_rows::work_orders::read(ground.db, number(asked, "todoId"))
    .await
    .map_err(faulted)?
    .ok_or_else(|| "no such order".to_owned())
}
