//! A job's turns after its first (docs/runs.md, "The turns"): the agent's change to the item that no working
//! turn took, asked over in a round of its own; a work step's shape refused once, kept with its reason and asked
//! for again; and a stopped stage's retry that hands the worker's kept session over before its turn opens a
//! fresh one.

use bmo_job::job_run::{WORK_SESSION_HANDOVER, WORK_SESSION_RESUME, WORK_STEP_PHASE};
use bmo_job::queue::outcomes::{JOB_DONE, JOB_WORK_STEP};
use job_codes::WorkOrderRefusal;
use queue_read::worker_lines::CHANGE_NOTE;

use crate::support::job::{Bench, Given, WORKER};
use crate::support::turns::{DESIGN, FINISHED, MAPPED, PLAN, Played, closing, reply};

#[tokio::test]
async fn the_agent_s_change_that_no_working_turn_took_is_asked_over_in_a_round_of_its_own() {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  bench.scene.item(5, "An item with a change").await;

  let job = bench.job(Some(5), WORKER, "bmo-task-twin").await;

  // The turn answers the moment it is prompted, so the words reach no working turn.
  job_base::run(
    &bench.scene.db,
    r#"INSERT INTO "todo_notes" ("todo_id", "author", "text", "added_at", "kind") VALUES (5, 'Agent', 'Also keep the old flag.', $1, $2)"#,
    vec![bmo_job::now_ms().into(), CHANGE_NOTE.into()],
  )
  .await
  .unwrap();
  bench.agent.play([
    Played::answered(reply("s1", FINISHED)),
    Played::answered(reply("s1", FINISHED)).doing(closing(&bench.scene.db, 5)),
  ]);

  let ran = bench.run(&bench.given(job, &dir)).await;
  let delivered = job_base::one(&bench.scene.db, r#"SELECT "delivered_at" FROM "todo_notes""#, vec![])
    .await
    .unwrap()
    .unwrap()
    .try_get::<Option<i64>>("", "delivered_at")
    .unwrap();

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  // Each turn names its job and the item the job works, for bmo-agency to name behind the worker's place.
  assert!(
    bench.agent.asked().iter().all(|asked| asked.job.is_some() && asked.item == Some(5)),
    "{:?}",
    bench.agent.asked()
  );
  assert!(delivered.is_some());
  assert!(bench.agent.asides().is_empty());
  assert!(
    bench.agent.prompts()[1].contains("Your report has not been handed over"),
    "{}",
    bench.agent.prompts()[1]
  );
}

// The refusal is one the plan's check words itself, a role no plan may name.
#[tokio::test]
async fn a_work_step_s_shape_refused_once_is_kept_with_its_reason_and_asked_for_again() {
  let mut bench = Bench::new().await;

  bench.roster();

  let dir = bench.checkout("checkout");
  let refused = r#"{"stages":[{"title":"Investigate","tasks":[{"title":"look","role":"nobody","model":null,"instruction":"look around"}]}]}"#;

  bench.worker(WORKER, &dir, 0).await;

  let job = bench.job(None, WORKER, "bmo-task-twin").await;

  bench.agent.play([
    Played::answered(reply("s1", DESIGN)),
    Played::answered(reply("s1", refused)),
    Played::answered(reply("s1", PLAN)),
  ]);

  let ran = bench
    .run(&Given {
      structure: "plan".into(),
      ..bench.step(job, &dir, WORK_STEP_PHASE, WORK_SESSION_RESUME)
    })
    .await;
  let why = WorkOrderRefusal::PlanRoleWrong {
    roles: vec!["builder".into(), "scout".into()],
  }
  .to_string();
  let kept = bench.at("documents").join("jobs").join(format!("work-output-{job}"));

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  assert!(bench.agent.prompts()[2].contains("[JSON correction]"), "{}", bench.agent.prompts()[2]);
  assert!(bench.agent.prompts()[2].contains(&why), "{}", bench.agent.prompts()[2]);
  assert_eq!(std::fs::read_to_string(kept.join("0.error.txt")).unwrap(), why);
  assert!(
    bench
      .scene
      .units
      .instructions()
      .iter()
      .any(|instruction| serde_json::to_string(&instruction.said).unwrap().contains("work_output_refused")),
    "the refusal is told"
  );
}

#[tokio::test]
async fn a_stopped_stage_retried_on_a_fresh_session_hands_the_kept_session_over_before_its_turn() {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;

  let job = bench.job(None, WORKER, "bmo-work-9-build-1-2").await;

  bench.agent.play([Played::answered(reply("s1", "The step's result"))]);

  let ran = bench
    .run(&Given {
      unit: "bmo-work-9-build-1-2".into(),
      ..bench.step(job, &dir, WORK_STEP_PHASE, WORK_SESSION_HANDOVER)
    })
    .await;
  let upkeep = bench.agent.upkeep();
  let handed = bench.agent.handed();

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  // Handed over once, through a summary of the session bmo-agency keeps for the worker on the model.
  assert_eq!(handed.len(), 1);
  assert_eq!((handed[0].session.as_deref(), handed[0].note.as_deref()), (None, None));
  assert_eq!(handed[0].model, MAPPED);
  assert!(
    upkeep.iter().position(|call| call == "hand_over_session")
      < upkeep.iter().position(|call| call == "prepare_turn"),
    "{upkeep:?}"
  );
}
