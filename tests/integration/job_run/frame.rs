//! A job's checkout before and after (docs/runs.md, "The checkout before and after"): a base checkout beside
//! support workers put on the job's branch and back on main, a support worker's worktree made from its base's
//! main and put on the item's branch, somebody's change open before the job carried back to main as it stood,
//! a work order's middle turn whose answer is the order's, and a job whose turn bmo-agency cannot prepare.

use bmo_job::Refusal;
use bmo_job::job_run::{WORK_SESSION_RESUME, WORK_STEP_PHASE};
use bmo_job::queue::outcomes::{JOB_DONE, JOB_ERROR, JOB_WORK_STEP};

use serde_json::json;

use crate::support::desk::{Desk, OWNER_CHAT};
use crate::support::job::{Bench, WORKER, git};
use crate::support::turns::{Agent, FINISHED, GRAPH_TOLD, Played, TRAILER, commit_work, reply};

/// The harness session a work order's middle turn answered on.
const HARNESS_SESSION: &str = "h-step";

#[tokio::test]
async fn a_done_job_on_a_base_checkout_beside_support_workers_works_on_its_branch_and_is_put_back_on_main()
{
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 1).await;

  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  bench.agent.play([Played::answered(reply("s1", FINISHED))
    .running(&dir, &commit_work(&["printf 'export const made = 1;\\n' > made.ts"]))]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(git(&dir, &["symbolic-ref", "--short", "HEAD"]), "main");
  // The job worked on a branch of its own, which still holds its commit.
  assert!(git(&dir, &["branch", "--format=%(refname:short)"]).contains(&format!("work/job-{job}")));
}

#[tokio::test]
async fn a_job_on_a_support_worker_works_in_a_worktree_made_from_its_base_s_main_on_the_item_s_branch() {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");
  let cwd = bench.at("support-1");

  bench.worker("base", &dir, 1).await;
  bench.support_worker(WORKER, &cwd, "base", 1).await;

  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  bench.agent.play([Played::answered(reply("s1", FINISHED))
    .running(&cwd, &commit_work(&["printf 'export const made = 1;\\n' > made.ts"]))]);

  let ran = bench.run(&bench.given(job, &cwd)).await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(git(&cwd, &["symbolic-ref", "--short", "HEAD"]).starts_with("work/"));
  assert_eq!(git(&cwd, &["log", "-1", "--format=%s"]), "The job did its work");
  // The base checkout stood where it was.
  assert_eq!(git(&dir, &["symbolic-ref", "--short", "HEAD"]), "main");
}

#[tokio::test]
async fn somebody_s_change_open_before_the_job_is_carried_back_to_main_as_it_stood() {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 1).await;
  std::fs::write(dir.join("kept.ts"), "export const kept = 'theirs';\n").unwrap();

  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  // The job commits its own file alone, leaving the change it found open as it stood.
  bench.agent.play([Played::answered(reply("s1", FINISHED)).running(
    &dir,
    "printf 'export const made = 1;\\n' > made.ts\ngit add made.ts\ngit commit -qm 'The job did its work'",
  )]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  // Nobody was asked to close a change that is not the job's own.
  assert_eq!(asked_again(&bench.agent), 0);
  assert_eq!(git(&dir, &["symbolic-ref", "--short", "HEAD"]), "main");
  assert_eq!(git(&dir, &["status", "--porcelain"]), " M kept.ts");
  assert_eq!(std::fs::read_to_string(dir.join("kept.ts")).unwrap(), "export const kept = 'theirs';\n");
}

// Through the binary as the job's unit runs it, on the settings and the stand-ins of the units: every call a
// run makes reaches them over the real ports.
#[tokio::test(flavor = "multi_thread")]
async fn a_work_order_s_middle_turn_ends_with_its_answer_for_the_order_to_read() {
  let desk = Desk::new().await;
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");
  let prompt = bench.at("prompt.txt");
  let task = "Read the panel and say how it is drawn.";
  let agency = &desk.agency;
  let session = format!("{WORKER}@worker");

  desk
    .run(
      r#"INSERT INTO "workers" ("name", "label", "cwd", "created_at") VALUES ($1, $1, $2, 1)"#,
      vec![WORKER.into(), dir.display().to_string().into()],
    )
    .await;
  desk
    .run(
      r#"INSERT INTO "jobs" ("id", "chat_id", "unit", "launched_at", "prompt", "worker") VALUES (7, $1, 'bmo-work-9-build-0-1', $2, $3, $4)"#,
      vec![OWNER_CHAT.into(), (bmo_job::now_ms() - 1000).into(), task.into(), WORKER.into()],
    )
    .await;
  std::fs::write(&prompt, task).unwrap();

  for (route, body) in [
    (
      "GET /providers",
      json!([{ "name": "claude-code", "enabled": true, "registered": false, "coworker": true, "lines": ["worker"] }]),
    ),
    ("GET /agents", json!({ "agents": [{ "name": WORKER, "kind": "worker" }], "roles": [] })),
    ("GET /rest", json!({ "resting": false, "until": null, "kind": null })),
    ("POST /worker", json!({ "ok": true })),
    (
      "POST /routes/resolve",
      json!({ "provider": "claude-code", "tier": "Frontier", "model": "claude-opus-5", "effort": null, "answerTokens": null }),
    ),
    (
      "GET /workers/{worker}/job-documents",
      json!({ "graph": GRAPH_TOLD, "trailer": TRAILER, "handoff": null }),
    ),
    (
      "POST /session/start",
      json!({ "id": session, "agent": WORKER, "scope": { "kind": "worker" } }),
    ),
    (
      "POST /session/turn",
      json!({
        "delivery": "taken",
        "run": "turn:1",
        "outcome": "answered",
        "answer": "The step's result",
        "reply": { "mode": "claude-code", "model": "claude-opus-5", "text": "The step's result", "sessionId": HARNESS_SESSION }
      }),
    ),
    (
      "GET /session/state",
      json!({ "session": session, "provider": "claude-code", "model": "claude-opus-5", "place": "p", "generation": 1, "context": null }),
    ),
  ] {
    agency.answer(route, 200, body);
  }

  for route in ["POST /workers/{worker}/bench", "POST /log/notes", "POST /workers/{worker}/runs"] {
    agency.answer_empty(route, 204);
  }

  desk.gate.answer("POST /sanitised", 200, json!({ "text": task, "changed": false }));

  let words = [
    "job-run",
    "--",
    &OWNER_CHAT.to_string(),
    &prompt.display().to_string(),
    "7",
    "claude-opus-5",
    "bmo-work-9-build-0-1",
    WORKER,
    &dir.display().to_string(),
    WORK_STEP_PHASE,
    WORK_SESSION_RESUME,
  ]
  .map(str::to_owned);
  let words: Vec<&str> = words.iter().map(String::as_str).collect();
  let ran = desk.binary(&dir, &words, &[]).await;
  let row =
    job_base::one(&desk.stand.db, r#"SELECT "outcome", "ok", "report" FROM "jobs" WHERE "id" = 7"#, vec![])
      .await
      .unwrap()
      .unwrap();
  let calls = agency.calls();
  let body = |route: &str| {
    calls
      .iter()
      .find(|call| call.route == route)
      .and_then(|call| call.body.clone())
      .unwrap_or_else(|| panic!("{route} was not called: {calls:?}"))
  };

  assert!(ran.status.success(), "{}", String::from_utf8_lossy(&ran.stderr));
  assert_eq!(row.try_get::<Option<String>>("", "outcome").unwrap().as_deref(), Some(JOB_WORK_STEP));
  assert_eq!(row.try_get::<Option<i64>>("", "ok").unwrap(), Some(1));
  assert_eq!(
    row.try_get::<Option<String>>("", "report").unwrap().as_deref(),
    Some("The step's result")
  );
  assert_eq!(body("POST /session/start")["scope"], json!({ "kind": "worker" }));

  let sent = body("POST /session/turn");

  assert_eq!((&sent["session"], &sent["job"]), (&json!(session), &json!(7)));
  assert!(sent["prompt"].as_str().unwrap().contains(task));
  assert!(
    sent["prompt"].as_str().unwrap().contains("This is one bot-managed co-work step."),
    "{sent}"
  );
  // bmo-agency's turn carries out a waiting reset and records the session and the usage, so none is asked; and a
  // middle turn hands nothing over.
  for asked in ["/sessions/reset", "/sessions/record", "/usage", "/sessions/handover"] {
    assert!(!calls.iter().any(|call| call.target.ends_with(asked)), "{asked}: {calls:?}");
  }
}

#[tokio::test]
async fn a_job_whose_turn_bmo_agency_cannot_prepare_for_its_worker_ends_as_an_error_saying_so() {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");
  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  Agent::set(
    &bench.agent.refusal,
    Some(Refusal::absent("no_worker", "There is no worker named worker to put a turn into.")),
  );

  let ran = bench.run(&bench.given(job, &dir)).await;

  assert!(ran.ended(JOB_ERROR, false), "{ran:?}");
  assert!(
    ran.report.contains("There is no worker named worker to put a turn into."),
    "{}",
    ran.report
  );
  assert!(bench.agent.prompts().is_empty());
}

// How many times the session was asked again over anything its answer left.
pub fn asked_again(agent: &Agent) -> usize {
  agent
    .prompts()
    .iter()
    .filter(|prompt| prompt.contains("Your answer was not sent"))
    .count()
}

// How many times the session was asked to close the change its answer left open.
pub fn asked_to_close(agent: &Agent) -> usize {
  agent
    .prompts()
    .iter()
    .filter(|prompt| prompt.contains("Your answer was not sent. Your own change is still open"))
    .count()
}
