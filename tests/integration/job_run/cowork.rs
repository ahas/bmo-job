//! A work order's steps and the session each one goes on (docs/runs.md, "What the runner is handed" and "The
//! turns"): the worker's own steps on the worker's seat, each recording the session it answered on for the next;
//! the model a step went out on written on its row as bmo-agency reports it; a branch beside them on a session it
//! keeps none of; a stopped stage's retry handing the kept session over first; and its last retry on the retry
//! model.

use std::path::PathBuf;

use bmo_job::job_run::{WORK_SESSION_FRESH, WORK_SESSION_HANDOVER, WORK_SESSION_RESUME, WORK_STEP_PHASE};
use bmo_job::queue::outcomes::JOB_WORK_STEP;
use job_agency::{Reply, Resume, TurnSeat, WentOut};

use super::pooled_branch;
use crate::support::job::{Bench, Given, Ran, WORKER};
use crate::support::turns::{MAPPED, OPUS, Played, reply, spend};

async fn bench() -> (Bench, PathBuf) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  (bench, dir)
}

// One step of item 9's order under `unit` on the session word given, over the turn given.
async fn step(bench: &Bench, dir: &std::path::Path, unit: &str, word: &str, played: Played) -> Ran {
  let job = bench.job(None, WORKER, unit).await;

  bench.agent.play([played]);
  bench
    .run(&Given {
      unit: unit.into(),
      ..bench.step(job, dir, WORK_STEP_PHASE, word)
    })
    .await
}

fn fixed(session: &str) -> Played {
  Played::answered(reply(session, "Fixed."))
}

// A co-work's implementation turns are the worker's own turns: each goes on the worker's seat and records the
// session it answered on, which the next one goes on.
async fn two_steps() -> (Bench, Ran, Ran) {
  let (bench, dir) = bench().await;
  let one = step(&bench, &dir, "bmo-work-9-build-0-1", WORK_SESSION_RESUME, fixed("work-session")).await;
  let two = step(&bench, &dir, "bmo-work-9-build-1-1", WORK_SESSION_RESUME, fixed("work-session")).await;

  (bench, one, two)
}

#[tokio::test]
async fn the_first_step_goes_on_the_worker_s_own_seat_and_leaves_its_record_to_bmo_agency() {
  let (bench, one, _) = two_steps().await;
  let asked = &bench.agent.asked()[0];

  assert!(one.ended(JOB_WORK_STEP, true), "{one:?}");
  assert_eq!(
    (&asked.seat, &asked.resume, asked.hears_words),
    (&TurnSeat::Worker, &Resume::Auto, true)
  );
  // A middle turn hands nothing over, so it asks nothing of the record bmo-agency keeps as its turn answers.
  assert!(bench.agent.kept_asked().is_empty());
  assert!(bench.agent.handed().is_empty());
}

#[tokio::test]
async fn the_step_after_it_goes_on_the_session_the_first_one_left_recorded() {
  let (bench, _, two) = two_steps().await;
  let asked = &bench.agent.asked()[1];

  assert!(two.ended(JOB_WORK_STEP, true), "{two:?}");
  assert_eq!((&asked.seat, &asked.resume), (&TurnSeat::Worker, &Resume::Auto));
  assert!(bench.agent.kept_asked().is_empty());
  assert!(bench.agent.handed().is_empty());
}

const FALLEN: &str = "claude-opus-4-8";

fn row_model(
  bench: &Bench,
  unit: &str,
) -> impl std::future::Future<Output = (Option<String>, Option<String>)> {
  let db = bench.scene.db.clone();
  let unit = unit.to_owned();

  async move {
    let row = job_base::one(
      &db,
      r#"SELECT "execution_model", "execution_effort" FROM "jobs" WHERE "unit" = $1"#,
      vec![unit.into()],
    )
    .await
    .unwrap()
    .unwrap();

    (
      row.try_get("", "execution_model").unwrap(),
      row.try_get("", "execution_effort").unwrap(),
    )
  }
}

// A worker whose heal the owner turned off keeps a session the safeguard refused, and bmo-agency sends a turn
// that resumes it out on the model that answered; a session opened afresh goes out on its own model again.
async fn on_a_fallen_session() -> (Bench, [Ran; 3]) {
  let (bench, dir) = bench().await;
  let answered = |spent: Vec<job_agency::ModelSpend>| Reply {
    spend: spent,
    ..reply("work-session", "Fixed.")
  };
  let one = step(
    &bench,
    &dir,
    "bmo-work-9-build-0-1",
    WORK_SESSION_RESUME,
    Played::answered(answered(vec![spend(FALLEN, 40), spend(OPUS, 10)])),
  )
  .await;
  let mut resumed = Played::answered(answered(vec![spend(FALLEN, 10)]));

  resumed.went_out = Some(WentOut {
    provider: "claude-code".into(),
    model: FALLEN.into(),
    effort: None,
    tier: Some("Frontier".into()),
  });

  let two = step(&bench, &dir, "bmo-work-9-build-1-1", WORK_SESSION_RESUME, resumed).await;
  let three = step(
    &bench,
    &dir,
    "bmo-work-9-build-2-1",
    WORK_SESSION_RESUME,
    Played::answered(Reply {
      session_id: Some("fresh-session".into()),
      ..answered(vec![spend(OPUS, 10)])
    }),
  )
  .await;

  (bench, [one, two, three])
}

#[tokio::test]
async fn the_fallen_step_goes_out_on_the_tier_s_model_and_bmo_agency_judges_the_fall_itself() {
  let (bench, [one, ..]) = on_a_fallen_session().await;

  assert!(one.success, "{one:?}");
  assert_eq!(bench.agent.asked()[0].mapping.model, MAPPED);
  assert!(bench.agent.kept_asked().is_empty());
}

#[tokio::test]
async fn the_next_step_on_that_session_is_written_down_on_the_model_bmo_agency_sent_it_out_on() {
  let (bench, [_, two, _]) = on_a_fallen_session().await;

  assert!(two.success, "{two:?}");
  assert_eq!(row_model(&bench, "bmo-work-9-build-1-1").await.0.as_deref(), Some(FALLEN));
}

#[tokio::test]
async fn a_session_opened_afresh_goes_out_on_the_tier_s_model_again() {
  let (bench, [_, _, three]) = on_a_fallen_session().await;

  assert!(three.success, "{three:?}");
  assert_eq!(row_model(&bench, "bmo-work-9-build-2-1").await.0.as_deref(), Some(MAPPED));
}

// A scout or a verifier is a branch beside the worker's conversation: it opens a session it keeps none of, and
// the worker's own record is not touched by it.
async fn branch_beside() -> (Bench, Ran) {
  let (bench, dir) = bench().await;

  step(&bench, &dir, "bmo-work-9-build-0-1", WORK_SESSION_RESUME, fixed("work-session")).await;

  let unit = "bmo-work-4-stage-0-0-1";
  let job = pooled_branch(&bench, 4, "claude-code", "claude-haiku-4-5-20251001", unit, WORKER).await;

  bench.agent.play([Played::answered(reply("branch-session", "Read it."))]);

  let ran = bench
    .run(&Given {
      model: "claude-code::claude-haiku-4-5-20251001".into(),
      unit: unit.into(),
      ..bench.step(job, &dir, WORK_STEP_PHASE, WORK_SESSION_FRESH)
    })
    .await;

  (bench, ran)
}

#[tokio::test]
async fn a_branch_step_goes_on_a_seat_of_its_own_even_though_the_worker_has_a_warm_session() {
  let (bench, ran) = branch_beside().await;
  let asked = &bench.agent.asked()[1];

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  assert!(matches!(asked.seat, TurnSeat::Branch { .. }), "{asked:?}");
  assert!(!asked.hears_words);
}

#[tokio::test]
async fn a_branch_step_keeps_nothing_so_the_worker_s_session_is_where_the_builder_left_it() {
  let (bench, _) = branch_beside().await;

  assert!(bench.agent.kept_asked().is_empty());
  assert!(bench.agent.handed().is_empty());
}

// A stopped stage retried a second time runs on a fresh session: the worker's session it would have resumed is
// handed over through a summary first, once, and the turn opens a new one with the note ahead of its prompt.
async fn retried_fresh() -> (Bench, Ran) {
  let (bench, dir) = bench().await;

  step(&bench, &dir, "bmo-work-9-build-0-1", WORK_SESSION_RESUME, fixed("work-session")).await;

  let ran = step(&bench, &dir, "bmo-work-9-build-1-2", WORK_SESSION_HANDOVER, fixed("new-session")).await;

  (bench, ran)
}

#[tokio::test]
async fn a_retried_stage_hands_the_worker_s_session_over_once_before_its_turn() {
  let (bench, ran) = retried_fresh().await;
  let handed = bench.agent.handed();
  let upkeep = bench.agent.upkeep();
  let prepared: Vec<usize> = upkeep
    .iter()
    .enumerate()
    .filter(|(_, call)| *call == "prepare_turn")
    .map(|(at, _)| at)
    .collect();
  let handed_at = upkeep.iter().position(|call| call == "hand_over_session").unwrap();

  assert!(ran.success, "{ran:?}");
  assert_eq!(handed.len(), 1);
  assert_eq!((handed[0].session.as_deref(), handed[0].note.as_deref()), (None, None));
  assert!(prepared[0] < handed_at && handed_at < prepared[1], "{upkeep:?}");
}

#[tokio::test]
async fn and_its_turn_goes_on_a_session_of_its_own_rather_than_the_one_it_handed_over() {
  let (bench, ran) = retried_fresh().await;
  let handed = Resume::Session {
    id: "work-session".into(),
  };

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  assert_ne!(bench.agent.asked()[1].resume, handed);
}

const RETRY: &str = "claude-opus-4-8";

// A stopped stage's last retry runs on the retry model on the Claude road, at the effort its tier gives, in place
// of the model the tier names.
async fn on_the_retry_model() -> (Bench, Ran) {
  let (bench, dir) = bench().await;

  step(&bench, &dir, "bmo-work-9-build-0-1", WORK_SESSION_RESUME, fixed("work-session")).await;

  let job = bench.job(None, WORKER, "bmo-work-9-build-1-4").await;

  bench.agent.play([Played::answered(Reply {
    spend: vec![spend(RETRY, 10)],
    ..reply("retry-session", "Fixed.")
  })]);

  let ran = bench
    .run(&Given {
      unit: "bmo-work-9-build-1-4".into(),
      retry_model: RETRY.into(),
      ..bench.step(job, &dir, WORK_STEP_PHASE, WORK_SESSION_RESUME)
    })
    .await;

  (bench, ran)
}

#[tokio::test]
async fn the_retried_turn_goes_out_on_the_retry_model() {
  let (bench, ran) = on_the_retry_model().await;
  let asked = &bench.agent.asked()[1];

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  assert_eq!((asked.model.as_str(), asked.mapping.model.as_str()), (RETRY, RETRY));
}

#[tokio::test]
async fn the_job_row_names_the_retry_model_at_the_effort_the_tier_gave_the_plain_step() {
  let (bench, _) = on_the_retry_model().await;
  let (plain, retried) = (
    row_model(&bench, "bmo-work-9-build-0-1").await,
    row_model(&bench, "bmo-work-9-build-1-4").await,
  );

  assert_eq!(retried.0.as_deref(), Some(RETRY));
  assert_eq!(retried.1, plain.1);
}
