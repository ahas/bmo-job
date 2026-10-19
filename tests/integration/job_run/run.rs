//! A job's run as a whole (docs/runs.md): the session its last answer came back on, recorded and handed over
//! or dropped as bmo-agency keeps it; where its report goes; the preamble it is told first; where its time went;
//! and the walk that refuses a co-work branch past the pool or ends one on its own road, and a follow-up that
//! did not answer.

use std::path::PathBuf;

use bmo_job::job_run::{WORK_SESSION_FRESH, WORK_STEP_PHASE};
use bmo_job::queue::outcomes::{JOB_DONE, JOB_ERROR, counts_as_attempt};
use job_agency::{TurnFailure, TurnSpan};
use queue_read::time::RUN_PHASE;
use queue_read::worker_lines::CHANGE_NOTE;
use serde_json::json;

use super::pooled_branch;

use crate::support::job::{Bench, Given, Ran, UNIT, WORKER};
use crate::support::queue::OWNER;
use crate::support::turns::{Agent, KeptAsked, OPUS, Played, reply, spend, tool_span};

const REPORT: &str = "Owner, the work is done.\n\nWhat changed\nThe file was fixed.\n\n1 done";

// The room that asked for a job, other than the owner's own.
const ROOM: i64 = 7;

async fn bench() -> (Bench, PathBuf) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  (bench, dir)
}

// A job asked for in a room of its own whose run answered on the Claude model and on the safeguard's sibling,
// which is what a fallback looks like from here; bmo-agency dropped the session as the turn answered.
async fn lesser_model() -> (Bench, Ran) {
  let (bench, dir) = bench().await;
  let job = bench.job_in(ROOM, None, UNIT, WORKER, OPUS).await;
  let mut fell = reply("poisoned-session", REPORT);

  fell.spend = vec![spend(OPUS, 120_000), spend("claude-opus-4-8", 40_000)];
  Agent::set(&bench.agent.kept, false);
  bench.agent.play([Played::answered(fell)]);

  let ran = bench
    .run(&Given {
      chat: ROOM,
      ..bench.given(job, &dir)
    })
    .await;

  (bench, ran)
}

#[tokio::test]
async fn a_session_that_fell_to_a_lesser_model_is_dropped_and_never_handed_over() {
  let (bench, ran) = lesser_model().await;

  assert!(ran.success, "{ran:?}");
  assert_eq!(bench.agent.kept_asked().len(), 1);
  // Nor is a note of the refused context carried to the next job.
  assert!(bench.agent.handed().is_empty());
}

// bmo-agency judges the fall from the turn it carried; the job asks its record whether the session it answered
// on still stands for the model the job went out on.
#[tokio::test]
async fn bmo_agency_s_record_is_asked_of_the_session_on_the_model_the_job_went_out_on() {
  let (bench, _) = lesser_model().await;

  assert_eq!(
    bench.agent.kept_asked(),
    [KeptAsked {
      model: crate::support::turns::MAPPED.into(),
      session: "poisoned-session".into(),
    }]
  );
}

#[tokio::test]
async fn the_job_s_own_ending_is_what_the_work_was_and_no_attempt_is_spent_on_the_swap() {
  let (_, ran) = lesser_model().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(!counts_as_attempt(ran.outcome.as_deref()));
}

#[tokio::test]
async fn the_report_the_job_wrote_still_goes_out() {
  let (bench, _) = lesser_model().await;

  assert!(bench.told().contains("Owner, the work is done."), "{}", bench.told());
}

// The job was asked for in a room other than the owner's private chat: the report is carried to the agent in
// the owner's private chat to tell, and the room is handed one voiced line made of its summary.
#[tokio::test]
async fn the_report_goes_to_the_owner_s_chat_while_the_room_that_asked_hears_one_line_of_it() {
  let (bench, _) = lesser_model().await;
  let room = bench.told_in(ROOM);

  assert!(
    bench.told_in(OWNER).iter().any(|said| said.contains("The file was fixed.")),
    "{:?}",
    bench.told_in(OWNER)
  );
  assert_eq!(room.len(), 1, "{room:?}");
  assert!(room[0].contains("Owner, the work is done."), "{}", room[0]);
  assert!(!room[0].contains("The file was fixed."), "{}", room[0]);
}

// The small model the program runs for its own housekeeping is another family and no fallback.
async fn warm() -> (Bench, Ran, PathBuf) {
  let (bench, dir) = bench().await;
  let job = bench.job(None, WORKER, UNIT).await;
  let mut answered = reply("warm-session", REPORT);

  answered.spend = vec![spend(OPUS, 120_000), spend("claude-haiku-4-5-20251001", 2_000)];
  bench.agent.play([Played::answered(answered)]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, ran, dir)
}

#[tokio::test]
async fn a_session_that_answered_on_its_model_is_recorded_and_handed_over_as_every_job_s_is() {
  let (bench, ran, _dir) = warm().await;

  assert!(ran.success, "{ran:?}");
  assert_eq!(bench.agent.kept_asked()[0].session, "warm-session");
  assert_eq!(bench.agent.handed()[0].session.as_deref(), Some("warm-session"));
  assert_eq!(bench.agent.handed()[0].note, None);
}

#[tokio::test]
async fn and_nothing_is_said_about_a_fall_that_did_not_happen() {
  let (bench, _, _dir) = warm().await;

  assert!(!bench.told().contains("claude-haiku"), "{}", bench.told());
  assert!(!bench.told().to_lowercase().contains("downgrade"), "{}", bench.told());
}

// The runner decides nothing about where its report goes: it leaves the report on its row for the telling,
// which delivers it by the rules live when it does.
#[tokio::test]
async fn the_runner_hands_its_report_to_the_telling_rather_than_sending_it_itself() {
  let (bench, ran, _dir) = warm().await;
  let carrying: Vec<_> = bench
    .scene
    .units
    .instructions()
    .into_iter()
    .filter(|instruction| {
      serde_json::to_string(&instruction.said).unwrap().contains("Owner, the work is done.")
    })
    .collect();

  assert!(ran.report.contains("Owner, the work is done."), "{}", ran.report);
  // One fact of the job's ending carries it, to the owner's chat; no order of the runner's own carries it.
  assert_eq!(carrying.len(), 1, "{carrying:?}");
  assert_eq!(carrying[0].chat, Some(OWNER));
  assert!(matches!(carrying[0].said, job_receiver::Said::Fact(_)), "{:?}", carrying[0].said);
  // The telling took the report it was left, and none waits for its next sweep.
  assert_eq!(
    job_base::one(
      &bench.scene.db,
      r#"SELECT COUNT(*) AS "n" FROM "settings" WHERE "key" LIKE 'job_report_waiting:%'"#,
      vec![]
    )
    .await
    .unwrap()
    .unwrap()
    .try_get::<i64>("", "n")
    .unwrap(),
    0
  );
}

// A worker's preamble is read from its row and stands at the very front of what the job is told, ahead of the
// job's own words, which follow it unchanged.
#[tokio::test]
async fn a_job_is_told_its_worker_s_preamble_first_with_its_workbench_named_and_its_own_words_after() {
  let (bench, dir) = bench().await;
  let preamble = "The dev server already runs on port 5396. Do not raise another; only check it.";
  let task = "Fix the colour of the login screen's button.";

  bench
    .scene
    .run(
      r#"UPDATE "workers" SET "preamble" = $1 WHERE "name" = $2"#,
      vec![preamble.into(), WORKER.into()],
    )
    .await;

  let job = bench.job(None, WORKER, UNIT).await;

  bench.agent.play([Played::answered(reply("bench-session", REPORT))]);

  let ran = bench
    .run(&Given {
      task: task.into(),
      ..bench.given(job, &dir)
    })
    .await;
  let told = &bench.agent.prompts()[0];

  assert!(ran.success, "{ran:?}");
  assert!(told.starts_with("[Worker preamble"), "{told}");
  assert!(told.contains(&std::env::temp_dir().join(WORKER).display().to_string()), "{told}");
  assert!(told.find(preamble).unwrap() < told.find(task).unwrap(), "{told}");
  assert!(bench.agent.upkeep().contains(&"make_bench".to_owned()));
}

// A run with something in it: two Bash calls, a co-worker, and the answer at the end. What is checked is that
// the breakdown was gathered and written, not how long a stand-in's tool took.
async fn timed() -> (Bench, i64, Ran) {
  let (bench, dir) = bench().await;

  bench.scene.item(7, "An item whose time is kept").await;

  let job = bench.job(Some(7), WORKER, UNIT).await;
  let scout = TurnSpan {
    kind: "agent".into(),
    ..tool_span("Task(scout) · where it is", 1)
  };

  bench.agent.play([Played::answered(reply("timed-session", REPORT))
    .doing(Box::new(|_| Box::pin(tokio::time::sleep(std::time::Duration::from_millis(20)))))
    .doing(crate::support::turns::closing(&bench.scene.db, 7))
    .spent(vec![tool_span("Bash", 2), scout])]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, job, ran)
}

async fn spans(bench: &Bench, job: i64) -> Vec<(String, String, f64, i64, Option<i64>)> {
  job_base::all(
    &bench.scene.db,
    r#"SELECT "kind", "name", "seconds", "times", "todo_id" FROM "job_spans" WHERE "job_id" = $1 ORDER BY "id""#,
    vec![job.into()],
  )
  .await
  .unwrap()
  .iter()
  .map(|row| {
    (
      row.try_get("", "kind").unwrap(),
      row.try_get("", "name").unwrap(),
      row.try_get("", "seconds").unwrap(),
      row.try_get("", "times").unwrap(),
      row.try_get("", "todo_id").unwrap(),
    )
  })
  .collect()
}

#[tokio::test]
async fn the_phases_of_the_job_are_its_own() {
  let (bench, job, ran) = timed().await;
  let spans = spans(&bench, job).await;
  let mut phases: Vec<&str> =
    spans.iter().filter(|span| span.0 == "phase").map(|span| span.1.as_str()).collect();

  phases.sort_unstable();

  let mut named = vec!["wait", "ready", RUN_PHASE, "finish"];

  named.sort_unstable();
  assert!(ran.success, "{ran:?}");
  assert_eq!(phases, named);
  assert!(spans.iter().any(|span| span.1 == RUN_PHASE && span.2 > 0.0), "{spans:?}");
}

#[tokio::test]
async fn they_cover_the_same_stretch_the_item_s_total_is_added_up_over() {
  let (bench, job, _) = timed().await;
  let whole: f64 = spans(&bench, job)
    .await
    .iter()
    .filter(|span| span.0 == "phase")
    .map(|span| span.2)
    .sum();
  let row = job_base::one(
    &bench.scene.db,
    r#"SELECT "launched_at", "finished_at" FROM "jobs" WHERE "id" = $1"#,
    vec![job.into()],
  )
  .await
  .unwrap()
  .unwrap();
  let took =
    row.try_get::<i64>("", "finished_at").unwrap() - row.try_get::<i64>("", "launched_at").unwrap();

  // The phases are cut at the row's two ends and measured in milliseconds, as the row keeps them.
  assert!((whole * 1000.0 - took as f64).abs() <= 2.0, "{whole} s against {took} ms");
}

#[tokio::test]
async fn under_the_phases_stands_what_the_run_itself_spent_the_time_on() {
  let (bench, job, _) = timed().await;
  let spans = spans(&bench, job).await;

  assert!(
    spans.iter().any(|span| span.0 == "tool" && span.1 == "Bash" && span.3 == 2),
    "{spans:?}"
  );
  assert!(
    spans.iter().any(|span| span.0 == "agent" && span.1 == "Task(scout) · where it is"),
    "{spans:?}"
  );
}

#[tokio::test]
async fn all_of_it_is_filed_under_the_item_the_job_was_a_run_of() {
  let (bench, job, _) = timed().await;
  let spans = spans(&bench, job).await;

  assert!(!spans.is_empty());
  assert!(spans.iter().all(|span| span.4 == Some(7)), "{spans:?}");
}

const BRANCH_UNIT: &str = "bmo-work-9-stage-0-0-1";
const HAIKU: &str = "claude-code::claude-haiku-4-5-20251001";

// A co-work branch of item 9's order on the scout's lease, which bmo-agency granted at the tier the task names.
async fn pooled() -> (Bench, PathBuf, i64) {
  let (bench, dir) = bench().await;
  let job = pooled_branch(&bench, 9, "claude-code", "claude-haiku-4-5-20251001", BRANCH_UNIT, WORKER).await;

  (bench, dir, job)
}

#[tokio::test]
async fn a_pooled_co_work_branch_ends_on_its_lease_s_road_rather_than_walking_to_another_provider() {
  let (bench, dir, job) = pooled().await;

  bench.agent.play([Played::failed(
    Some(TurnFailure::Limit {
      resets_at: None,
      account: None,
      silenced: false,
    }),
    None,
  )]);

  let ran = bench
    .run(&Given {
      model: HAIKU.into(),
      unit: BRANCH_UNIT.into(),
      ..bench.step(job, &dir, WORK_STEP_PHASE, WORK_SESSION_FRESH)
    })
    .await;
  let asked = bench.agent.asked();

  assert!(ran.ended(JOB_ERROR, false), "{ran:?}");
  // One attempt, on the lease's own mapping, and none on Codex.
  assert_eq!(asked.len(), 1, "{asked:?}");
  assert_eq!(asked[0].provider, "claude-code");
  assert_eq!(asked[0].mapping.model, "claude-haiku-4-5-20251001");
  assert_eq!(asked[0].mapping.effort.as_deref(), Some("medium"));
  assert_eq!(
    bench.setting(&format!("job_route_attempts:{job}")).await,
    Some(json!([HAIKU]).to_string())
  );
}

#[tokio::test]
async fn a_co_work_branch_with_no_durable_task_is_refused_before_a_turn_is_opened() {
  let (bench, dir) = bench().await;
  let unit = "bmo-work-999-stage-0-0-1";
  let job = bench.job_in(OWNER, None, unit, WORKER, HAIKU).await;
  let ran = bench
    .run(&Given {
      model: HAIKU.into(),
      unit: unit.into(),
      ..bench.step(job, &dir, WORK_STEP_PHASE, WORK_SESSION_FRESH)
    })
    .await;

  assert!(ran.ended(JOB_ERROR, false), "{ran:?}");
  assert!(bench.agent.asked().is_empty());
  assert!(ran.report.contains("no durable task"), "{}", ran.report);
}

// A follow-up over the agent's change that ran out of roads leaves the item's change undelivered and the earlier
// answer unsent, and the broken run is handed to the agent as an order to find and fix its cause.
#[tokio::test]
async fn a_failed_follow_up_stays_unfinished_with_its_note_pending_instead_of_sending_the_earlier_success()
{
  let mut bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  // Codex is shut, so the follow-up that ran out has no road left to walk to.
  bench.scene.units.providers.retain(|provider| provider.name == "claude-code");
  bench.worker(WORKER, &dir, 0).await;
  bench.scene.item(5, "A follow-up").await;

  let job = bench.job(Some(5), WORKER, UNIT).await;

  job_base::run(
    &bench.scene.db,
    r#"INSERT INTO "todo_notes" ("todo_id", "author", "text", "added_at", "kind") VALUES (5, 'Agent', 'An extra condition.', $1, $2)"#,
    vec![bmo_job::now_ms().into(), CHANGE_NOTE.into()],
  )
  .await
  .unwrap();
  bench.agent.play([
    Played::answered(reply("first", "EARLIER_SUCCESS_MUST_NOT_BE_SENT")),
    Played::failed(
      Some(TurnFailure::Limit {
        resets_at: None,
        account: None,
        silenced: false,
      }),
      None,
    ),
  ]);

  let ran = bench.run(&bench.given(job, &dir)).await;
  let pending = bmo_job::job_run::notes::undelivered(&bench.scene.db, 5, 0).await.unwrap();
  let order = bench
    .scene
    .units
    .instructions()
    .into_iter()
    .find(|instruction| instruction.kind.as_deref().is_some_and(|kind| kind.starts_with("failure-order:")))
    .expect("the broken run is ordered");
  let text = serde_json::to_string(&order.said).unwrap();

  assert!(ran.ended(JOB_ERROR, false), "{ran:?}");
  assert!(!bench.told().contains("EARLIER_SUCCESS_MUST_NOT_BE_SENT"), "{}", bench.told());
  assert_eq!(pending.len(), 1);
  assert_eq!(order.chat, Some(OWNER));
  assert!(text.contains(&format!("job {job}")), "{text}");
  assert!(text.contains(&format!("-u {UNIT} --since")), "{text}");
}
