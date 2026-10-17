//! A job's run whole (docs/runs.md), each case a job run from its words to its ending on a database, a git
//! checkout and units of its own, its turns played by the stand-in for bmo-agency: the checkout before and
//! after, how a last answer is judged, the turns asked again over what a job left open or reached, the prompt,
//! the preamble and the trailer, a work order's steps and branches, and what the run asks of the worker's
//! sessions.

mod branch;
mod branch_roads;
mod claude;
mod claude_turns;
mod closing;
mod cowork;
mod endings;
mod frame;
mod graph;
mod host_restart;
mod live;
mod plans;
mod preamble;
mod prompt;
mod run;

use std::path::{Path, PathBuf};

use job_agency::{PoolAsking, PoolRequest, PoolState};
use serde_json::json;

use crate::support::job::{Bench, Ran, WORKER};
use crate::support::queue::OWNER;
use crate::support::turns::Played;

/// A worker in a checkout of its own with no support workers, the checkout as `before` leaves it, and a job on
/// it run over the turns given in that checkout.
pub async fn on_checkout(
  before: impl FnOnce(&Path),
  plays: impl FnOnce(&Path) -> Vec<Played>,
) -> (Bench, PathBuf, Ran) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  before(&dir);

  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  bench.agent.play(plays(&dir));

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, dir, ran)
}

/// A co-work branch of item `todo`'s order on the scout's lease, which bmo-agency granted on the provider's model
/// at the tier the task names, its job run under `unit` by `named`, the name its task alone is given; the job's
/// number.
pub async fn pooled_branch(
  bench: &Bench,
  todo: i64,
  provider: &str,
  model: &str,
  unit: &str,
  named: &str,
) -> i64 {
  let selection = format!("{provider}::{model}");

  bench.scene.item(todo, "A pooled branch").await;
  bench.scene.units.requests.lock().unwrap().push(PoolRequest {
    asking: PoolAsking {
      request_key: format!("work-order:{todo}:stage-0-0:pool-1"),
      owner: format!("work-order:{todo}"),
      source: "cowork".into(),
      role: "scout".into(),
      work_key: "role:scout".into(),
      provider: provider.into(),
      model: model.into(),
      effort: Some("medium".into()),
      tier: "Light".into(),
      yielding: false,
      place_key: None,
      turn: false,
      reservation: None,
    },
    token: "token-1".into(),
    state: PoolState::Active,
    enqueued_at: 1,
    leased_at: Some(1),
    child: Some(unit.into()),
    ended_at: None,
    outcome: None,
  });

  let job = bench.job_in(OWNER, Some(todo), unit, named, &selection).await;
  let mut order = json!({
    "todoId": todo,
    "status": "active",
    "phase": "research",
    "stage": 0,
    "chatId": OWNER,
    "worker": WORKER,
    "model": selection,
    "stages": [{ "title": "read", "tasks": [{
      "id": "stage-0-0", "role": "scout", "model": selection, "instruction": "Read one angle.", "state": "running",
      "attempt": 1, "unit": unit, "tier": "Light", "poolRequest": "token-1", "jobId": job,
    }] }],
  });

  queue_rows::work_orders::save(&bench.scene.db, &mut order).await.unwrap();
  job
}
