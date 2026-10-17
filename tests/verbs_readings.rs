//! The verbs' readings of their own words and of what a job left (docs/verbs.md): a shell call read by what it
//! ran, a job's tool calls timed, a workflow's branches divided, a work order's time merged, the toolbook's
//! proxy calls named, a work order's words read into its request, a wait's end read, a keep's length read,
//! a hold's refusals, a question's wording judged, and a job's own process told by its markers.

use std::collections::BTreeMap;
use std::path::Path;

use bmo_job::verbs::ask::{AskRequest, ask_refusal, wording_refusal, words_refusal};
use bmo_job::verbs::branch_split::{branch_split, grouped, project_dir};
use bmo_job::verbs::cowork_timing::{Run, run_seconds};
use bmo_job::verbs::job_figures::{ShellClass, shell_class};
use bmo_job::verbs::node_land::setup_inputs_moved;
use bmo_job::verbs::overtime::duration_seconds;
use bmo_job::verbs::stand::{JobProcess, span};
use bmo_job::verbs::todo::holds::{Condition, order_hold_refusal, stop_here_refusal, wind_down_reason};
use bmo_job::verbs::tool_calls::{calls_of, tool_calls};
use bmo_job::verbs::toolbook::reached;
use bmo_job::verbs::waiting::{WaitRequest, WaitWords, WordsFault, read_request, wait_end};
use bmo_job::verbs::work_order::{capped, item_of, request_from_args};
use job_agency::{JobLines, ToolCall, ToolResult};
use job_codes::job_wait::WaitTarget;
use job_codes::todo_verbs::OrderHold;
use job_codes::{JobAskRefusal, JobWaitRefusal, TodoRefusal};
use queue_rows::store::Row;
use queue_rows::wording::BannedTerm;
use serde_json::json;

#[test]
fn a_call_is_read_by_the_program_and_its_first_word() {
  assert_eq!(shell_class("bun test --parallel a.test.ts b.test.ts"), ShellClass::BunTest);
  assert_eq!(shell_class("bun scripts/quick-check.ts"), ShellClass::QuickCheck);
  assert_eq!(shell_class("bun scripts/check.ts --since main"), ShellClass::Check);
  assert_eq!(shell_class("bmo check quick"), ShellClass::QuickCheck);
  assert_eq!(shell_class("bmo test log /tmp/x.log --failures"), ShellClass::TestLog);
  assert_eq!(shell_class("bmo edit a.ts --replace x y"), ShellClass::Other);
  assert_eq!(shell_class("cargo clippy -p a-crate"), ShellClass::Cargo);
  assert_eq!(shell_class("git status --short"), ShellClass::Git);
  assert_eq!(shell_class("rg -n foo crates"), ShellClass::Reading);
  assert_eq!(shell_class("python3 x.py"), ShellClass::Other);
}

#[test]
fn a_compound_line_is_read_by_its_heaviest_command_past_its_setting_up() {
  assert_eq!(
    shell_class("cd /repo && bunx tsc --noEmit > /tmp/t.log 2>&1; bmo test log /tmp/t.log"),
    ShellClass::TypeCheck
  );
  assert_eq!(
    shell_class("cd /repo && export L=/tmp/x && echo start && git diff --stat"),
    ShellClass::Git
  );
  assert_eq!(shell_class("cat a | wc -l"), ShellClass::Reading);
  assert_eq!(shell_class("sed -n 1,30p notes.md && cargo test -p a-crate"), ShellClass::Cargo);
  assert_eq!(shell_class("echo hi"), ShellClass::Other);
}

#[test]
fn an_environment_runner_queue_or_shell_in_front_is_looked_past() {
  assert_eq!(shell_class("XDG_RUNTIME_DIR=/run/user/7 X=1 cargo test -p a-crate"), ShellClass::Cargo);
  assert_eq!(shell_class("timeout 590 bun test --parallel a.test.ts"), ShellClass::BunTest);
  assert_eq!(shell_class("bmo testbed run -- bun test a.test.ts"), ShellClass::BunTest);
  assert_eq!(shell_class("zsh -c 'cd /repo && git log --oneline -3'"), ShellClass::Git);
}

fn call(id: i64, name: &str, at: i64, reference: &str, command: Option<&str>) -> ToolCall {
  ToolCall {
    id,
    agent: "worker".into(),
    run: Some(1),
    reference: Some(reference.into()),
    name: name.into(),
    source: None,
    at,
    command: command.map(str::to_owned),
    args: None,
  }
}

fn result(id: i64, at: i64, reference: &str) -> ToolResult {
  ToolResult {
    id,
    agent: "worker".into(),
    run: Some(1),
    reference: Some(reference.into()),
    complete: true,
    length: Some(10),
    at,
  }
}

#[test]
fn a_job_s_calls_are_paired_with_their_results_and_one_still_out_runs_to_the_end() {
  let lines = JobLines {
    calls: vec![
      call(1, "Bash", 1_000, "a", Some("cargo test -p a-crate")),
      call(2, "Read", 2_000, "b", None),
      call(3, "Bash", 3_000, "c", Some("sleep 9")),
    ],
    results: vec![result(4, 61_000, "a"), result(5, 2_500, "b")],
    requests: 3,
    compactions: 0,
  };
  let split = tool_calls(calls_of(&lines), 13_000).unwrap();

  assert_eq!(split.times, 3);
  assert_eq!(split.open, 1);
  assert_eq!(split.calls[0].name, "Bash");
  assert_eq!(split.calls[0].seconds, 60.0);
  assert_eq!(split.calls[1].seconds, 10.0);
  assert!(split.calls[1].open);
  assert_eq!(split.tests, (60, 1));
  assert_eq!(split.tools[0].name, "Bash");
  assert_eq!(split.tools[0].times, 2);
  assert!(tool_calls(vec![], 0).is_none());
}

fn stamped(at_seconds: i64, value: serde_json::Value) -> serde_json::Value {
  let mut value = value;
  let at = chrono::DateTime::from_timestamp(at_seconds, 0).unwrap().to_rfc3339();

  value["timestamp"] = json!(at);
  value
}

#[test]
fn a_branch_divides_into_the_stretch_its_calls_were_out_and_the_rest() {
  let entries = vec![
    stamped(0, json!({ "type": "user" })),
    stamped(
      10,
      json!({ "message": { "content": [{ "type": "tool_use", "id": "x", "name": "Bash" }, { "type": "tool_use", "id": "y", "name": "Read" }] } }),
    ),
    stamped(40, json!({ "message": { "content": [{ "type": "tool_result", "tool_use_id": "x" }] } })),
    stamped(30, json!({ "message": { "content": [{ "type": "tool_result", "tool_use_id": "y" }] } })),
    stamped(100, json!({ "type": "assistant" })),
  ];
  let split = branch_split(&entries).unwrap();

  assert_eq!(
    (split.seconds, split.tooling, split.generating, split.calls, split.open),
    (100, 30, 70, 2, 0)
  );
  assert_eq!(split.tools[0].name, "Bash");
  assert!(branch_split(&[stamped(5, json!({}))]).is_none());
  assert_eq!(grouped(1_234_567), "1,234,567");
  assert_eq!(
    project_dir(None, Path::new("/srv/user"), Path::new("/srv/a b")),
    Path::new("/srv/user/.claude/projects/-srv-a-b")
  );
}

#[test]
fn runs_side_by_side_count_once_and_the_time_between_counts_for_none() {
  let runs = [
    Run {
      from: 0.0,
      to: 10.0,
    },
    Run {
      from: 5.0,
      to: 15.0,
    },
    Run {
      from: 20.0,
      to: 25.0,
    },
  ];

  assert_eq!(run_seconds(&runs), Some(20.0));
  assert_eq!(run_seconds(&[]), None);
}

#[test]
fn a_proxy_call_reads_as_the_tool_it_reached_in_its_road_s_spelling() {
  assert_eq!(reached("mcp__toolbook__call", "web/web_fetch").as_deref(), Some("mcp__web__web_fetch"));
  assert_eq!(reached("toolbook/act", "web/web_fetch").as_deref(), Some("web/web_fetch"));
  assert_eq!(reached("mcp__web__web_fetch", "web/web_fetch"), None);
}

#[test]
fn a_manifest_a_lockfile_or_the_submodules_moving_asks_for_the_setup_again() {
  assert!(setup_inputs_moved(&["crates/a/Cargo.toml".into()]));
  assert!(setup_inputs_moved(&["bun.lock".into()]));
  assert!(setup_inputs_moved(&[".gitmodules".into()]));
  assert!(!setup_inputs_moved(&["src/lib.rs".into(), "docs/x.md".into()]));
}

fn words(given: &[&str]) -> Vec<String> {
  given.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn each_verb_is_one_request_and_a_call_with_none_is_a_read() {
  let request = |given: &[&str]| request_from_args(3, &words(given));

  assert_eq!(request(&["--todo", "3"]), Ok(None));
  assert_eq!(
    request(&["--todo", "3", "--skip", "build-2", "--why", "not doing it"]),
    Ok(Some(json!({ "id": 3, "action": "skip", "task": "build-2", "why": "not doing it" })))
  );
  assert_eq!(
    request(&["--todo", "3", "--rerun", "build-0", "--model", "provider::model"]),
    Ok(Some(
      json!({ "id": 3, "action": "rerun", "task": "build-0", "model": "provider::model" })
    ))
  );
  assert_eq!(
    request(&["--todo", "3", "--move", "build-3", "--before", "build-1"]),
    Ok(Some(json!({ "id": 3, "action": "move", "task": "build-3", "before": "build-1" })))
  );
  assert_eq!(
    request(&[
      "--todo",
      "3",
      "--add",
      "Wire the reader",
      "--instruction",
      "Read the new field",
      "--after",
      "build-1"
    ]),
    Ok(Some(
      json!({ "id": 3, "action": "add", "title": "Wire the reader", "instruction": "Read the new field", "after": "build-1" })
    ))
  );
  assert_eq!(
    request(&["--todo", "3", "--model", "auto"]),
    Ok(Some(json!({ "id": 3, "action": "save", "model": "auto" })))
  );
  assert_eq!(request(&["--todo", "3", "--resume"]), Ok(Some(json!({ "id": 3, "action": "resume" }))));
  assert_eq!(
    request(&["--todo", "3", "--recover", "900561-900585"]),
    Ok(Some(json!({ "id": 3, "action": "recover", "from": 900_561, "to": 900_585 })))
  );
  assert_eq!(
    request(&["--todo", "3", "--checkout", "/srv/checkout"]),
    Ok(Some(json!({ "id": 3, "action": "checkout", "cwd": "/srv/checkout" })))
  );
}

#[test]
fn two_verbs_a_range_that_does_not_read_and_a_flag_without_its_value_are_refused() {
  let request = |given: &[&str]| request_from_args(3, &words(given));

  assert_eq!(
    request(&["--todo", "3", "--checkout", "/a", "--resume"]),
    Err("one change a call, and this names --resume, --checkout".into())
  );
  assert!(
    request(&["--todo", "3", "--recover", "900561"])
      .unwrap_err()
      .starts_with("--recover takes")
  );
  assert_eq!(request(&["--todo", "3", "--skip", "--why", "x"]), Err("--skip needs its value".into()));
}

#[test]
fn an_item_number_that_is_not_one_is_refused_naming_what_was_given() {
  assert_eq!(item_of(&words(&["--help"])), Ok(None));
  assert_eq!(item_of(&words(&["--todo", "4"])), Ok(Some(4)));

  let refused = item_of(&words(&["--todo", "abc"])).unwrap_err();

  assert_eq!(refused.code, 2);
  assert_eq!(refused.lines[0], "work-order: --todo needs one item number ('abc' is not a number).");
  assert_eq!(item_of(&words(&[])).unwrap_err().lines[0], "work-order: --todo needs one item number.");
  assert!(item_of(&words(&["--todo", "0"])).is_err());
  assert_eq!(capped(vec!["a".into(), "b".into()], 3), ["a", "b"]);
}

// 2026-10-17 10:00 UTC.
const NOW: i64 = 1_792_231_200_000;

#[test]
fn a_wait_s_end_is_a_length_from_now_or_a_clock_time_in_utc() {
  assert_eq!(wait_end("40m", NOW), Some(NOW + 40 * 60_000));
  assert_eq!(wait_end("1h30m", NOW), Some(NOW + 90 * 60_000));
  assert_eq!(wait_end("2h", NOW), Some(NOW + 120 * 60_000));
  assert_eq!(wait_end("14:30", NOW), Some(NOW + (4 * 60 + 30) * 60_000));
  assert_eq!(wait_end("09:00", NOW), Some(NOW + 23 * 60 * 60_000));
  assert_eq!(wait_end("soon", NOW), None);
  assert_eq!(wait_end("25:00", NOW), None);
}

#[test]
fn a_wait_s_words_are_read_into_its_request_or_refused_by_what_they_lack() {
  let words = |what: Option<&str>, unit: Option<&str>| WaitWords {
    todo: Some("7".into()),
    what: what.map(str::to_owned),
    unit: unit.map(str::to_owned),
    until: Some("40m".into()),
    ..WaitWords::default()
  };

  assert_eq!(
    read_request(words(Some("the deploy"), Some("deploy")), NOW),
    Ok(WaitRequest::Mark {
      todo_id: 7,
      what: "the deploy".into(),
      target: Some(WaitTarget::Unit {
        name: "deploy".into()
      }),
      until: NOW + 40 * 60_000,
      command: vec![],
    })
  );
  assert_eq!(
    read_request(words(None, Some("deploy")), NOW),
    Err(WordsFault::Mark(JobWaitRefusal::WaitNeedsWhat))
  );
  assert_eq!(
    read_request(words(Some("x"), None), NOW),
    Err(WordsFault::Mark(JobWaitRefusal::WaitNeedsTarget))
  );
  assert!(matches!(
    read_request(
      WaitWords {
        command: Some(vec![]),
        ..words(Some("x"), None)
      },
      NOW
    ),
    Err(WordsFault::Form(_))
  ));
  assert_eq!(
    read_request(
      WaitWords {
        todo: Some("7".into()),
        clear: true,
        ..WaitWords::default()
      },
      NOW
    ),
    Ok(WaitRequest::Clear { todo_id: 7 })
  );
}

#[test]
fn a_keep_s_length_reads_minutes_hours_and_seconds() {
  assert_eq!(duration_seconds("20m"), Some(1200));
  assert_eq!(duration_seconds("1h"), Some(3600));
  assert_eq!(duration_seconds("90s"), Some(90));
  assert_eq!(duration_seconds("15"), Some(900));
  assert_eq!(duration_seconds("soon"), None);
}

fn item(held_kind: Option<&str>) -> Row {
  Row {
    id: 12,
    title: "an item".to_owned(),
    instruction: "instruction".to_owned(),
    done: false,
    priority: 0,
    note: None,
    report: None,
    unfinished: None,
    unfinished_resolution: None,
    held_reason: None,
    held_kind: held_kind.map(str::to_owned),
    merged_into: None,
  }
}

// An order at the given phase, its final turn run as job 40, its build list in the given states.
fn order(phase: &str, builds: &[&str]) -> serde_json::Value {
  let tasks: Vec<serde_json::Value> =
    builds.iter().map(|state| json!({ "id": "build", "state": state })).collect();

  json!({ "phase": phase, "tasks": tasks, "control": { "id": "final", "state": "running", "jobId": 40 } })
}

#[test]
fn a_run_of_a_work_order_holds_its_item_only_at_a_final_turn_with_nothing_ahead_and_only_once() {
  assert_eq!(order_hold_refusal(&order("implementation", &["running"]), 41), Some(OrderHold::Step));
  assert_eq!(order_hold_refusal(&order("finalization", &["done"]), 41), Some(OrderHold::Step));
  assert_eq!(
    order_hold_refusal(&order("finalization", &["done", "pending"]), 40),
    Some(OrderHold::StepsAhead)
  );
  assert_eq!(order_hold_refusal(&order("finalization", &["done", "skipped"]), 40), None);

  let mut held = order("finalization", &["done"]);

  held["finalHolds"] = json!(1);
  assert_eq!(order_hold_refusal(&held, 40), Some(OrderHold::HeldOnce));
}

#[test]
fn a_stop_here_is_refused_by_what_its_item_stands_at() {
  let mut finished = item(None);

  finished.done = true;

  assert!(matches!(
    stop_here_refusal(&finished, false, "", false),
    Some(TodoRefusal::StopHereClosed { id: 12, .. })
  ));
  assert_eq!(
    stop_here_refusal(&item(Some("pause")), true, "why", false),
    Some(TodoRefusal::StopHereHeldTakesNoReason { id: 12 })
  );
  assert_eq!(stop_here_refusal(&item(Some("stop")), false, "", false), None);
  assert_eq!(
    stop_here_refusal(&item(None), false, "", false),
    Some(TodoRefusal::StopHereNotHeld { id: 12 })
  );
  assert_eq!(
    stop_here_refusal(&item(None), true, "", false),
    Some(TodoRefusal::StopHereNeedsReason { id: 12 })
  );
  assert_eq!(stop_here_refusal(&item(None), true, "told to stop", false), None);
}

#[test]
fn a_held_item_s_reason_carries_its_resume_condition() {
  let after = Condition::Finished { id: 4 };
  let live = Condition::Deployed {
    commits: vec!["abcdef0123".into()],
    item: None,
  };

  assert_eq!(wind_down_reason("waits", None), "waits");
  assert_eq!(wind_down_reason("waits", Some(&after)), "waits (resumes once #4 is finished)");
  assert_eq!(wind_down_reason("", Some(&live)), "resumes once abcdef0 goes live");
}

fn banned() -> Vec<BannedTerm> {
  vec![BannedTerm {
    lang: "en".into(),
    term: "utilise".into(),
    instead: "use".into(),
    keeps: vec![],
  }]
}

#[test]
fn a_question_s_wording_is_judged_part_by_part() {
  let options = || vec!["Keep it".to_owned(), "Cut it".to_owned()];

  assert_eq!(wording_refusal("", &options(), Some(1), "why", &[]), Some(JobAskRefusal::QuestionText));
  assert_eq!(
    wording_refusal("Which?", &options()[..1], Some(1), "why", &[]),
    Some(JobAskRefusal::QuestionOptions)
  );
  assert_eq!(
    wording_refusal("Which?", &options(), Some(3), "why", &[]),
    Some(JobAskRefusal::QuestionRecommend { count: 2 })
  );
  assert_eq!(
    wording_refusal("Which?", &options(), Some(1), " ", &[]),
    Some(JobAskRefusal::QuestionWhy)
  );
  assert_eq!(wording_refusal("Which?", &options(), Some(2), "why", &[]), None);
  assert_eq!(
    wording_refusal("Which do we utilise?", &options(), Some(1), "why", &banned()),
    Some(JobAskRefusal::QuestionBannedTerms {
      terms: vec!["utilise".into()]
    })
  );
  assert_eq!(
    words_refusal(&["See https://gist.github.com/a/b."], &[]),
    Some(JobAskRefusal::QuestionOutsideHosting {
      addresses: vec!["https://gist.github.com/a/b".into()]
    })
  );
}

#[test]
fn a_question_goes_to_the_owner_s_own_chat_and_names_its_item() {
  let request = AskRequest {
    room: Some(5),
    todo_id: Some(1),
    question: "Which?".into(),
    options: vec!["a".into(), "b".into()],
    recommended: Some(1),
    why: "why".into(),
  };

  assert_eq!(ask_refusal(&request, 9, &[]), Some(JobAskRefusal::QuestionRoom));
  assert_eq!(
    ask_refusal(
      &AskRequest {
        room: Some(9),
        ..request.clone()
      },
      9,
      &[]
    ),
    None
  );
  assert_eq!(
    ask_refusal(
      &AskRequest {
        room: None,
        todo_id: None,
        ..request
      },
      9,
      &[]
    ),
    Some(JobAskRefusal::QuestionItem)
  );
}

#[test]
fn a_job_s_own_process_carries_the_program_s_mark_and_the_turn_s() {
  let env = |pairs: &[(&str, &str)]| -> BTreeMap<String, String> {
    pairs.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())).collect()
  };
  let job =
    JobProcess::of(&env(&[("CLAUDECODE", "1"), ("BMO_TASK_LOCKED", "1"), ("BMO_RUN_NAME", "bmo-task-7")]));
  let room = JobProcess::of(&env(&[("CLAUDECODE", "1"), ("BMO_RUN_NAME", "room")]));
  let step = JobProcess::of(&env(&[("BMO_WORK_ORDER_STEP", "1")]));

  assert_eq!(job.job_run(), Some("bmo-task-7"));
  assert_eq!(room.job_run(), None);
  assert!(step.work_order_step && !step.in_job);
}

#[test]
fn a_length_of_time_reads_at_a_glance() {
  assert_eq!(span(45.4), "45s");
  assert_eq!(span(125.0), "2m 05s");
  assert_eq!(span(3_720.0), "1h 02m");
}
