//! A turn that answered with something still running, and a job stopped mid-turn (docs/runs.md, "How a job
//! ends" and "The turns"): a work order's middle turn is judged by whether it produced its result and never by
//! the final turn's check of what it left running, a planning step is still asked for its shape, a job that
//! left its dev server up is asked once to settle it, and a stop is the stop rather than the work failing.

use std::path::Path;

use bmo_job::job_run::{WORK_SESSION_RESUME, WORK_STEP_PHASE};
use bmo_job::queue::outcomes::{JOB_DONE, JOB_LIVE_TASKS, JOB_WORK_STEP, RUN_STOPPED, counts_as_attempt};
use job_agency::Resume;

use super::frame::asked_again;
use crate::support::job::{Bench, Ran, WORKER};
use crate::support::turns::{
  DESIGN, FINISHED, PLAN, Played, TRAILER, reply, with_live_task, with_workflow,
};

const MID_FLIGHT: &str = "Owner, two branches are running. I will report once the build branch is done.";
const NO_RESULT: &str = "The step produced no result";

// A job on a worker in a checkout of its own, run on the words `given` makes of the bench's, over the turns.
async fn run(
  bench: &Bench,
  plays: Vec<Played>,
  given: impl FnOnce(&Bench, i64, &Path) -> crate::support::job::Given,
) -> Ran {
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;

  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  bench.agent.play(plays);
  bench.run(&given(bench, job, &dir)).await
}

async fn step(plays: Vec<Played>) -> (Bench, Ran) {
  let bench = Bench::new().await;
  let ran = run(&bench, plays, |bench, job, dir| {
    bench.step(job, dir, WORK_STEP_PHASE, WORK_SESSION_RESUME)
  })
  .await;

  (bench, ran)
}

// A middle turn's own words are the artifact the next stage reads, so a failure must say why rather than read
// like a finished turn.
async fn step_walked_away() -> Ran {
  step(vec![Played::answered(with_workflow(reply("s1", MID_FLIGHT), Some("stopped")))])
    .await
    .1
}

#[tokio::test]
async fn a_work_step_that_walked_away_from_a_workflow_does_not_pass_and_its_record_says_why() {
  let ran = step_walked_away().await;

  assert!(!ran.success && ran.ok == Some(0), "{ran:?}");
  // The reason stands where a reader looks first, and it names the thing that was left.
  assert!(ran.report.contains("Workflows not finished: two-answers"), "{}", ran.report);
  // And the turn's own words are kept under it rather than thrown away.
  assert!(ran.report.contains("What the job left:"), "{}", ran.report);
  assert!(ran.report.contains(MID_FLIGHT), "{}", ran.report);
}

#[tokio::test]
async fn that_reason_is_not_the_one_a_step_which_reported_nothing_gets() {
  let ran = step_walked_away().await;

  assert!(!ran.report.contains(NO_RESULT), "{}", ran.report);
}

#[tokio::test]
async fn a_work_step_that_reported_nothing_gets_its_own_sentence_and_not_the_unfinished_one() {
  let (_, ran) = step(vec![Played::answered(reply("s1", "<<SKIP>>"))]).await;

  assert!(!ran.success, "{ran:?}");
  assert!(ran.report.contains(NO_RESULT), "{}", ran.report);
  assert!(!ran.report.contains("Left running"), "{}", ran.report);
  assert!(!ran.report.contains("Workflows not finished"), "{}", ran.report);
}

// What a middle turn left running is a command it did not collect; the unfinished-background check is the final
// turn's.
async fn step_left_running() -> Ran {
  step(vec![Played::answered(with_live_task(reply("s1", FINISHED)))]).await.1
}

#[tokio::test]
async fn a_work_step_that_left_a_background_task_running_passes() {
  let ran = step_left_running().await;

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
}

#[tokio::test]
async fn its_report_is_the_turn_s_own_words_untouched_since_that_is_the_next_stage_s_artifact() {
  let ran = step_left_running().await;

  assert_eq!(ran.report.trim(), FINISHED);
}

// An unshaped design recorded as a finished step is what a work order refuses, so a live task is no reason to
// skip the shaping.
#[tokio::test]
async fn a_planning_step_that_left_a_background_task_running_is_still_asked_for_its_shape() {
  let mut bench = Bench::new().await;

  bench.roster();

  let ran = run(
    &bench,
    vec![
      Played::answered(with_live_task(reply("s1", DESIGN))),
      Played::answered(reply("s1", PLAN)),
    ],
    |bench, job, dir| crate::support::job::Given {
      structure: "plan".into(),
      ..bench.step(job, dir, WORK_STEP_PHASE, WORK_SESSION_RESUME)
    },
  )
  .await;

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  // The shape was asked for and it is the shape that got recorded, not the design prose.
  assert!(bench.agent.prompts()[1].contains("[Shape the answer]"), "{:?}", bench.agent.prompts());
  assert_eq!(ran.report, PLAN);
}

const SETTLED: &str = "Owner, the console fix is done. I took down the dev server I raised to check it.\n\nWhat changed\nThe panel was fixed.\n\n1 done";

// A job that did its work and checked it in a browser leaves the dev server it raised running when it answers:
// the session is asked once to settle what it left, and the answer to that is judged.
async fn dev_server_left() -> (Bench, Ran) {
  let bench = Bench::new().await;
  let ran = run(
    &bench,
    vec![
      Played::answered(with_live_task(reply(
        "s1",
        "Owner, the console fix is done.\n\nWhat changed\nThe panel was fixed.\n\n1 done",
      ))),
      Played::answered(reply("s1", SETTLED)),
    ],
    |bench, job, dir| bench.given(job, dir),
  )
  .await;

  (bench, ran)
}

#[tokio::test]
async fn the_same_session_is_asked_once_to_settle_what_it_left_running_naming_it() {
  let (bench, ran) = dev_server_left().await;
  let prompts = bench.agent.prompts();

  assert!(ran.success, "{ran:?}");
  assert!(prompts[1].contains("Your answer was not sent"), "{}", prompts[1]);
  assert!(prompts[1].contains("seed the repository"), "{}", prompts[1]);
  assert_eq!(prompts.iter().map(|prompt| prompt.matches(TRAILER).count()).sum::<usize>(), 2);
  // The asking went on the session the first answer came back on.
  assert_eq!(bench.agent.asked()[1].resume, Resume::Session { id: "s1".into() });
}

#[tokio::test]
async fn the_answer_to_that_asking_is_the_report_recorded_as_a_job_that_worked() {
  let (_, ran) = dev_server_left().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(ran.report.contains("I took down the dev server"), "{}", ran.report);
}

#[tokio::test]
async fn a_job_that_leaves_work_running_again_when_asked_to_settle_it_is_refused_once_naming_what_it_left()
{
  let bench = Bench::new().await;
  let ran = run(
    &bench,
    vec![
      Played::answered(with_live_task(reply(
        "s1",
        "Owner, the tests are running.\n\nWhat changed\nFixed.\n\n1 done",
      ))),
      Played::answered(with_live_task(reply(
        "s1",
        "Owner, the tests still run.\n\nWhat changed\nFixed.\n\n1 done",
      ))),
    ],
    |bench, job, dir| bench.given(job, dir),
  )
  .await;

  assert!(ran.ended(JOB_LIVE_TASKS, false), "{ran:?}");
  assert!(ran.report.contains("Left running: seed the repository"), "{}", ran.report);
  assert_eq!(asked_again(&bench.agent), 1);
}

// The owner stopped the item while its job's turn was running: the turn comes back without an answer, and that
// ending is the stop rather than a provider that failed, so nothing is carried to another road and nothing goes
// out as a result.
async fn stopped_mid_turn() -> (Bench, Ran) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;

  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  bmo_job::queue::settings::set(&bench.scene.db, &format!("job_stop:{job}"), "stopped by the owner")
    .await
    .unwrap();
  bench.agent.play([Played::failed(None, None)]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, ran)
}

#[tokio::test]
async fn a_job_stopped_while_its_turn_ran_closes_as_stopped_rather_than_as_the_work_failing() {
  let (bench, ran) = stopped_mid_turn().await;

  assert!(ran.ended(RUN_STOPPED, false), "{ran:?}");
  assert!(!counts_as_attempt(Some(RUN_STOPPED)));
  // Nothing was carried to another road.
  assert_eq!(bench.agent.asked().len(), 1);
}

// The stop is the owner's own doing and they were already told of it, so no room is sent a report about work that
// was cut off.
#[tokio::test]
async fn the_stop_stays_on_the_row_rather_than_going_out_as_a_report() {
  let (bench, ran) = stopped_mid_turn().await;

  assert!(ran.report.contains("The run was stopped"), "{}", ran.report);
  assert!(!bench.told().contains("The run was stopped"), "{}", bench.told());
}
