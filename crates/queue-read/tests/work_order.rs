//! A work order in lines (docs/reading.md, "A work order"). An order's structure is long: an order of nine
//! build tasks and two stages of branches runs past a hundred thousand characters as JSON, because every task
//! carries the paragraph it was told. A caller asking about an order wants where it stands, so the lines are
//! that answer and `--json` is the structure.

use queue_read::work_order::{TASK_HEAD_CHARS, draft, is_order, lines};
use serde_json::{Value, json};

const INTENT: &str = "Why this work is done.";
const INSTRUCTION: &str = "What the work is.";
const OWNER: &str = "Owner";

fn order(over: Value) -> Value {
  let mut order = draft(622, INTENT, INSTRUCTION);

  for (key, value) in [
    ("status", json!("active")),
    ("worker", json!("worker")),
    ("model", json!("claude-opus-5")),
    ("cwd", json!("/repo")),
    // 2026-09-20 11:09 UTC.
    ("updatedAt", json!(1_789_902_540_000_i64)),
  ] {
    order[key] = value;
  }

  for (key, value) in over.as_object().cloned().unwrap_or_default() {
    order[key] = value;
  }

  order
}

fn task(id: &str, role: &str, instruction: &str, title: Option<&str>) -> Value {
  json!({ "id": id, "role": role, "model": "haiku", "instruction": instruction, "title": title,
    "state": "pending", "attempt": 1, "error": null })
}

fn done(mut task: Value) -> Value {
  task["state"] = json!("done");
  task
}

fn read(order: &Value) -> Vec<String> {
  lines(order, OWNER)
}

fn joined(order: &Value) -> String {
  read(order).join("\n")
}

#[test]
fn the_head_says_the_state_the_phase_and_where_in_its_stages_it_stands() {
  let read = read(&order(json!({
    "phase": "research", "stage": 0,
    "stages": [{ "title": "Survey", "tasks": [task("s0", "scout", "look", Some("Measure traffic"))] }, { "title": "Check", "tasks": [] }],
  })));

  assert_eq!(read[0], "#622 active \u{b7} research 1/2");
  assert_eq!(read[1], "worker: worker \u{b7} model: claude-opus-5 \u{b7} cwd: /repo");
}

#[test]
fn a_draft_nobody_has_launched_says_a_dash_where_it_has_nothing() {
  assert_eq!(read(&draft(9, INTENT, INSTRUCTION))[1], "worker: - \u{b7} model: - \u{b7} cwd: -");
}

#[test]
fn the_orders_own_words_stand_whole_because_they_are_what_it_says() {
  let read = read(&order(json!({})));

  assert!(read.contains(&format!("intent: {INTENT}")));
  assert!(read.contains(&format!("instruction: {INSTRUCTION}")));
}

#[test]
fn a_stages_tasks_are_their_state_their_role_and_the_name_the_plan_gave_them() {
  let read = read(&order(json!({
    "stages": [{ "title": "Survey the ground", "tasks": [
      done(task("s0", "scout", "a whole paragraph of instruction", Some("Measure tool traffic"))),
      task("s1", "screener", "another paragraph", Some("Find path defects")),
    ] }],
  })));

  assert!(read.contains(&"1. Survey the ground".to_owned()));
  assert!(read.contains(&"  1. done  scout  Measure tool traffic".to_owned()));
  assert!(read.contains(&"  2. pending  screener  Find path defects".to_owned()));
  // The instruction is the paragraph the task was told, and it is not in a thin answer.
  assert!(!read.join("\n").contains("a whole paragraph of instruction"));
}

#[test]
fn a_task_no_plan_named_is_told_by_the_head_of_its_instruction_cut() {
  let long = "\u{2014}".repeat(TASK_HEAD_CHARS + 20);
  let read = read(&order(json!({ "tasks": [task("b0", "builder", &long, None)] })));

  assert!(read.contains(&format!("  1. pending  builder  {}\u{2026}", "\u{2014}".repeat(TASK_HEAD_CHARS))));
}

#[test]
fn and_the_whitespace_inside_such_a_head_is_laid_flat_so_one_task_stays_one_line() {
  let read = read(&order(json!({ "tasks": [task("b0", "builder", "  first\n\nsecond   third\n", None)] })));

  assert!(read.contains(&"  1. pending  builder  first second third".to_owned()));
}

#[test]
fn the_stage_counter_stands_only_inside_the_stages_never_past_their_end() {
  // An order running its build list has a stage number past the last stage, because the list belongs to no
  // stage, so printing it against the total would read as 3/2.
  let stages = json!([{ "title": "one", "tasks": [] }, { "title": "two", "tasks": [] }]);

  assert_eq!(
    read(&order(json!({ "phase": "implementation", "stage": 2, "stages": stages })))[0],
    "#622 active \u{b7} implementation"
  );
  assert_eq!(
    read(&order(json!({ "phase": "implementation", "stage": 1, "stages": stages })))[0],
    "#622 active \u{b7} implementation 2/2"
  );
}

#[test]
fn the_build_steps_which_belong_to_no_stage_get_a_heading_of_their_own() {
  let build = task("b0", "builder", "build it", Some("Thin the read verbs"));
  let surveyed = done(task("s0", "scout", "look", Some("Measure traffic")));
  let staged = json!({ "title": "Survey", "tasks": [surveyed] });

  assert!(
    read(&order(json!({ "tasks": [build], "stages": [staged] }))).contains(&"build steps:".to_owned())
  );
  assert!(read(&order(json!({ "tasks": [build] }))).contains(&"build steps:".to_owned()));
  // A task the stages already named is not listed twice.
  assert_eq!(
    read(&order(json!({ "tasks": [surveyed], "stages": [staged] })))
      .iter()
      .filter(|line| line.contains("Measure traffic"))
      .count(),
    1
  );
}

#[test]
fn why_a_task_was_skipped_and_why_one_failed_are_part_of_what_the_order_says() {
  let mut skipped = task("s0", "scout", "look", Some("Trace deploy path"));
  let mut failed = task("s1", "verifier", "check", Some("Check parity guard"));

  skipped["state"] = json!("skipped");
  skipped["skip"] = json!({ "why": "an earlier stage answered it", "at": 1 });
  failed["state"] = json!("failed");
  failed["error"] = json!("the session did not answer");
  failed["attempt"] = json!(2);

  let read = read(&order(json!({ "stages": [{ "title": "Survey", "tasks": [skipped, failed] }] })));

  assert_eq!(
    read.last().map(String::as_str),
    Some("updated: 2026-09-20 11:09 \u{b7} the whole structure is --json")
  );
  assert!(
    read
      .join("\n")
      .contains("skipped  scout  Trace deploy path: an earlier stage answered it")
  );
  assert!(
    read
      .join("\n")
      .contains("failed  verifier  Check parity guard (attempt 2): the session did not answer")
  );
}

#[test]
fn the_history_and_the_earlier_rounds_are_a_count_not_their_contents() {
  let read = read(&order(json!({
    "round": 2,
    "history": [task("h0", "builder", "an earlier paragraph", Some("Earlier work")), task("h1", "scout", "another", Some("Earlier look"))],
    "earlier": [{ "round": 1, "intent": INTENT, "instruction": INSTRUCTION }],
  })));

  assert!(read[0].ends_with(" \u{b7} round 2"));
  assert!(read.contains(&"history: 2 tasks".to_owned()));
  assert!(read.contains(&"earlier rounds: 1".to_owned()));
  assert!(!read.join("\n").contains("an earlier paragraph"));
}

#[test]
fn what_decides_whether_an_order_can_be_resumed_is_in_the_thin_answer_not_behind_json() {
  let read = joined(&order(json!({
    "error": "the last turn did not answer",
    "holdsCheckout": true,
    "releaseWait": "unit stop",
    "conflicts": ["server/a.ts", "server/b.ts"],
    // 2026-09-20 03:04 and 03:05 UTC.
    "ownerStop": { "why": "stopped by hand", "at": 1_789_873_440_000_i64 },
    "stoppedAt": 1_789_873_500_000_i64,
    "failedStops": 2,
  })));

  assert!(read.contains("error: the last turn did not answer"));
  assert!(read.contains("checkout: held by this order (waiting on unit stop)"));
  assert!(read.contains("conflicts: server/a.ts, server/b.ts"));
  assert!(read.contains("stopped by Owner: stopped by hand (2026-09-20 03:04)"));
  assert!(read.contains("stopped: 2026-09-20 03:05 \u{b7} 2 stops were failures"));
}

#[test]
fn and_an_order_with_none_of_those_says_none_of_them() {
  let read = joined(&order(json!({})));

  for word in ["error:", "checkout:", "conflicts:", "history:", "parked:", "queue:", "winding down:"] {
    assert!(!read.contains(word), "{word}");
  }
}

// An order handed back across a process boundary is asked here first, so a part of the answer that moved
// falls back to the structure.
#[test]
fn an_order_handed_back_is_read_as_one_and_anything_short_of_one_is_not() {
  let whole = order(json!({}));
  let without = |key: &str, value: Value| {
    let mut changed = whole.clone();

    changed[key] = value;
    changed
  };
  let mut no_intent = whole.clone();

  no_intent.as_object_mut().map(|members| members.remove("intent"));

  assert!(is_order(&whole));
  assert!(!is_order(&Value::Null));
  assert!(!is_order(&json!("there is no such order")));
  assert!(!is_order(&json!({})));
  assert!(!is_order(&without("todoId", json!("622"))));
  assert!(!is_order(&without("stages", Value::Null)));
  assert!(!is_order(&without("error", json!(7))));
  assert!(!is_order(&no_intent));
}
