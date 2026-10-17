//! A job's turn while something around it moves, on the registered road and on the Claude road (docs/runs.md,
//! "The turns"; docs/api.md, "Work turns"): bmo-agency preparing the turn again after its own restart, which the
//! job sends its prompt again for and runs through with no road closed; the owner stopping the job in the middle
//! of its turn, which ends it stopped and closes no road; and a registration a road failure closed for an outage
//! whose engine answers its health again, which the job goes out on.

use std::path::PathBuf;

use bmo_job::queue::outcomes::{JOB_DONE, RUN_STOPPED};
use job_agency::{Provider, Reply, TurnState};

use crate::support::job::{Bench, Given, Ran, WORKER};
use crate::support::turns::{Act, Agent, FINISHED, OPUS, Played, reply};

const REGISTRATION: &str = "qwen-test";
const REGISTERED: &str = "registered::qwen-test";
const UNIT: &str = "bmo-task-restart";

struct Seen {
  bench: Bench,
  ran: Ran,
  road: (Option<String>, Option<String>),
  blocks: Vec<String>,
}

// An answer on the registered road, which keeps its own session contract.
fn registered() -> Reply {
  Reply {
    mode: "registered".into(),
    model: Some(REGISTRATION.into()),
    session_id: None,
    spend: vec![],
    ..reply("unused", FINISHED)
  }
}

// One job on the selection given over the turn given, with a model registered beside the providers bmo-agency
// lists and, where `closed`, the registration's road closed for an outage.
async fn run(selection: &str, played: impl FnOnce(&Bench, i64) -> Played, closed: bool) -> Seen {
  let mut bench = Bench::new().await;
  let dir: PathBuf = bench.checkout("checkout");

  bench.scene.units.providers.push(Provider {
    name: "registered".into(),
    enabled: true,
    registered: true,
    coworker: false,
    lines: vec!["worker".into()],
  });
  bench.worker(WORKER, &dir, 0).await;

  if closed {
    let block = serde_json::json!({ "until": bmo_job::now_ms() + 600_000, "reason": "outage" });

    bmo_job::queue::settings::set(
      &bench.scene.db,
      &format!("job_route_block:{REGISTERED}"),
      &block.to_string(),
    )
    .await
    .unwrap();
  }

  let job = bench.job(None, WORKER, UNIT).await;

  bench.agent.play([played(&bench, job)]);

  let ran = bench
    .run(&Given {
      model: selection.into(),
      unit: UNIT.into(),
      ..bench.given(job, &dir)
    })
    .await;
  let row = job_base::one(
    &bench.scene.db,
    r#"SELECT "execution_mode", "execution_model" FROM "jobs" WHERE "id" = $1"#,
    vec![job.into()],
  )
  .await
  .unwrap()
  .unwrap();
  let road = (row.try_get("", "execution_mode").unwrap(), row.try_get("", "execution_model").unwrap());
  let blocks = job_base::all(
    &bench.scene.db,
    r#"SELECT "key" FROM "settings" WHERE "key" LIKE 'job_route_block:%' ORDER BY "key""#,
    vec![],
  )
  .await
  .unwrap()
  .iter()
  .map(|row| row.try_get("", "key").unwrap())
  .collect();

  Seen {
    bench,
    ran,
    road,
    blocks,
  }
}

// bmo-agency's host restarted just as the turn started: the turn comes back prepared again, is sent its prompt
// again on the session the new preparation settled, and answers.
async fn through_a_restart(selection: &str, answer: Reply, road: &str) {
  let seen = run(
    selection,
    |_, _| {
      let mut played = Played::answered(answer);

      played.states.insert(0, TurnState::Reprepared);
      played
    },
    false,
  )
  .await;
  let prompts = seen.bench.agent.prompts();

  assert!(seen.ran.ended(JOB_DONE, true), "{:?}", seen.ran);
  assert_eq!(seen.road.0.as_deref(), Some(road));
  assert!(seen.blocks.is_empty(), "{:?}", seen.blocks);
  // The prompt went in twice, once to each preparation, and the run log names the road the turn went out on.
  assert_eq!(prompts.len(), 2);
  assert_eq!(prompts[0], prompts[1]);
  assert!(
    seen.bench.agent.notes().iter().any(|note| note.starts_with(&format!("{road} · "))),
    "{:?}",
    seen.bench.agent.notes()
  );
}

#[tokio::test]
async fn on_the_registered_road_a_job_runs_through_bmo_agency_restarting_with_no_road_closed() {
  through_a_restart(REGISTERED, registered(), "registered").await;
}

#[tokio::test]
async fn on_the_claude_road_a_job_runs_through_bmo_agency_restarting_with_no_road_closed() {
  through_a_restart(OPUS, reply("s1", FINISHED), "claude-code").await;
}

#[tokio::test]
async fn a_job_on_the_registered_road_stopped_in_the_middle_of_its_turn_ends_stopped_and_leaves_its_registration_open()
 {
  let seen = run(
    REGISTERED,
    |bench, job| {
      let db = bench.scene.db.clone();
      // The owner stops the job while its turn works.
      let stop: Act = Box::new(move |_| {
        Box::pin(async move {
          bmo_job::queue::settings::set(&db, &format!("job_stop:{job}"), "stopped by the owner")
            .await
            .unwrap();
        })
      });

      Played::failed(None, None).doing(stop)
    },
    false,
  )
  .await;

  assert!(seen.ran.ended(RUN_STOPPED, false), "{:?}", seen.ran);
  assert!(seen.blocks.is_empty(), "{:?}", seen.blocks);
}

#[tokio::test]
async fn a_registration_closed_for_an_outage_whose_engine_answers_its_health_is_opened_again_and_taken() {
  let seen = run(
    REGISTERED,
    |bench, _| {
      Agent::set(&bench.agent.healthy, true);
      Played::answered(registered())
    },
    true,
  )
  .await;

  assert!(seen.ran.ended(JOB_DONE, true), "{:?}", seen.ran);
  assert_eq!(seen.road, (Some("registered".into()), Some(REGISTRATION.into())));
  assert!(seen.blocks.is_empty(), "{:?}", seen.blocks);
}
