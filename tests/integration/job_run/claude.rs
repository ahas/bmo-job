//! A job's turn on the Claude road as far as the session it is put into (docs/runs.md, "The turns"; docs/api.md,
//! "Work turns"): the session bmo-agency holds for the worker resumed and handed over while its record still
//! stands, one the program no longer holds opened again fresh, one bmo-agency dropped for coming back on a lesser
//! model handed over to nobody, and a login whose window is full rested until it resets.

use std::path::PathBuf;

use bmo_job::queue::outcomes::JOB_DONE;
use job_agency::{Reply, Resume, TurnFailure};

use crate::support::job::{Bench, Ran, WORKER};
use crate::support::turns::{Agent, FINISHED, OPUS, Played, reply, spend};

// A job on a worker in a checkout of its own over the turns given, with the agent set up first.
async fn claude_run(set: impl FnOnce(&Agent), plays: Vec<Played>) -> (Bench, Ran, PathBuf) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  set(&bench.agent);

  let job = bench.job(None, WORKER, "bmo-task-twin").await;

  bench.agent.play(plays);

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, ran, dir)
}

#[tokio::test]
async fn the_session_on_record_is_resumed_and_handed_over_while_it_still_stands() {
  let (bench, ran, _dir) = claude_run(|_| {}, vec![Played::answered(reply("s0", FINISHED))]).await;
  let asked = bench.agent.asked();

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  // The first turn goes on whatever session bmo-agency holds for the worker.
  assert_eq!(asked[0].resume, Resume::Auto);
  assert!(bench.agent.prompts()[0].contains("There is one thing to do in this repository."));
  assert_eq!(bench.agent.kept_asked()[0].session, "s0");
  assert_eq!(bench.agent.handed()[0].session.as_deref(), Some("s0"));
}

#[tokio::test]
async fn a_session_the_program_no_longer_holds_is_opened_again_fresh() {
  let (bench, ran, _dir) = claude_run(
    |_| {},
    vec![
      Played::failed(None, Some("No conversation found with session ID s0")),
      Played::answered(reply("s1", FINISHED)),
    ],
  )
  .await;
  let asked = bench.agent.asked();

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(asked.len(), 2);
  assert_eq!(asked[1].resume, Resume::Fresh);
  assert_eq!(asked[1].provider, asked[0].provider);
  assert_eq!(bench.agent.kept_asked()[0].session, "s1");
}

// bmo-agency judges a session that came back on a lesser model and drops it as the turn answers, telling the
// owner; the job reads its record and hands nothing over.
#[tokio::test]
async fn a_session_that_came_back_on_a_lesser_model_is_dropped_and_not_handed_over() {
  let fell = Reply {
    spend: vec![spend(OPUS, 120_000), spend("claude-opus-4-8", 40_000)],
    ..reply("s0", FINISHED)
  };
  let (bench, ran, _dir) =
    claude_run(|agent| Agent::set(&agent.kept, false), vec![Played::answered(fell)]).await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(bench.agent.kept_asked()[0].session, "s0");
  assert!(bench.agent.handed().is_empty());
}

// Which login a turn goes out on is bmo-agency's; a window gone full on one rests that login until the moment the
// window resets, and the job walks on to the next road.
#[tokio::test]
async fn a_login_whose_window_is_full_is_rested_until_it_resets_and_the_job_goes_on_elsewhere() {
  let codex = Reply {
    mode: "codex".into(),
    model: Some("gpt-5.6-terra".into()),
    spend: vec![],
    ..reply("thread-1", FINISHED)
  };
  let (bench, ran, _dir) = claude_run(
    |_| {},
    vec![
      Played::failed(
        Some(TurnFailure::Limit {
          resets_at: Some(1_891_430_000),
          account: Some("first".into()),
          silenced: false,
        }),
        None,
      ),
      Played::answered(codex),
    ],
  )
  .await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(bench.agent.rests(), [("first".to_owned(), 1_891_430_000_000)]);
  assert_eq!(bench.agent.asked()[1].provider, "codex");
}
