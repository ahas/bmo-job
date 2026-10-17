//! A person's verbs on what runs (docs/verbs.md, "Overtime" and "What a job cost"): an overtime ask listed, kept
//! or cut, and a work order's time merged across its runs — each on a stand of the test's own.

use bmo_job::verbs::cowork_timing::{Orders, Run, cowork_timing, run_seconds};
use bmo_job::verbs::overtime::{USAGE, Verdict, answer, duration_seconds, list};
use bmo_job::verbs::stand::{VerbError, clock};
use serde_json::json;

use crate::support::desk::Desk;

const MINUTE: i64 = 60_000;
// 2026-10-10 00:00 UTC.
const NOW: i64 = 1_791_590_400_000;

fn refused<T: std::fmt::Debug>(answer: Result<T, VerbError>) -> String {
  match answer {
    Err(VerbError::Refused(said)) => said,
    Err(VerbError::Ended { said, .. }) => said,
    other => panic!("not refused: {other:?}"),
  }
}

async fn clock_hold(desk: &Desk, id: i64, clock_ms: i64, answer_by: i64, closed: Option<&str>) {
  desk
    .run(
      r#"INSERT INTO "clock_holds" ("id", "run", "agent", "what", "clock", "clock_ms", "started_at", "asked_at", "answer_by", "closed")
         VALUES ($1, 'worker-w1-run', 'worker', 'item #7', 'turn clock', $2, 0, 0, $3, $4)"#,
      vec![id.into(), clock_ms.into(), answer_by.into(), closed.into()],
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn an_overtime_ask_is_listed_kept_or_cut_and_refused_once_it_is_over() {
  let desk = Desk::new().await;
  let db = &desk.stand.db;

  assert_eq!(list(db, NOW).await.unwrap(), ["(no turn is waiting on an answer)"]);

  clock_hold(&desk, 1, 10 * MINUTE, NOW + 30 * MINUTE, None).await;
  clock_hold(&desk, 2, 10 * MINUTE, NOW - 2 * MINUTE, None).await;
  clock_hold(&desk, 3, 10 * MINUTE, NOW + 30 * MINUTE, Some("ended")).await;

  let listed = list(db, NOW).await.unwrap();

  assert_eq!(listed.len(), 2, "{listed:?}");
  assert!(listed[0].starts_with("#1 worker worker-w1-run: item #7"), "{}", listed[0]);
  assert!(
    listed[0].ends_with(&format!("no answer yet; it is cut at {}", clock(NOW + 30 * MINUTE))),
    "{}",
    listed[0]
  );
  assert!(listed[1].ends_with("nothing to decide"), "{}", listed[1]);

  let keep = |given| Verdict::Keep { for_given: given };

  assert_eq!(
    answer(db, "agent", "1", keep(Some("20m")), Some(" tests running "), NOW).await.unwrap(),
    [format!("ask #1: kept until {}", clock(NOW + 20 * MINUTE))]
  );
  assert!(list(db, NOW).await.unwrap()[0].ends_with(&format!("kept until {}", clock(NOW + 20 * MINUTE))));
  // Kept with no length, it lasts the run's own clock.
  assert_eq!(
    answer(db, "agent", "1", keep(None), None, NOW).await.unwrap(),
    [format!("ask #1: kept until {}", clock(NOW + 10 * MINUTE))]
  );
  assert!(refused(answer(db, "agent", "1", keep(Some("3h")), None, NOW).await).contains("at most"));
  assert_eq!(
    refused(answer(db, "agent", "1", keep(Some("soon")), None, NOW).await),
    "--for takes 20m, 1h or a plain number of minutes, not \"soon\""
  );
  assert_eq!(
    answer(db, "agent", "1", Verdict::Cut, Some("stuck on a prompt"), NOW)
      .await
      .unwrap()
      .len(),
    1
  );
  assert_eq!(
    desk
      .count(
        r#"SELECT COUNT(*) FROM "clock_holds" WHERE "id" = 1 AND "verdict" = 'cut' AND "keep_until" IS NULL
             AND "decided_by" = 'agent' AND "why" = 'stuck on a prompt'"#,
        vec![]
      )
      .await,
    1
  );
  assert!(refused(answer(db, "agent", "3", Verdict::Cut, None, NOW).await).contains("already over"));
  assert_eq!(refused(answer(db, "agent", "9", Verdict::Cut, None, NOW).await), "no ask #9");
  assert_eq!(refused(answer(db, "agent", "#1", Verdict::Cut, None, NOW).await), USAGE);
  assert_eq!(duration_seconds("1h"), Some(3600));
  assert_eq!(duration_seconds("90 s"), Some(90));
  assert_eq!(duration_seconds("15"), Some(900));
}

#[test]
fn runs_side_by_side_count_once_and_the_time_between_them_for_none() {
  let run = |from: f64, to: f64| Run { from, to };

  assert_eq!(run_seconds(&[]), None);
  assert_eq!(run_seconds(&[run(0.0, 10.0), run(5.0, 20.0), run(30.0, 40.0)]), Some(30.0));
  assert_eq!(run_seconds(&[run(10.0, 5.0)]), Some(0.0));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_work_order_s_time_is_its_runs_merged_with_the_builder_s_share_and_the_idle_between() {
  let desk = Desk::new().await;
  let order = json!({
    "todoId": 77, "status": "done",
    "history": [{ "id": "plan", "role": "builder", "jobId": 11, "startedAt": 0 }],
    "stages": [{ "title": "look", "tasks": [
      { "role": "scout", "jobId": 12, "startedAt": 600_000 },
      { "role": "scout", "jobId": 13, "startedAt": 660_000 },
    ] }],
    "tasks": [{ "title": "write", "role": "builder", "jobId": 14, "startedAt": 3_600_000 }],
  });

  for (id, launched, finished) in [(11, 0, 300), (12, 600, 900), (13, 660, 960), (14, 3600, 4200)] {
    desk
      .run(
        r#"INSERT INTO "jobs" ("id", "chat_id", "unit", "launched_at", "finished_at", "prompt", "todo_id", "worker", "ok")
           VALUES ($1, 1, $2, $3, $4, 'work', 77, 'worker', 1)"#,
        vec![id.into(), format!("bmo-work-77-{id}").into(), (launched * 1000).into(), (finished * 1000).into()],
      )
      .await;
  }

  desk
    .run(
      r#"INSERT INTO "work_orders" ("todo_id", "body") VALUES (77, $1)"#,
      vec![order.to_string().into()],
    )
    .await;
  desk
    .run(
      r#"INSERT INTO "job_spans" ("id", "job_id", "kind", "name", "seconds", "output_tokens")
         VALUES (1, 14, 'tool', 'Bash', 120, NULL), (2, 14, 'usage', 'turn', 0, 4000)"#,
      vec![],
    )
    .await;

  let lines = cowork_timing(&desk.stand, Orders::Items(vec![77]), true).await.unwrap();

  assert!(
    lines[0]
      .starts_with("#77 done · total 21m · builder 15m (71%, 2 turns, tools 2m, 4k out) · branches 6m"),
    "{lines:?}"
  );
  assert!(lines[0].contains("span 70m · idle 49m · pool wait 0s"), "{lines:?}");
  assert_eq!(
    lines[1..],
    ["  B   300s ×1 plan", "  -   360s ×2 stage 1: look", "  B   600s ×1 build: write"]
  );
  assert!(refused(cowork_timing(&desk.stand, Orders::Items(vec![]), false).await).starts_with("usage:"));
}
