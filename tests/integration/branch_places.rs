//! A work order's branch places (docs/queue.md, "Branch places"), asked of bmo-agency through the runner's
//! own steps: a take names the order's checkout, the task's selection, role and tier and every place a task
//! sits in now, and is refused where bmo-agency names no directory; a finished branch's place is remembered
//! with what it ran on, a builder turn's never; and an order no longer active hands back the places it used.

use bmo_job::queue::work_orders as orders;
use job_agency::{HeldPlace, PlaceRecord, PlacesRetirement};
use serde_json::{Value, json};

use crate::support::queue::OWNER;
use crate::support::runner::Bench;

const CHECKOUT: &str = "/work/repo";

// A task of an order standing in a place, in the state given, of the job given.
fn placed(id: &str, role: &str, state: &str, slot: i64, job: Option<i64>) -> Value {
  let mut task = orders::task(id, role, "codex::model-1", "do it");

  task["state"] = json!(state);
  task["slot"] = json!(slot);
  task["jobId"] = json!(job);
  task
}

async fn order(bench: &Bench, todo: i64, status: &str, cwd: &str, tasks: Vec<Value>) {
  bench
    .write_order(json!({
      "todoId": todo, "worker": "worker", "intent": "build", "instruction": "build", "status": status,
      "phase": "build", "model": "codex::model-1", "cwd": cwd, "stages": [{ "tasks": tasks }],
      "tasks": [], "history": [], "holdsCheckout": false, "chatId": OWNER,
    }))
    .await;
}

// Item 1's order in the checkout: a branch running in place 2, one whose job is still open in place 4, one
// whose job finished in place 5, a waiting branch with no place and a builder turn done in place 6; item 2's
// paused order elsewhere still running in place 1; item 3's done order, which worked in places 7, 3 and 7
// again and holds nothing.
async fn orders_standing(bench: &Bench) {
  let open = bench.scene.job(Some(1), "worker", "bmo-work-1-s2-1", 1).await;
  let closed = bench.scene.job(Some(1), "worker", "bmo-work-1-s3-1", 1).await;

  bench
    .scene
    .run(r#"UPDATE "jobs" SET "finished_at" = 2 WHERE "id" = $1"#, vec![closed.into()])
    .await;

  let mut waiting = orders::task("s4", "scout", "codex::model-1", "look");

  waiting["tier"] = json!("Light");
  order(
    bench,
    1,
    "active",
    CHECKOUT,
    vec![
      placed("s1", "scout", "running", 2, None),
      placed("s2", "scout", "done", 4, Some(open)),
      placed("s3", "scout", "done", 5, Some(closed)),
      waiting,
      placed("b1", "builder", "done", 6, None),
    ],
  )
  .await;
  order(bench, 2, "paused", "/work/other", vec![placed("s1", "scout", "running", 1, None)]).await;
  order(
    bench,
    3,
    "done",
    CHECKOUT,
    vec![
      placed("s1", "scout", "done", 7, None),
      placed("s2", "scout", "done", 3, None),
      placed("s3", "scout", "failed", 7, None),
    ],
  )
  .await;
}

fn sorted(mut held: Vec<HeldPlace>) -> Vec<(String, i64)> {
  let mut pairs: Vec<(String, i64)> = held.drain(..).map(|one| (one.cwd, one.slot)).collect();

  pairs.sort();
  pairs
}

#[tokio::test]
async fn a_branch_takes_its_place_with_the_places_held_and_is_refused_where_no_directory_is_named() {
  let mut bench = Bench::new().await;

  orders_standing(&bench).await;

  let taken = bench.step(json!({ "op": "take-slot", "todoId": 1, "task": "s4" })).await;
  let dir = bench.scene.units.place.cwd.clone().unwrap();

  assert_eq!(taken, json!({ "slot": 1, "cwd": dir.to_string_lossy() }));

  let asks = bench.scene.units.place_asks.lock().unwrap().clone();

  assert_eq!(asks.len(), 1);
  assert_eq!(
    (
      asks[0].cwd.as_str(),
      asks[0].selection.as_str(),
      asks[0].role.as_str(),
      asks[0].tier.as_deref()
    ),
    (CHECKOUT, "codex::model-1", "scout", Some("Light"))
  );
  assert_eq!(
    sorted(asks[0].held.clone()),
    [("/work/other".to_owned(), 1), (CHECKOUT.to_owned(), 2), (CHECKOUT.to_owned(), 4)]
  );

  bench.scene.units.place.cwd = None;

  assert_eq!(
    bench.carried(json!({ "op": "take-slot", "todoId": 1, "task": "s4" })).await,
    Err(json!("bmo-agency named no directory for branch place 1"))
  );
}

#[tokio::test]
async fn a_finished_branch_s_place_is_remembered_with_what_it_ran_on_and_a_builder_turn_leaves_nothing() {
  let bench = Bench::new().await;

  orders_standing(&bench).await;
  bench
    .step(json!({
      "op": "remember-slot", "todoId": 1, "task": "s2", "mode": "codex", "model": "model-1", "upstream": "up-1",
    }))
    .await;
  bench
    .step(json!({ "op": "remember-slot", "todoId": 1, "task": "b1", "mode": "codex", "model": "model-1" }))
    .await;

  assert_eq!(
    *bench.scene.units.remembered.lock().unwrap(),
    [PlaceRecord {
      cwd: CHECKOUT.into(),
      slot: 4,
      role: "scout".into(),
      mode: Some("codex".into()),
      model: Some("model-1".into()),
      effort: None,
      upstream: Some("up-1".into()),
    }]
  );
}

#[tokio::test]
async fn an_order_no_longer_active_hands_back_the_places_it_used_with_those_still_held() {
  let bench = Bench::new().await;

  orders_standing(&bench).await;
  bench.step(json!({ "op": "retire-places", "todoId": 1 })).await;

  assert!(bench.scene.units.retired.lock().unwrap().is_empty());

  bench.step(json!({ "op": "retire-places", "todoId": 3 })).await;

  let retired: Vec<PlacesRetirement> = bench.scene.units.retired.lock().unwrap().clone();

  assert_eq!(retired.len(), 1);
  assert_eq!((retired[0].cwd.as_str(), retired[0].used.as_slice()), (CHECKOUT, [3, 7].as_slice()));
  assert_eq!(
    sorted(retired[0].held.clone()),
    [("/work/other".to_owned(), 1), (CHECKOUT.to_owned(), 2), (CHECKOUT.to_owned(), 4),]
  );
}
