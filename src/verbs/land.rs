//! `job land`: a job's branch landed on main, run in the checkout standing on the job's `work/` branch — a
//! support worker's worktree, or a base checkout during its job — once the job's own checks have passed
//! (docs/verbs.md, "Landing a branch"). It is the landing a repository without a close of its own that lands
//! uses: the branch rebased onto main, main moved to it and pushed, with no check of its own.

use std::path::Path;

use super::stand::{Answer, Stand, VerbError};
use crate::job_run::checkout::landing::{LandOptions, LandingHold, land_on_main};
use crate::job_run::checkout::units::{CheckoutUnits, TestbedLinks};
use crate::job_run::checkout::{Checkout, is_work_branch, resolved};
use crate::job_run::worktree::{preserved_work, recorded_baseline, worktree_state};
use crate::queue::workers::{Worker, workers};

fn refused(said: String) -> VerbError {
  VerbError::Ended { code: 1, said }
}

/// The main a job's branch in a checkout lands on: read off the base checkout the worker rows name for it,
/// since a support worktree stands on a `work/` branch, and a branch put on before its main was written
/// beside it reads the remote's head there. Any other linked worktree reads it off its repository's main
/// worktree, and a checkout that is neither reads its own.
pub fn landing_branch_of(here: &Checkout, workers: &[Worker]) -> String {
  let at = resolved(&here.cwd);
  let base = workers
    .iter()
    .find(|worker| resolved(&worker.cwd) == at)
    .filter(|worker| worker.is_support())
    .and_then(|worker| workers.iter().find(|base| Some(&base.name) == worker.support_of.as_ref()));
  let from = base.map(|base| base.cwd.clone()).or_else(|| here.main_checkout());

  from.map_or_else(|| here.main_branch(), |cwd| Checkout::local(cwd).main_branch())
}

/// Whether the repository a checkout belongs to has checkouts on work nodes, which push its main to the
/// remote themselves, so a landing here takes the remote's main in first.
pub fn pushed_from_nodes(here: &Checkout, workers: &[Worker]) -> bool {
  let main = resolved(&here.main_checkout().unwrap_or_else(|| here.cwd.clone()));
  let base = workers.iter().find(|worker| !worker.is_support() && resolved(&worker.cwd) == main);

  base.is_some_and(|base| {
    workers
      .iter()
      .any(|worker| worker.support_of.as_ref() == Some(&base.name) && worker.node.is_some())
  })
}

/// The branch the checkout at `here` stands on, landed on main.
pub async fn land(stand: &Stand, here: &Path) -> Result<Answer, VerbError> {
  let policy = &stand.policy;
  let said = &policy.landing.verb;
  let here = resolved(&here.to_string_lossy());
  let checkout = Checkout::local(here.clone());
  let branch = checkout.current_branch();

  if !is_work_branch(branch.as_deref()) {
    return Err(refused(policy.fill(
      &said.not_work_branch,
      &[("here", &here), ("branch", branch.as_deref().unwrap_or(&said.no_branch))],
    )));
  }

  // Main is the branch the base checkout stands on, read there rather than here, where a `work/` branch
  // stands. What a base checkout had open before its job began is somebody else's, set aside rather than
  // refused; a repository whose checkouts on nodes push main has it taken in first.
  let rows = workers(&stand.db, &stand.ground.source).await?;
  let main = landing_branch_of(&checkout, &rows);
  let records = &stand.ground.documents;
  let now = worktree_state(&checkout).map_err(refused)?;
  let others_open = preserved_work(&recorded_baseline(records, &here), &now).tracked;
  let refresh = || checkout.follow_remote_main(&main);
  let links = TestbedLinks(&stand.ground.testbed);
  let units = CheckoutUnits {
    links: &links,
    policy,
  };
  let mut hold = LandingHold::Here(stand.ground.host.locks.clone());
  let options = LandOptions {
    others_open: &others_open,
    refresh: pushed_from_nodes(&checkout, &rows).then_some(&refresh as &dyn Fn() -> Result<(), String>),
    units: Some(&units),
    ..LandOptions::default()
  };
  let landed = land_on_main(&checkout, policy, records, &main, options, &mut hold).await;

  match landed {
    Err(refusal) => Err(refused(policy.fill(&said.refused, &[("why", &refusal.text(policy, &main))]))),
    Ok(landed) => {
      let count = landed.landed.len().to_string();
      let what = match landed.landed.len() {
        0 => said.nothing.clone(),
        1 => said.one.clone(),
        _ => policy.fill(&said.many, &[("count", &count)]),
      };

      Ok(Answer::from(vec![policy.fill(
        &said.landed,
        &[
          ("commit", &landed.commit[..landed.commit.len().min(12)]),
          ("main", &main),
          ("landed", &what),
        ],
      )]))
    }
  }
}
