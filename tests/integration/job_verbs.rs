//! A job's own verbs (docs/verbs.md): the long wait's mark, the live-Claude guard's opening, the stop's
//! targets, a launch from a shell and a borrowed checkout, each through its function on a stand of the test's
//! own and through the binary as a job's shell runs it.

use std::path::Path;

use axum::Router;
use axum::routing::post;
use bmo_job::queue::telling::JobWait;
use bmo_job::verbs::live_claude;
use bmo_job::verbs::stand::VerbError;
use bmo_job::verbs::task::{TaskAsked, task};
use bmo_job::verbs::waiting::{WaitRequest, WaitWords, WordsFault, read_request, wait_end, waiting};
use bmo_job::verbs::{Built, Target, TargetKind};
use job_codes::JobWaitRefusal;
use job_codes::job_wait::WaitTarget;
use serde_json::json;

use crate::support::desk::Desk;
use crate::support::program::{linked, set_shell};
use crate::support::server::{Served, code};
use crate::support::stub;

// 2026-09-25 12:00 UTC.
const NOON: i64 = 1_790_337_600_000;
const MINUTE: i64 = 60_000;

fn ended(answer: Result<impl std::fmt::Debug, VerbError>) -> (u8, String) {
  match answer {
    Err(VerbError::Ended { code, said }) => (code, said),
    other => panic!("not ended: {other:?}"),
  }
}

// The markers of a job's own run.
fn in_job(run: &str) -> [(&'static str, String); 3] {
  [
    ("CLAUDECODE", "1".into()),
    ("BMO_TASK_LOCKED", "1".into()),
    ("BMO_RUN_NAME", run.into()),
  ]
}

fn env<'a>(pairs: &'a [(&'static str, String)]) -> Vec<(&'static str, &'a str)> {
  pairs.iter().map(|(name, value)| (*name, value.as_str())).collect()
}

async fn running_job(desk: &Desk, unit: &str, todo: i64) -> i64 {
  desk
    .run(
      r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "todo_id", "worker") VALUES (42, $1, 60, 'work', $2, 'worker')"#,
      vec![unit.into(), todo.into()],
    )
    .await;
  desk.count(r#"SELECT MAX("id") FROM "jobs""#, vec![]).await
}

async fn mark(desk: &Desk, job: i64) -> Option<JobWait> {
  bmo_job::queue::settings::get(&desk.stand.db, &format!("job_wait:{job}"))
    .await
    .unwrap()
    .as_deref()
    .and_then(JobWait::read)
}

// The verb's words as the command line hands them over: each flag's value, and what followed `--`.
fn words(args: &[&str]) -> WaitWords {
  let (own, command) = match args.iter().position(|arg| *arg == "--") {
    Some(at) => (&args[..at], Some(args[at + 1..].iter().map(|arg| (*arg).to_owned()).collect())),
    None => (args, None),
  };
  let value = |name: &str| {
    own
      .iter()
      .position(|arg| *arg == name)
      .map(|at| own.get(at + 1).copied().unwrap_or("").to_owned())
  };

  WaitWords {
    todo: value("--todo"),
    what: value("--what"),
    unit: value("--unit"),
    pid: value("--pid"),
    until: value("--until"),
    clear: own.contains(&"--clear"),
    command,
  }
}

#[test]
fn the_end_a_job_gives_is_a_length_from_now_or_a_clock_time_in_utc_the_next_one_to_come() {
  assert_eq!(wait_end("40m", NOON), Some(NOON + 40 * MINUTE));
  assert_eq!(wait_end("2h", NOON), Some(NOON + 120 * MINUTE));
  assert_eq!(wait_end("1h30m", NOON), Some(NOON + 90 * MINUTE));
  assert_eq!(wait_end("14:30", NOON), Some(NOON + 150 * MINUTE));
  assert_eq!(wait_end("09:15", NOON), Some(NOON + (21 * 60 + 15) * MINUTE));
}

#[test]
fn anything_else_is_no_end() {
  for text in ["", "0m", "soon", "25:00", "12:60", "m", "h", "1h2", "1:5"] {
    assert_eq!(wait_end(text, NOON), None, "{text}");
  }
}

#[test]
fn the_verb_s_words_read_a_mark_on_a_unit_on_a_pid_around_a_command_and_a_clearing() {
  assert_eq!(
    read_request(
      words(&["--todo", "776", "--what", "judging round", "--unit", "bmo-judge-1", "--until", "30m"]),
      NOON
    ),
    Ok(WaitRequest::Mark {
      todo_id: 776,
      what: "judging round".into(),
      target: Some(WaitTarget::Unit {
        name: "bmo-judge-1".into()
      }),
      until: NOON + 30 * MINUTE,
      command: vec![],
    })
  );
  assert!(matches!(
    read_request(words(&["--todo", "776", "--what", "training", "--pid", "4242", "--until", "13:00"]), NOON),
    Ok(WaitRequest::Mark { target: Some(WaitTarget::Pid { pid: 4242 }), until, .. }) if until == NOON + 60 * MINUTE
  ));
  assert!(matches!(
    read_request(words(&["--todo", "776", "--what", "check", "--until", "10m", "--", "sleep", "--pid", "1"]), NOON),
    Ok(WaitRequest::Mark { target: None, command, .. }) if command == ["sleep", "--pid", "1"]
  ));
  assert_eq!(
    read_request(words(&["--todo", "776", "--clear"]), NOON),
    Ok(WaitRequest::Clear { todo_id: 776 })
  );
}

#[test]
fn the_verb_s_words_refuse_what_cannot_be_a_mark_saying_why() {
  let refused = |args: &[&str]| read_request(words(args), NOON).unwrap_err();
  let form = |fault: WordsFault| match fault {
    WordsFault::Form(why) => why,
    other => panic!("not a fault of form: {other:?}"),
  };

  assert!(form(refused(&["--what", "x", "--unit", "u", "--until", "5m"])).contains("--todo <id>"));
  assert_eq!(
    refused(&["--todo", "1", "--unit", "u", "--until", "5m"]),
    WordsFault::Mark(JobWaitRefusal::WaitNeedsWhat)
  );
  assert!(form(refused(&["--todo", "1", "--what", "x", "--unit", "u"])).contains("--until 40m"));
  assert_eq!(
    refused(&["--todo", "1", "--what", "x", "--until", "5m"]),
    WordsFault::Mark(JobWaitRefusal::WaitNeedsTarget)
  );
  assert_eq!(
    refused(&["--todo", "1", "--what", "x", "--unit", "u", "--pid", "2", "--until", "5m"]),
    WordsFault::Mark(JobWaitRefusal::WaitNeedsTarget)
  );
  assert_eq!(
    form(refused(&["--todo", "1", "--what", "x", "--pid", "abc", "--until", "5m"])),
    "--pid takes a process number."
  );
  assert!(
    form(refused(&[
      "--todo", "1", "--what", "x", "--unit", "u", "--until", "5m", "--", "sleep", "1"
    ]))
    .contains("its own target")
  );
  assert!(
    form(refused(&["--todo", "1", "--what", "x", "--until", "5m", "--"])).contains("nothing follows --")
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_verb_refuses_an_item_with_no_running_job_and_a_target_that_is_not_alive_and_clears_a_mark() {
  let desk = Desk::new().await;
  let job = running_job(&desk, "bmo-task-wait", 31).await;
  let own = std::process::id();
  let mut gone = std::process::Command::new("true").spawn().unwrap();
  let gone_pid = gone.id();
  let mark_on = |todo_id: i64, pid: u32| WaitRequest::Mark {
    todo_id,
    what: "x".into(),
    target: Some(WaitTarget::Pid { pid }),
    until: NOON + 5 * MINUTE,
    command: vec![],
  };

  gone.wait().unwrap();

  let (code, said) = ended(waiting(&desk.stand, mark_on(32, own), NOON).await);

  assert_eq!(code, 1);
  assert!(said.contains("has no running job"), "{said}");

  let (code, said) = ended(waiting(&desk.stand, mark_on(31, gone_pid), NOON).await);

  assert_eq!(code, 1);
  assert!(said.contains(&format!("process {gone_pid} is not alive")), "{said}");
  assert_eq!(mark(&desk, job).await, None);

  let marked = waiting(&desk.stand, mark_on(31, own), NOON).await.unwrap();

  assert_eq!(marked.code, 0);
  assert_eq!(mark(&desk, job).await.map(|wait| wait.target), Some(WaitTarget::Pid { pid: own }));

  let cleared = waiting(&desk.stand, WaitRequest::Clear { todo_id: 31 }, NOON).await.unwrap();

  assert_eq!(cleared.lines, ["wait: #31's mark is cleared"]);
  assert_eq!(mark(&desk, job).await, None);
  assert_eq!(
    waiting(&desk.stand, WaitRequest::Clear { todo_id: 31 }, NOON).await.unwrap().lines,
    ["wait: #31 carries no mark"]
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_verb_marks_a_command_it_runs_on_its_own_process_clears_it_when_the_command_ends_and_exits_with_its_code()
 {
  let desk = Desk::new().await;
  let job = running_job(&desk, "bmo-task-wait", 41).await;
  let signal = desk.scratch.path().join("go");
  let script = format!("until [ -e {} ]; do sleep 0.05; done; exit 3", signal.display());
  let words = [
    "job", "wait", "--todo", "41", "--what", "check", "--until", "10m", "--", "sh", "-c", &script,
  ];
  let (ran, seen) = tokio::join!(desk.binary(desk.scratch.path(), &words, &[]), async {
    let mut seen = None;

    for _ in 0..400 {
      tokio::time::sleep(std::time::Duration::from_millis(25)).await;
      seen = mark(&desk, job).await;

      if seen.is_some() {
        break;
      }
    }

    std::fs::write(&signal, "").unwrap();
    seen
  });
  let seen = seen.expect("the mark stood while the command ran");
  let WaitTarget::Pid { pid } = seen.target else {
    panic!("the mark is on a process: {seen:?}")
  };
  let out = String::from_utf8_lossy(&ran.stdout).into_owned();

  assert_eq!((seen.what.as_str(), seen.ended_at), ("check", None));
  assert_eq!(ran.status.code(), Some(3), "{}", String::from_utf8_lossy(&ran.stderr));
  assert!(out.starts_with(&format!("wait: #41 waits on check (process {pid}) until ")), "{out}");
  assert!(out.ends_with("wait: #41 is done waiting on check (exit 3)\n"), "{out}");
  assert_eq!(mark(&desk, job).await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_guard_is_opened_for_the_job_its_run_name_belongs_to_and_lists_what_it_opened() {
  let desk = Desk::new().await;
  let db = &desk.stand.db;

  running_job(&desk, "bmo-task-guard", 331).await;

  assert_eq!(
    live_claude::list(db, "bmo-task-guard").await.unwrap(),
    ["live-claude: this job has opened nothing"]
  );

  let opened = live_claude::open(db, "bmo-task-guard", " three judge measurements ", 1_000)
    .await
    .unwrap();
  let opening = desk.count(r#"SELECT MAX("id") FROM "live_claude_openings""#, vec![]).await;

  assert_eq!(
    opened,
    [format!(
      "live-claude: opened #{opening} for this job — three judge measurements. Every run it lets through is recorded; name them in the report."
    )]
  );

  desk
    .run(
      r#"INSERT INTO "live_claude_runs" ("opening_id", "what", "ran_at") VALUES ($1, 'shell: claude -p', $2)"#,
      vec![opening.into(), NOON.into()],
    )
    .await;

  assert_eq!(
    live_claude::list(db, "bmo-task-guard").await.unwrap(),
    [
      format!("#{opening} three judge measurements — 1 run"),
      "  2026-09-25T12:00:00.000Z shell: claude -p".to_owned()
    ]
  );
  assert_eq!(
    desk
      .count(r#"SELECT COUNT(*) FROM "live_claude_openings" WHERE "todo_id" = 331"#, vec![])
      .await,
    1
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_guard_refuses_a_process_that_is_not_a_job_s_and_an_opening_that_says_nothing() {
  let desk = Desk::new().await;
  let here = desk.scratch.path();
  let markers = in_job("bmo-task-guard");
  let job = env(&markers);

  running_job(&desk, "bmo-task-guard", 331).await;

  let ran = desk.binary(here, &["job", "live-claude", "--purpose", "x"], &[]).await;

  assert_eq!(ran.status.code(), Some(1));
  assert!(String::from_utf8_lossy(&ran.stderr).contains("this process is not a job's"));

  let ran = desk.binary(here, &["job", "live-claude", "--purpose", "  "], &job).await;

  assert_eq!(ran.status.code(), Some(1));
  assert!(String::from_utf8_lossy(&ran.stderr).contains("say what the runs are for with --purpose"));
  assert_eq!(desk.count(r#"SELECT COUNT(*) FROM "live_claude_openings""#, vec![]).await, 0);

  let ran = desk.binary(here, &["job", "live-claude", "--purpose", "a measurement"], &job).await;

  assert_eq!(ran.status.code(), Some(0), "{}", String::from_utf8_lossy(&ran.stderr));
  assert!(String::from_utf8_lossy(&ran.stdout).contains("for this job — a measurement."));

  let ran = desk.binary(here, &["job", "live-claude", "--list"], &job).await;

  assert_eq!(ran.status.code(), Some(0));
  assert!(String::from_utf8_lossy(&ran.stdout).contains("a measurement — 0 runs"));
}

#[test]
fn the_shape_a_stop_target_is_named_by_is_the_shape_it_is_handed_back_as() {
  let unit = Target {
    kind: TargetKind::Unit,
    id: "bmo-search-9".into(),
  };

  assert_eq!(unit.to_string(), "unit:bmo-search-9");
  assert_eq!(serde_json::to_value(&unit).unwrap(), json!({ "kind": "unit", "id": "bmo-search-9" }));
  assert_eq!(
    serde_json::from_value::<Target>(json!({ "kind": "execution", "id": "bmo-execution-chat" }))
      .unwrap()
      .to_string(),
    "execution:bmo-execution-chat"
  );
  assert_eq!(
    serde_json::from_value::<Target>(json!({ "kind": "item", "id": "623" }))
      .unwrap()
      .to_string(),
    "item:623"
  );

  for wrong in [json!({ "kind": "runner", "id": "x" }), json!({ "kind": "unit" }), json!("unit:x")] {
    assert!(serde_json::from_value::<Target>(wrong.clone()).is_err(), "{wrong}");
  }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_is_refused_for_an_item_named_otherwise_than_by_number_a_co_worker_and_a_run_nobody_knows() {
  let served = Served::<Built>::ported().await;
  let execution = |id: &str, shared_pane: bool, parent: Option<&str>| {
    json!({
      "id": id, "parent": parent, "root": parent, "line": null, "shared_pane": shared_pane, "started_at": 1,
      "ended_at": null, "outcome": null, "cancelled": false, "details": {}
    })
  };
  let refused = |answer: &(u16, serde_json::Value)| (answer.0, code(&answer.1).to_owned());

  assert_eq!(
    refused(
      &served
        .post("/run/stop", json!({ "target": { "kind": "item", "id": "06" }, "why": "x" }))
        .await
    ),
    (400, "stop_item_number".into())
  );

  served
    .agency
    .answer("GET /executions/{id}", 200, execution("ex-child", true, Some("ex-parent")));

  let answer = served
    .post("/run/stop", json!({ "target": { "kind": "execution", "id": "ex-child" } }))
    .await;

  assert_eq!(refused(&answer), (409, "stop_coworker".into()));
  assert!(answer.1["error"].as_str().unwrap().contains("the run that launched it"), "{}", answer.1);

  served.agency.answer_empty("GET /executions/{id}", 404);
  assert_eq!(
    refused(
      &served
        .post("/run/stop", json!({ "target": { "kind": "execution", "id": "ex-gone" } }))
        .await
    ),
    (404, "stop_target_absent".into())
  );
}

// A desk whose server is a stand-in answering `POST /task`.
async fn with_server() -> (Desk, stub::Stub) {
  let server = stub::Stub::start(Router::new().route("/task", post(stub::take))).await;
  let address = server.address();
  let desk = Desk::edited(move |_, text| {
    text.replace("address = \"http://127.0.0.1:1/\"", &format!("address = \"{address}\""))
  })
  .await;

  (desk, server)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_launch_is_handed_to_the_server_s_task_route_and_its_answer_said_as_it_came() {
  let (desk, server) = with_server().await;

  server.answer("POST /task", 200, json!({ "code": "job_started", "job": 7 }));

  let asked = TaskAsked {
    chat: 42,
    prompt: "work".into(),
    item: Some(7),
    started_by: Some("console".into()),
    ..TaskAsked::default()
  };
  let answer = task(&desk.stand, &asked).await.unwrap();

  assert_eq!(answer.lines, [json!({ "code": "job_started", "job": 7 }).to_string()]);
  server.took(
    "POST /task",
    "/task",
    Some(json!({
      "chat": 42, "prompt": "work", "model": null, "item": 7, "purpose": null, "worker": null, "cowork": false,
      "started_by": "console"
    })),
  );

  // A prompt that begins with two dashes is the prompt it is, and a why is trimmed.
  let ran = desk
    .binary(
      desk.scratch.path(),
      &[
        "job",
        "start",
        "--chat",
        "-100",
        "--why",
        " a trial launch ",
        "--cowork",
        "--prompt",
        "--made-up-flag typed as words",
      ],
      &[],
    )
    .await;

  assert_eq!(ran.status.code(), Some(0), "{}", String::from_utf8_lossy(&ran.stderr));
  assert_eq!(
    server.calls()[1].body,
    Some(json!({
      "chat": -100, "prompt": "--made-up-flag typed as words", "model": null, "item": null, "purpose": "a trial launch",
      "worker": null, "cowork": true, "started_by": null
    }))
  );

  // The server's refusal is what the caller reads, and the run ends as a failure.
  server.answer(
    "POST /task",
    409,
    json!({ "code": "checkout_busy", "error": "every checkout is at work" }),
  );

  let (code, said) = ended(task(&desk.stand, &asked).await);

  assert_eq!(code, 1);
  assert!(said.starts_with("task refused: ") && said.ends_with("nothing was launched."), "{said}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_launch_goes_nowhere_where_the_server_does_not_answer_and_words_that_do_not_read_are_refused() {
  let desk = Desk::new().await;
  let here = desk.scratch.path();
  let asked = TaskAsked {
    chat: 42,
    prompt: "work".into(),
    item: Some(7),
    ..TaskAsked::default()
  };
  let (code, said) = ended(task(&desk.stand, &asked).await);

  assert_eq!(code, 1);
  assert!(said.contains("nothing was launched"), "{said}");

  let ran = desk.binary(here, &["job", "start", "--chat", "42", "--prompt", "  "], &[]).await;

  assert_eq!(ran.status.code(), Some(2));
  assert!(String::from_utf8_lossy(&ran.stderr).contains("usage: job start"));

  let ran = desk
    .binary(here, &["job", "start", "--chat", "42", "--made-up-flag", "--prompt", "work"], &[])
    .await;

  assert_eq!(ran.status.code(), Some(2));
  assert!(String::from_utf8_lossy(&ran.stderr).contains("--made-up-flag"));
  assert_eq!(desk.count(r#"SELECT COUNT(*) FROM "jobs""#, vec![]).await, 0);
}

// A git repository of its own on main, with one commit, standing for another worker's checkout.
fn repository(at: &Path) {
  for args in [
    vec!["init", "-q", "-b", "main"],
    vec![
      "-c",
      "user.email=tester@example.invalid",
      "-c",
      "user.name=tester",
      "commit",
      "-q",
      "--allow-empty",
      "-m",
      "start",
    ],
  ] {
    assert!(
      std::process::Command::new("git")
        .args(&args)
        .current_dir(at)
        .status()
        .unwrap()
        .success()
    );
  }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_borrows_a_free_checkout_of_another_worker_s_repository_and_nothing_else() {
  let desk = Desk::new().await;
  let checkout = tempfile::tempdir().unwrap();
  let cwd = checkout.path().display().to_string();
  let here = desk.scratch.path();
  let markers = in_job("bmo-task-borrow");
  let job_env = env(&markers);

  // The service manager stands in where the settings name it, saying nothing runs, and the agency holds no
  // open execution.
  desk.agency.answer("GET /executions", 200, json!([]));
  linked(here, "no-systemctl");
  set_shell(
    here,
    "for unit in \"$@\"; do case \"$unit\" in --user|is-active) ;; *) echo inactive ;; esac; done\n",
  );
  repository(checkout.path());
  desk
    .run(
      r#"INSERT INTO "workers" ("name", "label", "cwd", "builtin", "support_count", "created_at") VALUES ('other', 'Other', $1, 0, 0, 1)"#,
      vec![cwd.clone().into()],
    )
    .await;

  let job = running_job(&desk, "bmo-task-borrow", 77).await;
  let ran = desk.binary(here, &["job", "borrow", "other"], &[]).await;

  assert_eq!(ran.status.code(), Some(1));
  assert!(
    String::from_utf8_lossy(&ran.stderr)
      .contains("a checkout is lent to a job, and this is not run inside one"),
    "{}",
    String::from_utf8_lossy(&ran.stderr)
  );

  let ran = desk.binary(here, &["job", "borrow", "nobody"], &job_env).await;

  assert_eq!(ran.status.code(), Some(1));
  assert!(String::from_utf8_lossy(&ran.stderr).contains("no worker is called nobody"));

  let ran = desk.binary(here, &["job", "borrow", "other"], &job_env).await;
  let out = String::from_utf8_lossy(&ran.stdout).into_owned();

  assert_eq!(ran.status.code(), Some(0), "{}", String::from_utf8_lossy(&ran.stderr));
  assert!(out.contains(&cwd) && out.contains("other"), "{out}");
  assert_eq!(
    desk
      .count(
        r#"SELECT COUNT(*) FROM "checkout_leases" WHERE "worker" = 'other' AND "job_id" = $1"#,
        vec![job.into()]
      )
      .await,
    1
  );

  // Asked again, the job is told it has that checkout already.
  let ran = desk.binary(here, &["job", "borrow", "other"], &job_env).await;

  assert_eq!(ran.status.code(), Some(0));
  assert_eq!(
    String::from_utf8_lossy(&ran.stdout),
    format!("borrow: this job already has other's checkout: {cwd}\n")
  );
}
