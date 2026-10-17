//! The work-order runner's pieces that stand without a database: what a step is handed, the session word a
//! step is launched on, the branch places' choice, the checkout hold's readings and commit lines, the pool's
//! keys, a runaway's verdict and the retries, and what the agent is told of a stopped order.

mod support;

use std::collections::{HashMap, HashSet};

use bmo_job::now_ms;
use bmo_job::order_run::context::{result_block, result_excerpt, spoken};
use bmo_job::order_run::hold::{TurnWindow, note, same_checkout, written_in};
use bmo_job::order_run::launch::session_word;
use bmo_job::order_run::places::{Agent, CAP, KEEP_MS, Lifetimes, choose, prunable};
use bmo_job::order_run::pool::{branch_task, reservation_stage, stage_reservation_key};
use bmo_job::order_run::stops::{retry_model, retry_step, runaway_verdict};
use bmo_job::order_run::tick::paused_fact;
use job_codes::WorkOrderStop;
use job_codes::checkout_holds::CheckoutNote;
use job_codes::work_order_stops::{RetryStep, RunawayAction, RunawayCause, StopReason};
use job_policy::Names;
use queue_rows::closing::Reader;
use serde_json::{Value, json};
use support::queue::policy;

#[test]
fn names_are_spoken_as_a_list() {
  assert_eq!(spoken(&["A", "B", "C"], "or"), "A, B or C");
  assert_eq!(spoken(&["A"], "and"), "A");
  assert_eq!(spoken(&[], "and"), "");
}

#[test]
fn a_long_result_is_cut_by_its_round() {
  let cut = result_excerpt(&json!({ "result": "a".repeat(6001) }));

  assert!(cut.ends_with("(cut here; the whole result is in its result document)"));
  assert_eq!(result_excerpt(&json!({ "result": "a".repeat(6000) })), "a".repeat(6000));
  assert_eq!(result_excerpt(&json!({ "result": "short", "round": 1 })), "short");
  assert!(result_excerpt(&json!({ "result": "a".repeat(2001), "round": 1 })).contains("cut here"));
}

#[test]
fn a_block_is_versioned_by_its_text() {
  let block = result_block("#1 plan", Some("/d.md"), "body");

  assert!(block.starts_with("WORK-RESULT {\"key\":\"#1 plan\",\"version\":\"230d8358dc8e8890b4c58deeb62912ee2f20357ae92a5cc861b98e68fe31acb5\",\"document\":\"/d.md\"}\nbody\n"));
  assert!(block.ends_with("\nEND WORK-RESULT"));
}

#[test]
fn a_task_is_launched_on_its_session_word() {
  assert_eq!(session_word(&json!({ "role": "scout" })), "fresh");
  assert_eq!(session_word(&json!({ "role": "builder" })), "resume");
  assert_eq!(
    session_word(&json!({ "role": "builder", "retry": { "freshSession": true } })),
    "handover"
  );
  assert_eq!(session_word(&json!({ "role": "scout", "retry": { "freshSession": true } })), "fresh");
}

fn agent(slot: i64, model: &str, at: i64) -> Agent {
  Agent {
    slot,
    mode: "claude-code".into(),
    model: model.into(),
    role: "scout".into(),
    upstream: String::new(),
    prefix_digest: "prefix".into(),
    last_reply_at: at,
  }
}

// Five minutes for every model of Claude Code the test names.
fn lifetimes() -> Lifetimes {
  HashMap::from([
    (("claude-code".to_owned(), "opus".to_owned(), String::new()), Some(300_000)),
    (("claude-code".to_owned(), "sonnet".to_owned(), String::new()), Some(300_000)),
  ])
}

#[test]
fn a_warm_place_is_reused_oldest_first_then_a_cold_one_then_a_new_number() {
  let now = 10_000_000;
  let at = now - 60_000;
  let agents = [agent(2, "opus", at), agent(1, "opus", at - 10_000), agent(3, "sonnet", at)];
  let busy = HashSet::from([4]);
  let choice = choose(&agents, &[1, 2, 3], &busy, 4, ("claude-code", "opus"), (now, &lifetimes()), |_| {
    "prefix".into()
  });

  assert_eq!(choice.reuse, [1, 2]);
  assert_eq!(choice.cold, [3]);
  assert_eq!(choice.opened, [5]);
  assert!(choice.expired.is_empty());
}

#[test]
fn a_row_whose_prefix_moved_dies_like_one_past_its_life() {
  let now = 10_000_000;
  let agents = [agent(1, "opus", now - 60_000)];
  let choice =
    choose(&agents, &[1], &HashSet::new(), 1, ("claude-code", "opus"), (now, &lifetimes()), |_| {
      "moved".into()
    });

  assert!(choice.reuse.is_empty());
  assert_eq!(choice.cold, [1]);
  assert_eq!(choice.expired, agents);
}

#[test]
fn a_row_of_a_provider_with_no_published_lifetime_is_never_warm() {
  let now = 10_000_000;
  let agents = [agent(1, "opus", now - 1000)];
  let choice = choose(
    &agents,
    &[1],
    &HashSet::new(),
    1,
    ("claude-code", "opus"),
    (now, &Lifetimes::new()),
    |_| "prefix".into(),
  );

  assert!(choice.reuse.is_empty());
  assert_eq!(choice.cold, [1]);
}

#[test]
fn a_place_goes_past_the_cap_or_rested_past_the_keep() {
  let now = 30 * 24 * 60 * 60 * 1000;
  let old = agent(2, "opus", now - KEEP_MS - 1000);
  let slots: Vec<i64> = (1..=10).collect();

  assert_eq!(
    prunable(&[old], &slots, &HashSet::from([9]), now, &lifetimes(), (CAP, KEEP_MS)),
    [2, 10]
  );
}

#[test]
fn two_orders_share_a_checkout_by_its_resolved_path() {
  assert!(same_checkout(&json!({ "cwd": "/srv/a/../b" }), &json!({ "cwd": "/srv/b" })));
  assert!(!same_checkout(&json!({ "cwd": "" }), &json!({ "cwd": "" })));
}

#[test]
fn a_checkout_note_goes_after_what_the_order_already_keeps() {
  let mut order = json!({});

  note(
    &mut order,
    CheckoutNote::Left {
      paths: vec!["a.txt".into()],
    },
  );
  note(&mut order, CheckoutNote::Uncommitted { why: None });

  assert_eq!(
    order["checkoutNotes"],
    json!([{ "kind": "left", "paths": ["a.txt"] }, { "kind": "uncommitted", "why": null }])
  );
}

#[test]
fn every_park_commit_is_english_as_the_close_and_the_landing_read_it() {
  let policy = policy();
  let names = Names {
    owner: "Owner".into(),
    agent: "Agent".into(),
    sender: "Agent".into(),
  };
  let reader = Reader::new(&policy.closing, &names);
  let commits = &policy.hold_commits;
  let marks = [("item", "1682"), ("count", "3"), ("worker", "worker-two")];
  let kept_off = |checked: &str| policy.fill(&commits.kept_off, &[("item", "1682"), ("checked", checked)]);

  for message in [
    policy.fill(&commits.passed, &marks),
    kept_off(&commits.unfinished),
    kept_off(&commits.failed),
    policy.fill(&commits.released, &marks),
    policy.fill(&commits.stopped, &marks),
    policy.fill(&commits.taken, &marks),
  ] {
    assert!(!message.contains('{'), "{message}");
    assert!(reader.written_in_english(&message), "{message}");
  }
}

#[test]
fn a_written_path_is_one_a_turn_was_at_work_for() {
  let dir = tempfile::tempdir().expect("a scratch directory");
  let cwd = dir.path().to_string_lossy().into_owned();

  std::fs::write(dir.path().join("a.txt"), "a").expect("the file is written");

  let now = now_ms();

  assert!(written_in(
    &cwd,
    "a.txt",
    &[TurnWindow {
      from: now - 60_000,
      to: now + 60_000
    }]
  ));
  assert!(!written_in(
    &cwd,
    "a.txt",
    &[TurnWindow {
      from: now + 60_000,
      to: now + 120_000
    }]
  ));
  assert!(written_in(&cwd, "gone.txt", &[TurnWindow { from: 0, to: 1 }]));
  assert!(!written_in(&cwd, "gone.txt", &[]));
}

#[test]
fn a_reservation_key_names_its_stage() {
  let key = stage_reservation_key(12, 3, "codex", "light");

  assert_eq!(key, "work-order:12:stage-3:codex:light");
  assert_eq!(reservation_stage(&key), Some(3));
  assert_eq!(reservation_stage("native:abc"), None);
}

#[test]
fn every_role_but_the_builder_is_a_branch() {
  assert!(branch_task(&json!({ "role": "scout" })));
  assert!(!branch_task(&json!({ "role": "builder" })));
}

#[test]
fn a_run_past_its_starts_is_stopped_in_a_branch_and_told_of_in_the_checkout() {
  let policy = policy();
  let verdict =
    |elapsed: i64, starts: u64, disposable: bool| runaway_verdict(&policy, elapsed, starts, disposable);

  assert_eq!(verdict(0, 13, true).action, Some(RunawayAction::Stopped));
  assert_eq!(verdict(0, 13, false).action, Some(RunawayAction::Told));
  assert_eq!(verdict(0, 13, true).cause, Some(RunawayCause::Spin));
  assert_eq!(verdict(46 * 60_000, 2, true).cause, Some(RunawayCause::Clock));
  assert_eq!(verdict(46 * 60_000, 2, false).action, None);
  assert_eq!(verdict(0, 0, true).action, None);
  assert!(
    verdict(0, 13, true)
      .reason
      .is_some_and(|reason| reason.starts_with(&format!("{}: ", policy.runaway.labels.spin)))
  );
}

#[test]
fn the_retry_grows_with_the_stops_counted() {
  let policy = policy();

  assert_eq!(
    [1, 2, 3, 4].map(retry_step),
    [RetryStep::Again, RetryStep::FreshSession, RetryStep::OtherModel, RetryStep::OtherModel]
  );
  assert_eq!(retry_model(&policy, Some("claude-code::claude-opus-5")), "claude-opus-5");
  assert_eq!(retry_model(&policy, Some("claude-opus-5")), "claude-opus-5");
  assert_eq!(retry_model(&policy, Some("codex::gpt-5.6-sol")), policy.queue.work_retry_model);
  assert_eq!(retry_model(&policy, None), policy.queue.work_retry_model);
}

#[test]
fn a_stop_is_told_by_the_line_its_record_carries() {
  let policy = policy();
  let order = |error: &str| json!({ "todoId": 3, "worker": "worker", "phase": "research", "error": error });
  let reason = |order: &Value| match paused_fact(&policy, order) {
    WorkOrderStop::OrderStopped { reason, .. } => reason,
    other => panic!("an order's stop is told as stopped, not {other:?}"),
  };
  let spin = format!("{}: re-invoked 13 times", policy.runaway.labels.spin);

  assert_eq!(
    reason(&order("x\n  Left running: bun test\n")),
    StopReason::LeftRunning {
      line: "Left running: bun test".into()
    }
  );
  assert_eq!(reason(&order(&spin)), StopReason::Runaway { line: spin.clone() });
  assert_eq!(reason(&order("job-route-exhausted: none")), StopReason::NoRoute);
  assert_eq!(reason(&order("the build broke")), StopReason::StageFailed);
}
