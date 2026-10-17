//! A work order's verbs (docs/queue.md, "A work order's verbs"): a step added under the next free build
//! name and the final turn handed back to the build, what stands ahead, a stop and a resume, a close of an
//! order that holds its checkout, a reopen, and the refusals a running order answers.

use bmo_job::queue::work_orders::{
  StoppedRun, Tiers, add, back_to_build, close, edit, final_attempt, reopen, resume, skip, steps_ahead,
  stop, task, with_added,
};
use serde_json::{Value, json};

const NOW: i64 = 1_000;

// An order under way at the given phase, its build list in the given states.
fn running(phase: &str, builds: &[&str]) -> Value {
  let mut order = queue_read::work_order::draft(7, "purpose", "instruction");

  order["status"] = json!("active");
  order["phase"] = json!(phase);
  order["tasks"] = Value::Array(
    builds
      .iter()
      .enumerate()
      .map(|(at, state)| {
        let mut one = task(&format!("build-{at}"), "builder", "tier", "build");

        one["state"] = json!(state);
        one
      })
      .collect(),
  );
  order
}

fn final_turn(state: &str, attempt: i64) -> Value {
  let mut control = task("final", "builder", "tier", "verify");

  control["state"] = json!(state);
  control["attempt"] = json!(attempt);
  control
}

fn ids(order: &Value) -> Vec<&str> {
  order["tasks"]
    .as_array()
    .unwrap()
    .iter()
    .map(|task| task["id"].as_str().unwrap())
    .collect()
}

fn step() -> Value {
  task("", "builder", "tier", "build the next piece")
}

fn tiers() -> Tiers {
  Tiers {
    ladder: vec!["Frontier".into(), "Reliable".into(), "Light".into()],
    coworker: vec!["Reliable".into(), "Light".into()],
  }
}

#[test]
fn an_added_step_takes_the_next_free_build_name_where_it_is_put() {
  let mut order = running("implementation", &["done", "pending"]);

  assert_eq!(add(&mut order, step(), None, NOW).unwrap(), "build-2");
  assert_eq!(add(&mut order, step(), Some(("build-1", false)), NOW).unwrap(), "build-3");
  assert_eq!(add(&mut order, step(), Some(("build-1", true)), NOW).unwrap(), "build-4");
  assert_eq!(ids(&order), ["build-0", "build-3", "build-1", "build-4", "build-2"]);
  assert_eq!(order["phase"], "implementation");
  assert_eq!(order["updatedAt"], NOW);
}

#[test]
fn a_step_is_put_only_beside_a_pending_build_step_of_an_order_under_way() {
  let mut order = running("implementation", &["done", "pending"]);

  assert_eq!(add(&mut order, step(), Some(("build-0", false)), NOW).unwrap_err().code, "add_anchor");

  let missing = add(&mut order, step(), Some(("missing", true)), NOW).unwrap_err();

  assert_eq!((missing.status.as_u16(), missing.code), (404, "task_absent"));

  order["status"] = json!("done");
  assert_eq!(add(&mut order, step(), None, NOW).unwrap_err().code, "order_finished");
  assert_eq!(ids(&order), ["build-0", "build-1"]);
}

#[test]
fn a_step_added_at_a_final_turn_that_is_not_running_hands_the_order_back_to_its_build() {
  let mut order = running("finalization", &["done"]);

  order["status"] = json!("paused");
  order["control"] = final_turn("done", 3);

  assert_eq!(add(&mut order, step(), None, NOW).unwrap(), "build-1");
  assert_eq!(order["phase"], "implementation");
  assert!(order["control"].is_null());
  assert_eq!(final_attempt(&order), 3);
  assert_eq!(order["history"][0]["id"], "final-3");
  assert!(steps_ahead(&order));
}

#[test]
fn a_step_added_while_the_final_turn_runs_waits_for_it_to_end() {
  let mut order = running("finalization", &["done"]);

  order["control"] = final_turn("running", 1);
  add(&mut order, step(), None, NOW).unwrap();

  assert_eq!(order["phase"], "finalization");
  assert_eq!(order["control"]["state"], "running");
  assert!(steps_ahead(&order));
}

#[test]
fn a_final_turn_that_ended_nothing_is_let_go_and_the_attempts_counted_are_kept() {
  let mut order = running("finalization", &["done", "pending"]);

  order["finalAttempt"] = json!(4);
  order["control"] = final_turn("pending", 2);
  back_to_build(&mut order);

  assert!(order["history"].as_array().is_none_or(Vec::is_empty));
  assert_eq!(final_attempt(&order), 4);
  assert_eq!(order["phase"], "implementation");
}

#[test]
fn only_a_step_not_yet_passed_over_stands_ahead() {
  assert!(!steps_ahead(&running("finalization", &["done", "skipped"])));
  assert!(steps_ahead(&running("finalization", &["done", "failed"])));
  assert!(steps_ahead(&running("finalization", &["pending"])));
}

#[test]
fn steps_added_before_a_synthesis_stood_follow_its_list_renamed_on_from_it() {
  let order = running("synthesis", &["pending"]);
  let read = vec![task("build-0", "builder", "tier", "a"), task("build-1", "builder", "tier", "b")];
  let tasks = with_added(&order, read);
  let names: Vec<&str> = tasks.iter().map(|task| task["id"].as_str().unwrap()).collect();

  assert_eq!(names, ["build-0", "build-1", "build-2"]);
  assert_eq!(tasks[2]["instruction"], "build");
}

#[test]
fn resuming_a_final_turn_with_steps_ahead_goes_back_to_the_build() {
  let mut order = running("finalization", &["done", "pending"]);

  order["status"] = json!("paused");
  order["control"] = final_turn("done", 2);
  resume(&mut order, NOW).unwrap();

  assert_eq!(order["status"], "active");
  assert_eq!(order["phase"], "implementation");
  assert_eq!(final_attempt(&order), 2);
  assert_eq!(order["tasks"][1]["state"], "pending");

  let mut finished = running("finalization", &["done"]);

  finished["status"] = json!("paused");
  finished["control"] = final_turn("done", 2);
  resume(&mut finished, NOW).unwrap();

  assert_eq!(finished["phase"], "finalization");
  assert_eq!(finished["control"]["state"], "pending");
  assert_eq!(resume(&mut finished, NOW).unwrap_err().code, "resume_not_stopped");
}

#[test]
fn a_reopened_order_counts_its_final_turns_and_holds_afresh() {
  let mut order = running("finalization", &["done"]);

  order["status"] = json!("done");
  order["finalAttempt"] = json!(3);
  order["finalHolds"] = json!(1);
  reopen(&mut order, NOW).unwrap();

  assert!(order.get("finalAttempt").is_none());
  assert!(order.get("finalHolds").is_none());
  assert_eq!(final_attempt(&order), 0);
  assert_eq!(order["status"], "draft");
  assert_eq!(order["history"][0]["id"], "r1-build-0");
  assert_eq!(reopen(&mut order, NOW).unwrap_err().code, "reopen_not_done");
}

#[test]
fn a_stop_cuts_every_run_back_to_waiting_and_hands_back_what_to_end() {
  let mut order = running("implementation", &["done", "running"]);

  order["tasks"][1]["unit"] = json!("bmo-work-7-build-1-1");
  order["tasks"][1]["jobId"] = json!(42);

  let stopped = stop(&mut order, " Enough. ", NOW).unwrap();

  assert_eq!(
    stopped,
    [StoppedRun {
      unit: Some("bmo-work-7-build-1-1".into()),
      job_id: Some(42),
      pool_request: None,
    }]
  );
  assert_eq!(order["status"], "paused");
  assert_eq!(order["tasks"][1]["state"], "pending");
  assert_eq!(order["ownerStop"], json!({ "why": "Enough.", "at": NOW }));
  assert_eq!(stop(&mut order, " ", NOW).unwrap_err().code, "stop_reason_size");
}

#[test]
fn an_order_holding_its_checkout_closes_by_stopping_and_a_running_builder_refuses_the_close() {
  let mut holding = running("implementation", &["done", "pending"]);

  holding["holdsCheckout"] = json!(true);
  close(&mut holding, "Done with it.", NOW).unwrap();

  assert_eq!(holding["status"], "paused");
  assert_eq!(holding["closing"]["why"], "Done with it.");
  assert_eq!(holding["tasks"][1]["state"], "skipped");
  assert_eq!(close(&mut holding, "Again.", NOW).unwrap_err().code, "order_closing");

  let mut building = running("implementation", &["running"]);

  assert_eq!(close(&mut building, "Stop.", NOW).unwrap_err().code, "close_builder_running");
}

#[test]
fn an_edit_takes_a_tier_its_role_may_run_at_and_a_skip_takes_a_reason() {
  let mut order = running("implementation", &["done", "pending"]);
  let mut branch = task("branch-1", "reviewer", "Light", "check it");

  branch["state"] = json!("pending");
  order["stages"] = json!([{ "tasks": [branch] }]);

  assert_eq!(
    edit(&mut order, &tiers(), "build-0", Some("again"), None, NOW).unwrap_err().code,
    "done_task_edit"
  );
  assert_eq!(
    edit(&mut order, &tiers(), "branch-1", None, Some("Frontier"), NOW).unwrap_err().code,
    "coworker_tier_wrong"
  );
  assert_eq!(
    edit(&mut order, &tiers(), "build-1", None, Some("Huge"), NOW).unwrap_err().code,
    "model_tier_wrong"
  );
  edit(&mut order, &tiers(), "build-1", Some(" Build it well. "), Some("Reliable"), NOW).unwrap();

  assert_eq!(order["tasks"][1]["instruction"], "Build it well.");
  assert_eq!(order["tasks"][1]["tier"], "Reliable");
  assert_eq!(skip(&mut order, "build-1", "", NOW).unwrap_err().code, "skip_reason_size");
  assert_eq!(skip(&mut order, "build-1", "Not needed.", NOW).unwrap(), None);
  assert_eq!(order["tasks"][1]["state"], "skipped");
  assert_eq!(skip(&mut order, "build-1", "Again.", NOW).unwrap_err().code, "task_already_skipped");
}
