//! What a launch of a job is judged against (docs/queue.md, "Launching a job"): who may launch what, the
//! pacing of an item whose launches keep dying, which of a repository's checkouts a launch may take and
//! what holds one. Each refusal is a code of `TaskRefusal`.

use job_codes::task::CheckoutHolder;
use job_codes::{JobError, TaskRefusal};
use job_policy::workers::WorkerRules;
use queue_read::lines::BUILTIN_WORKER;
use queue_rows::todos::Item;
use sea_orm::ConnectionTrait;
use serde_json::Value;

use super::jobs::OpenJob;
use super::outcomes::counts_as_execution_failure;
use super::paths::resolve;
use super::settings;
use super::workers::Worker;
use crate::{Refusal, all, one, run};

/// Who set off a run of a work order, as its job row records it.
pub const WORK_ORDER_STARTED_BY: &str = "work-order";

/// How a chat session signs a launch: what follows is the session's own chat.
pub const CHAT_LAUNCHER: &str = "chat:";

/// The token a job's own text turns the branching on with.
pub const COWORK_SENTINEL: &str = "<<COWORK>>";

/// The setting a launch held for want of room on the machine is written to.
pub const MACHINE_LAUNCH_HELD_KEY: &str = "machine_launch_held";

/// The first wait an item earns by a launch that died, and the ceiling it doubles towards.
const RETRY_STEP_MS: i64 = 60_000;
const MOST_RETRY_MS: i64 = 15 * 60_000;

/// What a launch says of itself: who launched it, the item it is a run of, and why a job with no item is
/// made.
pub struct Launch<'a> {
  pub started_by: Option<&'a str>,
  pub todo_id: Option<i64>,
  pub purpose: Option<&'a str>,
}

/// Where a launch would land and what it would take, read at the moment of the launch.
pub struct Stand<'a> {
  pub worker: &'a Worker,
  /// That worker's line as it stands now, in the owner's order.
  pub queue: &'a [Item],
  /// The item the launch names, whatever it stands at.
  pub item: Option<&'a Item>,
  /// The worker that item is assigned to.
  pub item_worker: Option<&'a str>,
  /// The item's own runs, as their endings read: when each finished and how.
  pub runs: &'a [(Option<i64>, Option<String>)],
  pub now: i64,
}

/// One loan of a worker's checkout to a job.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lease {
  pub worker: String,
  pub job_id: i64,
}

fn refused(refusal: TaskRefusal) -> Refusal {
  JobError::from(refusal).into()
}

/// Whose work an item is: the worker its row names, its base where that is a support worker, and the
/// built-in worker where it names none or one since removed.
pub fn todo_worker<'a>(item: &Item, known: &'a [Worker]) -> &'a str {
  let named = item.worker.as_deref().and_then(|name| known.iter().find(|worker| worker.name == name));

  match named {
    None => BUILTIN_WORKER,
    Some(worker) => worker.base_name(),
  }
}

/// One worker's line: the items assigned to it, in the owner's own order.
pub fn queue_for<'a>(items: &'a [Item], worker: &str, known: &[Worker]) -> Vec<&'a Item> {
  items.iter().filter(|item| todo_worker(item, known) == worker).collect()
}

/// Whether one job runs as a co-work: the item's own flag, the launch naming it, or the sentinel in the
/// job's own text.
pub fn runs_as_cowork(item: Option<&Item>, asked: bool, task: &str) -> bool {
  item.is_some_and(|item| item.cowork) || asked || task.contains(COWORK_SENTINEL)
}

/// The checkouts one repository's jobs may be launched into: the base first, then each support worker
/// within the count by number.
pub fn family<'a>(
  workers: &'a [Worker],
  name: &str,
  rules: &WorkerRules,
) -> Option<(&'a Worker, Vec<&'a Worker>)> {
  let named = workers.iter().find(|worker| worker.name == name)?;
  let base = workers.iter().find(|worker| worker.name == named.base_name() && !worker.is_support())?;
  let mut supports: Vec<&Worker> = workers
    .iter()
    .filter(|worker| {
      worker.support_of.as_deref() == Some(base.name.as_str())
        && !worker.is_node_checkout()
        && worker.in_use(base, rules)
    })
    .collect();

  supports.sort_by_key(|worker| worker.support_slot.unwrap_or(0));

  Some((base, std::iter::once(base).chain(supports).collect()))
}

/// The order a launch of an item tries a family's checkouts in: a support worktree already on the item's
/// branch, then the base, then the support workers by number.
pub fn launch_order<'a>(members: &[&'a Worker], on_branch: impl Fn(&Worker) -> bool) -> Vec<&'a Worker> {
  let (continuing, rest): (Vec<&Worker>, Vec<&Worker>) =
    members.iter().copied().partition(|worker| worker.is_support() && on_branch(worker));

  continuing.into_iter().chain(rest).collect()
}

/// The branch a checkout stands on, or none where its HEAD is detached or it is no checkout.
pub fn current_branch(cwd: &str) -> Option<String> {
  let run = std::process::Command::new("git")
    .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
    .current_dir(cwd)
    .env("GIT_TERMINAL_PROMPT", "0")
    .stdin(std::process::Stdio::null())
    .output()
    .ok()?;
  let named = String::from_utf8_lossy(&run.stdout).trim().to_owned();

  (run.status.success() && !named.is_empty()).then_some(named)
}

/// The branch a job of an item works on in a support worktree.
pub fn item_branch(todo_id: i64) -> String {
  format!("work/{todo_id}")
}

/// What holds one checkout now: the jobs of its own worker or of any worker pointed at the same
/// directory, and the jobs that borrowed it. None where nobody could say what is running.
pub fn checkout_holders(
  worker: &Worker,
  known: &[Worker],
  jobs: Option<&[OpenJob]>,
  leases: &[Lease],
) -> Option<Vec<CheckoutHolder>> {
  let jobs = jobs?;
  let here = resolve(&worker.cwd);
  let directory_of = |name: &str| known.iter().find(|one| one.name == name).map(|one| resolve(&one.cwd));
  let holder = |job: &OpenJob, borrowed: bool| CheckoutHolder {
    worker: job.worker.clone(),
    job: job.id,
    item: job.todo_id,
    unit: job.unit.clone(),
    borrowed,
  };
  let mut holders: Vec<CheckoutHolder> = jobs
    .iter()
    .filter(|job| job.worker == worker.name || directory_of(&job.worker).as_ref() == Some(&here))
    .map(|job| holder(job, false))
    .collect();
  let lent: Vec<CheckoutHolder> = leases
    .iter()
    .filter(|lease| lease.worker == worker.name || directory_of(&lease.worker).as_ref() == Some(&here))
    .flat_map(|lease| jobs.iter().filter(move |job| job.id == lease.job_id))
    .filter(|job| !holders.iter().any(|held| held.job == job.id))
    .map(|job| holder(job, true))
    .collect();

  holders.extend(lent);

  Some(holders)
}

/// The refusal of a launch into a checkout something holds, where anything does.
pub fn checkout_held(worker: &Worker, holders: Option<&[CheckoutHolder]>) -> Result<(), Refusal> {
  match holders.filter(|holders| !holders.is_empty()) {
    Some(holders) => Err(refused(TaskRefusal::CheckoutHeld {
      cwd: worker.cwd.clone(),
      holders: holders.to_vec(),
    })),
    None => Ok(()),
  }
}

/// Who may start a job, and for what.
pub fn launch_refusal(launch: &Launch<'_>, stand: &Stand<'_>) -> Result<(), Refusal> {
  let by = launch.started_by.map(str::trim).unwrap_or_default();

  if by.is_empty() {
    return Err(refused(TaskRefusal::LaunchNeedsLauncher));
  }

  let purpose = launch.purpose.map(str::trim).unwrap_or_default();

  if let Some(item) = launch.todo_id
    && !purpose.is_empty()
  {
    return Err(refused(TaskRefusal::ItemLaunchTakesNoWhy { item }));
  }

  if purpose.contains(['\r', '\n']) {
    return Err(refused(TaskRefusal::WhyOneLine));
  }

  if !by.starts_with(CHAT_LAUNCHER) {
    if launch.todo_id.is_none() && purpose.is_empty() {
      return Err(refused(TaskRefusal::ItemlessNeedsWhy));
    }

    return Ok(());
  }

  item_refusal(launch.todo_id, stand)
}

fn item_refusal(todo_id: Option<i64>, stand: &Stand<'_>) -> Result<(), Refusal> {
  let Some(id) = todo_id else {
    return Err(refused(TaskRefusal::SessionLaunchesItem {
      worker: stand.worker.name.clone(),
      front: stand.queue.first().map(|front| front.id),
    }));
  };
  let Some(item) = stand.item else {
    return Err(refused(TaskRefusal::LaunchItemAbsent { item: id }));
  };
  let title = item.title.clone();

  if item.done {
    return Err(refused(TaskRefusal::LaunchItemDone { item: id, title }));
  }

  if let Some(into) = item.merged_into {
    return Err(refused(TaskRefusal::LaunchItemMerged {
      item: id,
      title,
      into,
    }));
  }

  if item.held_at.is_some() {
    return Err(refused(TaskRefusal::LaunchItemHeld {
      item: id,
      title,
      why: item.held_reason.clone(),
    }));
  }

  if !item.approved {
    return Err(refused(TaskRefusal::LaunchItemUnapproved { item: id, title }));
  }

  if let Some(owner) = stand.item_worker
    && owner != stand.worker.name
  {
    return Err(refused(TaskRefusal::LaunchItemOtherWorker {
      item: id,
      title,
      worker: owner.to_owned(),
      asked: stand.worker.name.clone(),
    }));
  }

  launch_wait(item, stand.runs, stand.now)
}

// The runs that ended, oldest first.
fn ended(runs: &[(Option<i64>, Option<String>)]) -> Vec<(i64, &str)> {
  runs
    .iter()
    .filter_map(|(finished, outcome)| Some(((*finished)?, outcome.as_deref()?)))
    .collect()
}

// How many of the item's runs, counting back from the last, failed to start.
fn failure_streak(runs: &[(Option<i64>, Option<String>)]) -> u32 {
  let streak = ended(runs)
    .iter()
    .rev()
    .take_while(|(_, outcome)| counts_as_execution_failure(Some(outcome)))
    .count();

  u32::try_from(streak).unwrap_or(u32::MAX)
}

/// The moment the item may be launched again, or none where it waits on nothing.
pub fn retry_at(runs: &[(Option<i64>, Option<String>)]) -> Option<i64> {
  let streak = failure_streak(runs);

  if streak == 0 {
    return None;
  }

  let (finished, _) = *ended(runs).last()?;
  let wait = RETRY_STEP_MS.saturating_mul(1_i64 << (streak - 1).min(32)).min(MOST_RETRY_MS);

  Some(finished + wait)
}

/// The pacing of an item whose launches keep dying before they reach the work.
pub fn launch_wait(item: &Item, runs: &[(Option<i64>, Option<String>)], now: i64) -> Result<(), Refusal> {
  match retry_at(runs).filter(|at| now < *at) {
    Some(at) => Err(refused(TaskRefusal::LaunchWaits {
      item: item.id,
      title: item.title.clone(),
      streak: failure_streak(runs),
      wait_ms: at - now,
    })),
    None => Ok(()),
  }
}

/// A launch held for want of room on the machine, written down at `now` with what was short, so the
/// queue's watch tells the agent the queue again once the room is back.
pub async fn note_held(db: &impl ConnectionTrait, held: &[String], now: i64) -> Result<(), Refusal> {
  settings::set(db, MACHINE_LAUNCH_HELD_KEY, &serde_json::json!({ "at": now, "held": held }).to_string())
    .await
}

/// An item's own runs as their endings read, the ones a work order set off left out.
pub async fn run_endings(
  db: &impl ConnectionTrait,
  todo_id: i64,
) -> Result<Vec<(Option<i64>, Option<String>)>, Refusal> {
  let rows = all(
    db,
    r#"SELECT "finished_at", "outcome" FROM "jobs" WHERE "todo_id" = $1 AND COALESCE("started_by", '') != $2 ORDER BY "id""#,
    vec![todo_id.into(), WORK_ORDER_STARTED_BY.into()],
  )
  .await?;

  rows
    .iter()
    .map(|row| Ok((row.try_get("", "finished_at")?, row.try_get("", "outcome")?)))
    .collect()
}

/// The loans standing of a worker's checkout to a job, a loan whose job has ended swept first.
pub async fn leases(db: &impl ConnectionTrait) -> Result<Vec<Lease>, Refusal> {
  run(
    db,
    r#"DELETE FROM "checkout_leases" WHERE "job_id" NOT IN (SELECT "id" FROM "jobs" WHERE "finished_at" IS NULL)"#,
    vec![],
  )
  .await?;

  let rows = all(
    db,
    r#"SELECT "worker", "job_id" FROM "checkout_leases" ORDER BY "taken_at", "worker""#,
    vec![],
  )
  .await?;

  rows
    .iter()
    .map(|row| {
      Ok(Lease {
        worker: row.try_get("", "worker")?,
        job_id: row.try_get("", "job_id")?,
      })
    })
    .collect()
}

/// A worker's checkout lent to a job at `now`, in place of any loan of it standing.
pub async fn lend(db: &impl ConnectionTrait, worker: &str, job_id: i64, now: i64) -> Result<(), Refusal> {
  run(
    db,
    r#"INSERT INTO "checkout_leases" ("worker", "job_id", "taken_at") VALUES ($1, $2, $3)
       ON CONFLICT ("worker") DO UPDATE SET "job_id" = excluded."job_id", "taken_at" = excluded."taken_at""#,
    vec![worker.into(), job_id.into(), now.into()],
  )
  .await?;

  Ok(())
}

/// The work orders holding a checkout, which no job is started in: every order under way, and a stopped
/// one still holding it, of any item but this one.
pub async fn orders_holding(
  db: &impl ConnectionTrait,
  cwd: &str,
  todo_id: Option<i64>,
) -> Result<Vec<i64>, Refusal> {
  let here = resolve(cwd);
  let mut holding = vec![];

  for order in super::turn::orders_in(db, &["active", "paused"]).await? {
    let status = order.get("status").and_then(Value::as_str);
    let holds = status == Some("active") || order.get("holdsCheckout") == Some(&Value::Bool(true));
    let id = order.get("todoId").and_then(Value::as_i64);
    let order_cwd = order.get("cwd").and_then(Value::as_str).unwrap_or_default();

    if holds && id != todo_id && !order_cwd.is_empty() && resolve(order_cwd) == here {
      holding.extend(id);
    }
  }

  Ok(holding)
}

/// Which run of its item a job is: the count of the item's jobs up to it.
pub async fn attempt(db: &impl ConnectionTrait, job_id: i64) -> Result<i64, Refusal> {
  let row = one(
    db,
    r#"SELECT COUNT(*) AS "count" FROM "jobs" WHERE "todo_id" IS NOT NULL
       AND "todo_id" = (SELECT "todo_id" FROM "jobs" WHERE "id" = $1) AND "id" <= $1"#,
    vec![job_id.into()],
  )
  .await?;

  Ok(row.map(|row| row.try_get::<i64>("", "count")).transpose()?.unwrap_or_default())
}
