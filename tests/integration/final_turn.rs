//! A work order's final turn that ended done (docs/queue.md, "The work-order runner"), carried through the
//! runner's own tick: one that held its item — a run that wound down after one step of several — stops the
//! order uncounted, telling no stop of its own, and runs again once resumed; one that neither closed nor held
//! its item is refused and counted, and the agent is told the order stopped.

use bmo_job::queue::work_orders as orders;
use job_codes::work_order_stops::StopReason;
use job_codes::{Fact, WorkOrderStop};
use job_receiver::Said;
use queue_rows::todos;
use serde_json::{Value, json};

use crate::support::queue::{NOW, OWNER};
use crate::support::runner::Bench;

// An item whose order stands at its final turn, that turn's job finished done with its report.
async fn final_turn_ended(bench: &Bench) -> i64 {
  let todo = 4;

  bench.scene.item(todo, "Move the tree").await;

  let unit = format!("bmo-work-{todo}-final-1");
  let job = bench.scene.job(Some(todo), "worker", &unit, 1).await;

  bench
    .scene
    .run(
      r#"UPDATE "jobs" SET "finished_at" = 2, "outcome" = 'done', "ok" = 1,
           "report" = 'Step 1 of 7 done; held until its commit is live.' WHERE "id" = $1"#,
      vec![job.into()],
    )
    .await;

  let mut control = orders::task("final", "builder", "claude-code::claude-opus-5-5", "finish");

  control["state"] = json!("running");
  control["attempt"] = json!(1);
  control["unit"] = json!(unit);
  control["jobId"] = json!(job);
  control["startedAt"] = json!(1);

  bench
    .write_order(json!({
      "todoId": todo, "worker": "worker", "intent": "move", "instruction": "move", "status": "active",
      "phase": "finalization", "model": "claude-code::claude-opus-5-5", "stages": [], "tasks": [],
      "history": [], "control": control, "holdsCheckout": false, "chatId": OWNER,
    }))
    .await;

  todo
}

// The facts the agent was told of the item's order as its own stop.
fn stops_told(bench: &Bench, todo: i64) -> usize {
  bench
    .scene
    .units
    .instructions()
    .iter()
    .filter(|told| {
      matches!(&told.said, Said::Fact(Fact::WorkOrder(WorkOrderStop::OrderStopped { item, .. })) if *item == todo)
    })
    .count()
}

async fn tick(bench: &Bench) -> Value {
  bench.step(json!({ "op": "tick" })).await
}

#[tokio::test]
async fn a_final_turn_that_held_its_item_pauses_the_order_uncounted_and_runs_again_on_resume() {
  let bench = Bench::new().await;
  let todo = final_turn_ended(&bench).await;

  todos::hold_as(&bench.scene.db, todo, Some("waiting for its commit to go live"), Some("pause"), NOW)
    .await
    .unwrap();

  let seen = tick(&bench).await;
  let mut order = bench.order(todo).await;

  assert_eq!(order["status"], "paused");
  assert_eq!(order["control"]["state"], "done");
  assert!(order["error"].is_null(), "{}", order["error"]);
  assert!(order.get("failedStops").is_none());
  assert_eq!(stops_told(&bench, todo), 0);
  assert!(seen.as_array().unwrap().contains(&json!("freed")), "{seen}");

  orders::resume(&mut order, NOW).unwrap();

  assert_eq!(order["status"], "active");
  assert_eq!(order["control"]["state"], "pending");
  assert_eq!(order["control"]["attempt"], 1);
  assert!(order["control"]["jobId"].is_null());
}

#[tokio::test]
async fn a_final_turn_that_neither_closed_nor_held_its_item_is_refused_and_counted() {
  let bench = Bench::new().await;
  let todo = final_turn_ended(&bench).await;

  tick(&bench).await;

  let order = bench.order(todo).await;

  assert_eq!(order["status"], "paused");
  assert_eq!(order["control"]["state"], "failed");
  assert_eq!(order["error"], "The final turn ended without closing or holding its item.");
  assert_eq!(order["failedStops"], 1);
  assert_eq!(stops_told(&bench, todo), 1);

  let told = bench.scene.units.instructions();
  let stopped = told
    .iter()
    .find_map(|told| match &told.said {
      Said::Fact(Fact::WorkOrder(WorkOrderStop::OrderStopped { reason, .. })) => Some(reason.clone()),
      _ => None,
    })
    .unwrap();

  assert_eq!(stopped, StopReason::StageFailed);
}
