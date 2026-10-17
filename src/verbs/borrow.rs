//! A checkout of another worker's repository borrowed for the rest of a job (docs/verbs.md, "Borrowing a
//! checkout"): a free support worker's worktree first, put on this job's item branch, and the base checkout
//! where no support worker is free, put on the job's branch too where its repository has support workers. It
//! is lent to this job until the job ends and its place printed; meanwhile it reads held wherever a held
//! checkout is read, and nothing is launched into it. When the job ends the runner lets it go.

use sea_orm::DatabaseConnection;

use super::stand::{RunningJob, Stand, VerbError, running_job};
use crate::job_run::checkout::{Checkout, item_branch, jobs_work_on_branches};
use crate::job_run::worktree::{record_baseline, worktree_state};
use crate::order_run::hold::SETUP_RUNTIME;
use crate::queue::job_start::lock_held;
use crate::queue::launches::{checkout_holders, family, leases, orders_holding};
use crate::queue::workers::{Worker, workers};
use crate::{now_ms, one, run};

fn refused(why: String) -> VerbError {
  VerbError::Ended {
    code: 1,
    said: format!("borrow: refused — {why}"),
  }
}

// The loan taken, where nobody holds this checkout: whether this job holds it now.
async fn take_lease(db: &DatabaseConnection, worker: &str, job_id: i64) -> Result<bool, VerbError> {
  run(
    db,
    r#"INSERT INTO "checkout_leases" ("worker", "job_id", "taken_at") VALUES ($1, $2, $3) ON CONFLICT DO NOTHING"#,
    vec![worker.into(), job_id.into(), now_ms().into()],
  )
  .await?;

  let held =
    one(db, r#"SELECT "job_id" FROM "checkout_leases" WHERE "worker" = $1"#, vec![worker.into()]).await?;

  Ok(held.map(|row| row.try_get::<i64>("", "job_id")).transpose()? == Some(job_id))
}

async fn release_lease(db: &DatabaseConnection, worker: &str, job_id: i64) -> Result<(), VerbError> {
  run(
    db,
    r#"DELETE FROM "checkout_leases" WHERE "worker" = $1 AND "job_id" = $2"#,
    vec![worker.into(), job_id.into()],
  )
  .await?;

  Ok(())
}

/// The named worker's checkout lent to the job running as `run`; the lines saying where it is and how its
/// work is closed.
pub async fn borrow(stand: &Stand, run: Option<&str>, asked: &str) -> Result<Vec<String>, VerbError> {
  let db = &stand.db;
  let job = match run {
    Some(run) => running_job(db, run).await?,
    None => None,
  };
  let Some(job) = job else {
    return Err(refused("a checkout is lent to a job, and this is not run inside one".into()));
  };
  let rules = &stand.policy.workers;
  let known = workers(db, &stand.ground.source).await?;
  let Some((base, members)) = family(&known, asked, rules) else {
    return Err(refused(format!("no worker is called {asked}")));
  };
  let own = job.worker.clone().unwrap_or_default();

  if family(&known, &own, rules).is_some_and(|(own, _)| own.name == base.name) {
    return Err(refused(format!(
      "{} is this job's own repository; work in this job's own checkout",
      base.name
    )));
  }

  let standing = leases(db).await?;

  if let Some(held) = standing
    .iter()
    .find(|lease| lease.job_id == job.id && members.iter().any(|member| member.name == lease.worker))
  {
    let at = members
      .iter()
      .find(|member| member.name == held.worker)
      .map_or("(unknown)", |member| member.cwd.as_str());

    return Ok(vec![format!("borrow: this job already has {}'s checkout: {at}", held.worker)]);
  }

  // Which jobs run is read off the user's service manager; where it gave no answer, no checkout can be said to
  // be free, and that is the refusal rather than every checkout being at work.
  let units = stand.ground.units();
  let Some(jobs) = stand.ground.running(db, &units).jobs().await? else {
    return Err(refused(
      "the service manager gave no answer about which jobs are running, so no checkout can be said to be free; try again in a moment"
        .into(),
    ));
  };
  // A support worker's worktree first, which lands through its own branch; the base checkout last, since it is
  // where the base worker's own queue runs.
  let order: Vec<&Worker> =
    members.iter().copied().filter(|member| member.is_support()).chain([base]).collect();
  let host = &stand.ground.host;

  for member in order {
    let busy = checkout_holders(member, &known, Some(&jobs), &standing)
      .is_none_or(|holders| !holders.is_empty())
      || !orders_holding(db, &member.cwd, None).await?.is_empty()
      || lock_held(host, &member.cwd);

    if busy || !take_lease(db, &member.name, job.id).await? {
      continue;
    }

    // A launch that took the lock in between has it; the loan is given back and the next is tried.
    if lock_held(host, &member.cwd) {
      release_lease(db, &member.name, job.id).await?;
      continue;
    }

    return lend(stand, &job, base, member).await;
  }

  let names: Vec<&str> = members.iter().map(|member| member.name.as_str()).collect();

  Err(refused(format!(
    "every checkout of {} is at work ({}); borrow once one is free",
    base.name,
    names.join(", ")
  )))
}

// The checkout put where the job works in it, and the lines saying so.
async fn lend(
  stand: &Stand,
  job: &RunningJob,
  base: &Worker,
  member: &Worker,
) -> Result<Vec<String>, VerbError> {
  let db = &stand.db;
  let here = Checkout::local(member.cwd.as_str());
  // A support worker's worktree, or a worker of its own standing in a linked worktree, which reads main off
  // its repository's main worktree.
  let apart_from = if member.is_support() { Some(base.cwd.clone()) } else { here.main_checkout() };
  let branch = item_branch(job.todo_id, job.id);
  let parked = format!("Parked by job {} borrowing {}'s checkout for {branch}", job.id, member.name);
  // Text the job reads names the deploy tree's command line, never the release it runs on.
  let land = stand.ground.tree.join("bin").join("bmo");
  let closing = |main: &str, place: &str| {
    format!(
      "Work there, commit on {branch} without pushing it, and close it there the way {}'s repository closes a job: this repository's close is its job close, which lands the branch on {main}; elsewhere land it with {} job land run in that {place}.",
      base.name,
      land.display()
    )
  };

  if let Some(apart_from) = apart_from {
    let main = Checkout::local(apart_from.as_str()).main_branch();
    let put = (|| {
      if member.is_support() {
        Checkout::local(base.cwd.as_str()).ensure_support_checkout(&member.cwd, SETUP_RUNTIME)?;
      }

      here.prepare_branch(&branch, &main, &parked, SETUP_RUNTIME).map(drop)
    })();

    if let Err(why) = put {
      release_lease(db, &member.name, job.id).await?;

      return Err(refused(format!("{}'s worktree could not be put on {branch}: {why}", member.name)));
    }

    return Ok(vec![
      format!(
        "borrow: {} is this job's until it ends — {}'s worktree, on the branch {branch} made from {main}.",
        member.cwd, member.name
      ),
      closing(&main, "worktree"),
    ]);
  }

  if jobs_work_on_branches(member.support_of.as_deref(), member.support_count) {
    // A base checkout beside support workers is put on the job's branch like one of them, so their landings
    // never wait on what this job opens there; what was already open is carried across and written down as
    // not this job's, and the runner puts it back on main as the job ends.
    let main = here.main_branch();
    let records = &stand.ground.documents;
    let put = here
      .prepare_branch(&branch, &main, &parked, SETUP_RUNTIME)
      .and_then(|_| worktree_state(&here))
      .map(|state| record_baseline(records, &member.cwd, &state, now_ms()));

    if let Err(why) = put {
      release_lease(db, &member.name, job.id).await?;

      return Err(refused(format!("{}'s checkout could not be put on {branch}: {why}", member.name)));
    }

    return Ok(vec![
      format!(
        "borrow: {} is this job's until it ends — {}'s own checkout, on the branch {branch} made from {main}; it goes back on {main} when the job ends.",
        member.cwd, member.name
      ),
      closing(&main, "checkout"),
    ]);
  }

  Ok(vec![
    format!(
      "borrow: {} is this job's until it ends — {}'s own checkout, on {}.",
      member.cwd,
      member.name,
      here.main_branch()
    ),
    "Work there, commit and push, and close it there the way that repository closes a job.".to_owned(),
  ])
}
