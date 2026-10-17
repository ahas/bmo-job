//! The work-order runner's tick (docs/queue.md, "The work-order runner"): every few seconds each active or
//! stopped order taken in its lock, in the owner's order — closed with its item, a stopped one resumed at its
//! turn or its checkout let go, a running one's finished tasks collected into their result documents and a
//! unit gone without one ended, its phase moved on the plan and the build list, and each task of the stage it
//! stands at let into bmo-agency's co-worker pool and launched as the runner's `job-run` verb.

use std::collections::HashSet;
use std::path::Path;

use job_agency::{PoolAsking, PoolRequest, PoolState};
use job_codes::WorkOrderStop;
use job_codes::work_order_stops::StopReason;
use job_policy::Policy;
use queue_rows::order_lock::with_order_lock;
use queue_rows::todos::{self, Item};
use serde_json::{Value, json};

use super::hold::{Hold, base_back_on_main};
use super::launch::{Spawn, launch_task};
use super::pool::{self, COWORK, WORKER, Wanted, branch_task, keep_token, owner_of, role_key, token_of};
use super::{Fault, Ground, context, faulted, places, stops};
use crate::job_run::checkout::Checkout;
use crate::job_run::plan_check::{BUILDER, PLAN, Roster, read_build_tasks, read_plan};
use crate::job_run::recovery::{ROUTE_EXHAUSTED, execution_root, task_key};
use crate::job_run::worktree::{nothing_open, read_worktree_state, worktree_state};
use crate::job_run::{ABANDONED_LABEL, LIVE_LABEL};
use crate::queue::holds::queue_held;
use crate::queue::jobs;
use crate::queue::outcomes::{JOB_DIED, JOB_DONE, JOB_WORK_STEP};
use crate::queue::turn::orders_in;
use crate::queue::work_orders::{self as orders, Place, current_places, places, task_at, task_at_mut};
use crate::{Refusal, now_ms, one};

// What a stage's ending reads where its run reported nothing, failed without a report, or left with its
// unit gone.
const EMPTY_RESULT: &str = "The work's result is empty.";
const RUN_FAILED: &str = "The work's run failed.";
const UNIT_GONE: &str = "The run's process ended. The work can resume from the stages it finished.";

// What a final turn that ended done without closing or holding its item stops its order with.
const FINAL_NOT_CLOSED: &str = "The final turn ended without closing or holding its item.";

// What a stage that failed with no words of its own stops its order with.
const STAGE_FAILED: &str = "A stage failed.";

// How long a unit the machine no longer runs is given to write its ending before it is taken for gone.
const UNIT_GRACE_MS: i64 = 60_000;

// The line a builder turn's executions run on.
const WORKER_LINE: &str = "worker";

/// What a final turn that ended done leaves its order as, by its item: done where the turn finished the item,
/// stopped without a failure where it held it — a run that wound down after one step of several — and
/// refused where it did neither.
#[derive(Debug, PartialEq, Eq)]
enum FinalEnding {
  Closed,
  Held,
  Refused,
}

fn final_ending(item: Option<&Item>) -> FinalEnding {
  match item {
    Some(item) if item.done => FinalEnding::Closed,
    Some(item) if item.held_at.is_some() => FinalEnding::Held,
    _ => FinalEnding::Refused,
  }
}

/// What a tick asks of the world past the database and the checkouts: a unit's start and its stop, and what
/// the agent, the queue and the screens are told.
pub trait Hands: Spawn + Sync {
  /// A unit asked to stop.
  fn stop(&self, unit: &str) -> impl Future<Output = ()> + Send;
  /// An order that stopped, ended or could not be collected, told to the agent and the queue.
  fn notify(&self, order: &Value) -> impl Future<Output = ()> + Send;
  /// The queue told a worker an order held is free for its next item.
  fn freed(&self) -> impl Future<Output = ()> + Send;
  /// The screens told an order's body changed.
  fn changed(&self, todo_id: i64, order: &Value) -> impl Future<Output = ()> + Send;
}

fn text<'v>(value: &'v Value, key: &str) -> Option<&'v str> {
  value.get(key).and_then(Value::as_str)
}

fn state(task: &Value) -> &str {
  text(task, "state").unwrap_or_default()
}

fn settled(task: &Value) -> bool {
  matches!(state(task), "done" | "skipped")
}

fn remove(value: &mut Value, keys: &[&str]) {
  if let Some(object) = value.as_object_mut() {
    for key in keys {
      object.remove(*key);
    }
  }
}

// A failure as a task's record keeps it: its kind ahead of its words.
fn thrown(why: &str) -> String {
  format!("Error: {why}")
}

/// The first line of a stop's record that opens with one of these labels.
fn labelled(error: Option<&str>, labels: &[&str]) -> Option<String> {
  error?
    .split('\n')
    .map(str::trim)
    .find(|line| labels.iter().any(|label| line.starts_with(&format!("{label}: "))))
    .map(str::to_owned)
}

/// What the agent is told of an order that stopped: the line its record already carries — what the turn
/// walked away from, or what the runner ended it for — and otherwise what kind of stop it was.
pub fn paused_fact(policy: &Policy, order: &Value) -> WorkOrderStop {
  let error = text(order, "error");
  let labels = &policy.runaway.labels;
  let reason = if let Some(line) = labelled(error, &[LIVE_LABEL, ABANDONED_LABEL]) {
    StopReason::LeftRunning { line }
  } else if let Some(line) = labelled(error, &[&labels.spin, &labels.stalled]) {
    StopReason::Runaway { line }
  } else if error.is_some_and(|error| error.starts_with(ROUTE_EXHAUSTED)) {
    StopReason::NoRoute
  } else {
    StopReason::StageFailed
  };

  WorkOrderStop::OrderStopped {
    item: orders::number(order, "todoId").unwrap_or_default(),
    worker: text(order, "worker").map(str::to_owned),
    phase: text(order, "phase").map(str::to_owned),
    reason,
  }
}

// The orders the runner carries, in the owner's order: by their items' place in the queue and by number where
// two share one, an order whose item is gone last.
async fn by_priority(ground: &Ground<'_>) -> Result<Vec<i64>, Fault> {
  let mut carried = vec![];

  for order in orders_in(ground.db, &["active", "paused"]).await.map_err(faulted)? {
    let Some(todo) = orders::number(&order, "todoId") else { continue };
    let priority = todos::item(ground.db, todo).await.map_err(faulted)?.map(|item| item.priority);

    carried.push((priority.is_none(), priority.unwrap_or_default(), todo));
  }

  carried.sort_unstable();

  Ok(carried.into_iter().map(|(_, _, todo)| todo).collect())
}

// The last job a unit ran, with how its run went.
struct WorkJob {
  id: i64,
  launched_at: i64,
  finished: bool,
  outcome: Option<String>,
  report: Option<String>,
  mode: Option<String>,
  model: Option<String>,
  effort: Option<String>,
  upstream: Option<String>,
}

async fn work_job(ground: &Ground<'_>, unit: &str) -> Result<Option<WorkJob>, Fault> {
  let Some(row) = one(
    ground.db,
    r#"SELECT "id", "launched_at", "finished_at", "outcome", "report", "execution_mode", "execution_model", "execution_effort", "execution_upstream"
       FROM "jobs" WHERE "unit" = $1 ORDER BY "id" DESC LIMIT 1"#,
    vec![unit.into()],
  )
  .await
  .map_err(faulted)?
  else {
    return Ok(None);
  };
  let read = |key: &str| row.try_get::<Option<String>>("", key).ok().flatten();

  Ok(Some(WorkJob {
    id: row.try_get("", "id").map_err(|why| why.to_string())?,
    launched_at: row.try_get("", "launched_at").map_err(|why| why.to_string())?,
    finished: row.try_get::<Option<i64>>("", "finished_at").ok().flatten().is_some(),
    outcome: read("outcome"),
    report: read("report"),
    mode: read("execution_mode"),
    model: read("execution_model"),
    effort: read("execution_effort"),
    upstream: read("execution_upstream"),
  }))
}

// The road a step actually went out on, kept beside the one it asked for: the provider and model its job row
// names for the last execution, marked changed once two of the step's executions went out on different roads,
// whether across its attempts or inside one job between two ticks. A job whose run has not started an
// execution yet leaves the step as it was.
async fn note_ran_on(
  ground: &Ground<'_>,
  task: &mut Value,
  job: &WorkJob,
  unit: &str,
) -> Result<(), Fault> {
  let (Some(provider), Some(model)) = (
    job.mode.as_deref().filter(|mode| !mode.is_empty()),
    job.model.as_deref().filter(|model| !model.is_empty()),
  ) else {
    return Ok(());
  };
  // A road that names no provider or model says nothing about a switch.
  let elsewhere = |road: &serde_json::Map<String, Value>| match (
    road.get("provider").and_then(Value::as_str),
    road.get("model").and_then(Value::as_str),
  ) {
    (Some(other_provider), Some(other_model)) => other_provider != provider || other_model != model,
    _ => false,
  };
  // Only this job's executions: a unit name an earlier round used comes back in a later one.
  let executions = ground.units.all_executions().await.map_err(faulted)?;
  let earlier = task.get("ranOn").and_then(Value::as_object);
  let changed = earlier
    .is_some_and(|road| road.get("changed").and_then(Value::as_bool) == Some(true) || elsewhere(road))
    || executions
      .iter()
      .filter(|execution| {
        execution.parent.as_deref() == Some(unit) && execution.line.as_deref() == Some(WORKER_LINE)
      })
      .filter(|execution| execution.started_at.is_some_and(|at| at >= job.launched_at - 1000))
      .any(|execution| elsewhere(&execution.details));

  task["ranOn"] = json!({ "provider": provider, "model": model, "changed": changed });
  Ok(())
}

// A file written whole or not at all, read only by the service's user.
fn write_whole(path: &Path, body: &str) -> Result<(), Fault> {
  use std::io::Write as _;
  use std::os::unix::fs::OpenOptionsExt;

  let part = path.with_extension(format!(
    "{}.tmp",
    path
      .extension()
      .map(|extension| extension.to_string_lossy().into_owned())
      .unwrap_or_default()
  ));

  std::fs::OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .mode(0o600)
    .open(&part)
    .and_then(|mut file| file.write_all(body.as_bytes()))
    .and_then(|()| std::fs::rename(&part, path))
    .map_err(|why| format!("{}: {why}", path.display()))
}

/// The tier a pending task runs at, settled once and kept on it: the tier it names, the one its model names
/// where its role takes that tier, and its role's otherwise.
pub fn prepare_task_tier(roster: &Roster, task: &mut Value) -> Result<String, Fault> {
  if state(task) != "pending" {
    return Err("only a pending task settles its tier".to_owned());
  }

  if let Some(tier) = text(task, "tier") {
    return if roster.holds(tier) {
      Ok(tier.to_owned())
    } else {
      Err(format!("there is no model tier {tier}"))
    };
  }

  let model = text(task, "model").unwrap_or_default().to_owned();
  let role = text(task, "role").unwrap_or_default().to_owned();

  if !roster.holds(&model) && task.get("legacyModel").is_none_or(Value::is_null) {
    task["legacyModel"] = json!(model);
  }

  let follows = task.get("followRole").and_then(Value::as_bool) == Some(true);
  let fits = role == BUILDER || roster.lends(&model);
  let tier = if !follows && roster.holds(&model) && fits { model } else { roster.role_tier(&role)? };

  task["tier"] = json!(tier);
  Ok(tier)
}

// A fault carried out of an order's lock, which answers its own refusals too.
struct Carried(Fault);

impl From<Refusal> for Carried {
  fn from(refusal: Refusal) -> Self {
    Self(refusal.error)
  }
}

/// The runner: the ground it stands on, the checkouts it holds across its ticks, and the hands it reaches the
/// rest through.
pub struct Runner<'g, H> {
  ground: &'g Ground<'g>,
  hold: Hold,
  hands: H,
  busy: tokio::sync::Mutex<()>,
}

impl<'g, H: Hands> Runner<'g, H> {
  pub fn new(ground: &'g Ground<'g>, hold: Hold, hands: H) -> Self {
    Self {
      ground,
      hold,
      hands,
      busy: tokio::sync::Mutex::new(()),
    }
  }

  /// What the runner reaches past the database with.
  pub fn hands(&self) -> &H {
    &self.hands
  }

  /// The checks a stopped order's release left running in the background, waited for.
  pub async fn settled(&self) {
    self.hold.settled().await;
  }

  /// One tick: never beside another.
  pub async fn tick(&self) -> Result<(), Fault> {
    let Ok(_turn) = self.busy.try_lock() else { return Ok(()) };
    let ground = self.ground;
    let roster = Roster::read(ground.units).await.map_err(faulted)?;

    // A reservation whose order is no longer carried forward at all has no stage coming for it.
    pool::release_orphan_reservations(ground).await?;
    // A worker's turn whose unit has left the checkout gives its place back, whatever ended it.
    pool::release_turn_places(ground).await?;

    // In the owner's order rather than by number, each order taken in its own lock and read again inside
    // it, so an edit asked meanwhile lands before this body is read and one asked now waits for its save.
    for todo in by_priority(ground).await? {
      with_order_lock(ground.db, todo, async { self.carry(&roster, todo).await.map_err(Carried) })
        .await
        .map_err(|Carried(why)| why)?;
    }

    Ok(())
  }

  async fn save(&self, order: &mut Value) -> Result<(), Fault> {
    queue_rows::work_orders::save(self.ground.db, order).await.map_err(faulted)
  }

  async fn finish_pool(&self, task: &mut Value, outcome: &str, cancel: bool) -> Result<(), Fault> {
    pool::finish_pool(self.ground, task, outcome, cancel).await
  }

  // An order that let its checkout go hands its worker on to the queue.
  async fn release(&self, order: &mut Value) -> Result<(), Fault> {
    if self.hold.release(self.ground, order).await? {
      self.hands.freed().await;
    }

    Ok(())
  }

  async fn carry(&self, roster: &Roster, todo: i64) -> Result<(), Fault> {
    let db = self.ground.db;
    let Some(mut order) = queue_rows::work_orders::read(db, todo).await.map_err(faulted)? else {
      return Ok(());
    };
    let before = order.clone();
    let carried = self.carry_order(roster, &mut order).await;

    if let Ok(Some(after)) = queue_rows::work_orders::read(db, todo).await
      && orders::changed(Some(&before), &after)
    {
      self.hands.changed(todo, &after).await;
    }

    carried
  }

  async fn carry_order(&self, roster: &Roster, order: &mut Value) -> Result<(), Fault> {
    self.close_with_item(order).await?;

    // A stopped order that still holds its checkout lets it go before its turn is looked at.
    if text(order, "status") == Some("paused")
      && order.get("holdsCheckout").and_then(Value::as_bool) != Some(true)
    {
      stops::resume_at_turn(self.ground, roster, &self.hold, order).await?;
    }

    if text(order, "status") != Some("active") {
      pool::settle_reservations(self.ground, roster, order).await?;
      return self.release(order).await;
    }

    let todo = orders::number(order, "todoId").unwrap_or_default();

    if todos::item(self.ground.db, todo).await.map_err(faulted)?.is_none() {
      return Ok(());
    }

    match self.advance(roster, order).await {
      Ok(()) => pool::settle_reservations(self.ground, roster, order).await,
      Err(why) => self.stopped(roster, order, why).await,
    }
  }

  // A stage that could not be carried on: kept running where a task of it still runs, since a collection
  // error is no evidence that a child stopped; otherwise the order stops, counts the stop and lets go.
  async fn stopped(&self, roster: &Roster, order: &mut Value, why: Fault) -> Result<(), Fault> {
    order["error"] = json!(why);

    let current = current_places(order);

    if current.iter().any(|place| state(task_at(order, *place)) == "running") {
      self.save(order).await?;
      self.hands.notify(order).await;
      return Ok(());
    }

    let control = matches!(text(order, "phase"), Some("planning" | "synthesis" | "finalization"));
    // A control turn that had finished and still stopped the order gave a result the runner refused.
    let refused = control && current.iter().any(|place| state(task_at(order, *place)) == "done");

    order["status"] = json!("paused");

    for place in current {
      let task = task_at_mut(order, place);

      if state(task) == "pending" || (control && state(task) == "done") {
        self.finish_pool(task, &why, true).await?;
        task["state"] = json!("failed");
        task["error"] = json!(why);
      }
    }

    stops::count_stop(self.ground, order, refused).await?;
    self.save(order).await?;
    pool::settle_reservations(self.ground, roster, order).await?;
    self.hands.notify(order).await;
    places::retire_places(self.ground, order).await?;
    self.release(order).await
  }

  // An order whose item was finished or merged into another has nothing left to do, so it is closed once
  // nothing of it runs: what it had not passed over skipped with the reason, and one still holding its
  // checkout standing closing until the release lets it go.
  async fn close_with_item(&self, order: &mut Value) -> Result<(), Fault> {
    if order.get("closing").is_some() || places(order).iter().any(|(_, task)| state(task) == "running") {
      return Ok(());
    }

    let todo = orders::number(order, "todoId").unwrap_or_default();
    let Some(item) = todos::item(self.ground.db, todo).await.map_err(faulted)? else {
      return Ok(());
    };

    if !item.done && item.merged_into.is_none() {
      return Ok(());
    }

    let why = match item.merged_into {
      None => "The item is finished, so its work order is closed too".to_owned(),
      Some(into) => format!("The item was merged into #{into}, so its work order is closed too"),
    };
    let frees = text(order, "status") == Some("active")
      && order.get("holdsCheckout").and_then(Value::as_bool) != Some(true);
    let every: Vec<Place> = places(order).into_iter().map(|(place, _)| place).collect();

    for place in every {
      self.finish_pool(task_at_mut(order, place), &why, true).await?;
    }

    orders::close(order, &why, now_ms()).map_err(faulted)?;
    remove(order, &["queuePlace"]);
    self.save(order).await?;

    if text(order, "status") != Some("done") {
      return Ok(());
    }

    self.hands.notify(order).await;
    places::retire_places(self.ground, order).await?;

    if frees {
      self.hands.freed().await;
    }

    Ok(())
  }

  // One running task's run read: its result collected where its job ended, its unit taken for gone where the
  // machine no longer runs it, and its watch kept while it runs on. Whether a result landed.
  async fn collect(&self, order: &mut Value, place: Place, alive: &HashSet<String>) -> Result<bool, Fault> {
    let ground = self.ground;
    let unit = text(task_at(order, place), "unit").map(str::to_owned);
    let job = match &unit {
      Some(unit) => work_job(ground, unit).await?,
      None => None,
    };
    let mut landed = false;

    if let (Some(job), Some(unit)) = (&job, &unit) {
      let task = task_at_mut(order, place);

      task["jobId"] = json!(job.id);
      note_ran_on(ground, task, job, unit).await?;

      // The order's own line — every turn but a branch — is what its item ran on, read by whatever shows
      // the item's model.
      let task = task_at(order, place);

      if !branch_task(task)
        && let Some(road) = task.get("ranOn").cloned()
      {
        order["ranOn"] = road;
      }
    }

    match &job {
      Some(job) if job.finished => {
        let outcome = job.outcome.as_deref();

        if outcome != Some(JOB_DONE) && outcome != Some(JOB_WORK_STEP) {
          let task = task_at_mut(order, place);

          task["state"] = json!("failed");
          task["error"] = json!(job.report.clone().unwrap_or_else(|| RUN_FAILED.to_owned()));
          self
            .finish_pool(task, &format!("job {}: {}", job.id, outcome.unwrap_or("failed")), false)
            .await?;
          return Ok(false);
        }

        if text(task_at(order, place), "id") == Some(PLAN)
          && let (Some(mode), Some(model)) = (&job.mode, &job.model)
          && !mode.is_empty()
          && !model.is_empty()
        {
          order["model"] = json!(format!("{mode}::{model}"));
        }

        let report = job.report.as_deref().map(str::trim).unwrap_or_default();

        if report.is_empty() {
          let task = task_at_mut(order, place);

          task["state"] = json!("failed");
          task["error"] = json!(EMPTY_RESULT);
          self.finish_pool(task, &format!("job {}: empty result", job.id), false).await?;
          return Ok(false);
        }

        let todo = orders::number(order, "todoId").unwrap_or_default();
        let directory = ground.order_root(todo);

        std::fs::create_dir_all(&directory).map_err(|why| format!("{}: {why}", directory.display()))?;

        let task = task_at(order, place);
        let path = directory.join(format!(
          "{}-{}.md",
          task_key(order, task),
          orders::number(task, "attempt").unwrap_or_default()
        ));

        write_whole(&path, &orders::result_document(task, report))?;

        let builder = !branch_task(task);
        let task = task_at_mut(order, place);

        task["result"] = json!(report);
        task["artifact"] = json!(path.to_string_lossy());
        task["state"] = json!("done");
        landed = true;

        // A builder turn that was told of conflicted files has had its turn at them.
        if builder {
          remove(order, &["conflicts"]);
        }

        places::remember_slot(
          ground,
          order,
          task_at(order, place),
          (
            job.mode.as_deref(),
            job.model.as_deref(),
            job.effort.as_deref(),
            job.upstream.as_deref(),
          ),
        )
        .await?;
        self
          .finish_pool(task_at_mut(order, place), &format!("job {}: completed", job.id), false)
          .await?;
      }
      _ => {
        let task = task_at(order, place);
        let started = orders::number(task, "startedAt").unwrap_or(0);

        // A runaway stop already asked for comes back through the watch, which tells its own account once
        // the unit is gone; an unrelated disappearance takes the ordinary dead-unit path.
        if let Some(unit) = &unit
          && !alive.contains(unit)
          && now_ms() - started > UNIT_GRACE_MS
          && task.get("stopRequestedAt").is_none()
        {
          let task = task_at_mut(order, place);

          task["state"] = json!("failed");
          task["error"] = json!(UNIT_GONE);

          if let Some(job) = &job {
            jobs::finish(ground.db, job.id, UNIT_GONE, false, JOB_DIED, now_ms())
              .await
              .map_err(faulted)?;
          }

          self.finish_pool(task, &format!("unit {unit}: confirmed stopped"), false).await?;
        }
      }
    }

    // Last, and only over what is still running: a result that has just landed is a finished stage
    // whatever its run looked like getting there, and a unit already gone has its own ending above.
    if state(task_at(order, place)) == "running" {
      let held = order.clone();

      stops::watch(
        ground,
        &held,
        task_at_mut(order, place),
        job.as_ref().map(|job| job.id),
        async |unit: &str| self.hands.stop(unit).await,
      )
      .await?;
    }

    let task = task_at_mut(order, place);

    if state(task) != "running" && token_of(task).is_some() {
      let outcome = text(task, "error")
        .map(str::to_owned)
        .unwrap_or_else(|| format!("task {}: ended", text(task, "id").unwrap_or_default()));

      self.finish_pool(task, &outcome, false).await?;
    }

    Ok(landed)
  }

  async fn advance(&self, roster: &Roster, order: &mut Value) -> Result<(), Fault> {
    let ground = self.ground;
    let db = ground.db;
    let current = current_places(order);
    let running: Vec<Place> = current
      .iter()
      .copied()
      .filter(|place| state(task_at(order, *place)) == "running")
      .collect();
    let units: Vec<String> = running
      .iter()
      .filter_map(|place| text(task_at(order, *place), "unit").map(str::to_owned))
      .collect();
    let alive = ground.running().active(&units).await.map_err(faulted)?;
    let mut progressed = false;

    for place in running {
      progressed |= self.collect(order, place, &alive).await?;
    }

    // A completed task clears the count of failed stops, unless a task of the same stage has failed meanwhile.
    if progressed && !current.iter().any(|place| state(task_at(order, *place)) == "failed") {
      remove(order, &["failedStops"]);
    }

    self.save(order).await?;

    let failed = current
      .iter()
      .find(|place| state(task_at(order, **place)) == "failed")
      .map(|place| text(task_at(order, *place), "error").unwrap_or(STAGE_FAILED).to_owned());
    let has_running = current.iter().any(|place| state(task_at(order, *place)) == "running");
    let has_pending = current.iter().any(|place| state(task_at(order, *place)) == "pending");

    // Peers finish and save their results before the order stops; while they run is also when the stage
    // after them is known and not yet launched, the one moment warming a place it will want saves anything.
    if has_running {
      pool::reserve_next_stage(ground, roster, order).await?;

      // A pool-limited stage is under way even while some of its tasks still wait: each branch that ends may
      // free a place for the next in line, so the stage is filled again before its peers are watched on.
      if !has_pending {
        return Ok(());
      }
    }

    if let Some(failed) = failed
      && !has_pending
    {
      return Err(failed);
    }

    if current.iter().all(|place| settled(task_at(order, *place))) {
      self.next(roster, order).await?;

      if text(order, "status") == Some("done") {
        let baseline = order.get("checkoutBaseline").map_or_else(nothing_open, read_worktree_state);

        base_back_on_main(ground, order, &baseline).await?;
        order["holdsCheckout"] = json!(false);
        remove(order, &["checkoutBaseline", "checkoutTurns", "releaseWait"]);
      }

      self.save(order).await?;

      if text(order, "status") == Some("done") {
        self.hands.notify(order).await;
        return places::retire_places(ground, order).await;
      }
    }

    let todo = orders::number(order, "todoId").unwrap_or_default();
    let item = todos::item(db, todo).await.map_err(faulted)?;
    let unavailable = match &item {
      None => Some("TODO missing before launch"),
      Some(item) if item.done => Some("TODO closed before launch"),
      Some(item) if item.held_at.is_some() || !item.approved || item.merged_into.is_some() => {
        Some("TODO unavailable before launch")
      }
      Some(_) => None,
    };

    if let Some(outcome) = unavailable {
      if pool::cancel_pending_pools(ground, order, outcome).await? {
        self.save(order).await?;
      }

      // A held item's order stops once nothing of it runs, keeping what it finished, so it lets its worker
      // and its checkout go instead of standing active over nothing.
      let current = current_places(order);

      if let Some(item) = item
        && item.held_at.is_some()
        && !item.done
        && !current.iter().any(|place| state(task_at(order, *place)) == "running")
      {
        order["status"] = json!("paused");
        self.save(order).await?;
        places::retire_places(ground, order).await?;

        // An order holding its checkout hands the queue on when it lets the checkout go; one that holds none
        // has freed its worker already.
        if order.get("holdsCheckout").and_then(Value::as_bool) != Some(true) {
          self.hands.freed().await;
        }
      }

      return Ok(());
    }

    // The stage boundary: what waits for the owner to start the queue again, or for a deployment to let it
    // go, is the launch of what comes next. A held co-work's waiting places rejoin the line on resume.
    if queue_held(db).await.map_err(faulted)? {
      if pool::cancel_pending_pools(ground, order, "automatic progression paused before launch").await? {
        self.save(order).await?;
      }

      return Ok(());
    }

    let pending = |order: &Value| -> Vec<Place> {
      current_places(order)
        .into_iter()
        .filter(|place| state(task_at(order, *place)) == "pending")
        .collect()
    };

    // One order in a checkout at a time, and only once what ran there before has left it.
    if !pending(order).is_empty() && !self.hold.take_checkout(ground, order).await? {
      return Ok(());
    }

    let mut branch_launched = false;

    for place in pending(order) {
      if self.launch(roster, order, place).await? {
        branch_launched = true;
      }
    }

    if branch_launched {
      self.hands.freed().await;
    }

    Ok(())
  }

  // A task's place asked of the pool: a branch's seat as a child of its order, a builder turn's as the
  // worker's own turn on its checkout. The place it was let into, or none while it waits. The request's key
  // names the generation it asks at, so a tick that died before the token was kept asks again for the same
  // request.
  async fn admit(
    &self,
    roster: &Roster,
    order: &mut Value,
    place: Place,
  ) -> Result<Option<PoolRequest>, Fault> {
    let ground = self.ground;
    let todo = orders::number(order, "todoId").unwrap_or_default();
    let tier = prepare_task_tier(roster, task_at_mut(order, place))?;
    let task = task_at(order, place).clone();
    let held = match token_of(&task) {
      Some(token) => Some(
        ground
          .units
          .pool_request(token)
          .await
          .map_err(faulted)?
          .ok_or("the stored co-worker pool request is missing")?,
      ),
      None => None,
    };
    let generation = orders::number(&task, "poolGeneration").unwrap_or(0) + i64::from(held.is_none());
    let key = task_key(order, &task);
    let branch = branch_task(&task);
    let cwd = orders::text(order, "cwd").unwrap_or_default().to_owned();
    let request_key = if branch {
      format!("work-order:{todo}:{key}:pool-{generation}")
    } else {
      format!(
        "work-order:{todo}:{key}:turn-{}:pool-{generation}",
        orders::number(&task, "attempt").unwrap_or_default() + 1
      )
    };
    let owner = owner_of(order);
    let role = text(&task, "role").unwrap_or_default();
    let source = if branch { COWORK } else { WORKER };
    let place_key = (!branch).then(|| cwd.clone());
    let request = match held {
      Some(held) => {
        let asked = &held.asking;

        if asked.request_key != request_key
          || asked.owner != owner
          || asked.source != source
          || asked.role != role
          || asked.work_key != role_key(role)
          || asked.place_key != place_key
          || asked.tier != tier
        {
          return Err("the stored co-worker pool request does not match its execution root".to_owned());
        }

        held
      }
      None => {
        let mapping = pool::mapping(
          ground,
          roster,
          &Wanted {
            role,
            selection: text(&task, "model").unwrap_or_default(),
            requested_tier: Some(&tier),
            provider: None,
          },
          branch,
        )
        .await?;
        let asking = PoolAsking {
          request_key,
          owner,
          source: source.to_owned(),
          role: role.to_owned(),
          work_key: role_key(role),
          provider: mapping.provider,
          model: mapping.model,
          effort: mapping.effort,
          tier: mapping.tier,
          yielding: false,
          place_key,
          turn: !branch,
          reservation: None,
        };
        let request = ground.units.enqueue_request(&asking).await.map_err(faulted)?;

        keep_token(task_at_mut(order, place), &request.token, generation);
        self.save(order).await?;
        request
      }
    };

    if request.state == PoolState::Ended {
      return Err("the stored co-worker pool request is already finished".to_owned());
    }

    let leased = ground.units.lease_place(&request.token).await.map_err(faulted)?;

    Ok((leased.state == PoolState::Active).then_some(leased))
  }

  // A pending task let into the pool, composed and launched; whether it was a branch that launched.
  async fn launch(&self, roster: &Roster, order: &mut Value, place: Place) -> Result<bool, Fault> {
    let ground = self.ground;
    let todo = orders::number(order, "todoId").unwrap_or_default();
    let admitted = match self.admit(roster, order, place).await {
      Ok(admitted) => admitted,
      Err(why) => {
        let task = task_at_mut(order, place);

        self.finish_pool(task, &format!("admission failed: {}", thrown(&why)), true).await?;
        task["state"] = json!("failed");
        task["error"] = json!(thrown(&why));
        self.save(order).await?;
        return Ok(false);
      }
    };
    let Some(active) = admitted else { return Ok(false) };
    // The step's prompt is composed before the task is marked running: a translator that does not answer
    // leaves it waiting for the next tick rather than failing a stage over a restart.
    let prompt = match context::work_context(ground, roster, order, task_at(order, place)).await {
      Ok(prompt) => prompt,
      Err(why) => {
        eprintln!("bmo-job: work order {todo}: the step's prompt waits for the translator: {why}");
        self
          .finish_pool(task_at_mut(order, place), "the step's prompt was not composed before launch", true)
          .await?;
        self.save(order).await?;
        return Ok(false);
      }
    };
    let selected = format!("{}::{}", active.asking.provider, active.asking.model);
    let id = text(task_at(order, place), "id").unwrap_or_default().to_owned();

    task_at_mut(order, place)["model"] = json!(selected);

    if text(order, "model") == Some("auto") && id == PLAN {
      order["model"] = json!(selected);
    }

    if order.get("worktreeBaseline").is_none() {
      // An order already past planning accounts for no open work of its own; a checkout whose open work cannot
      // be read stops the order rather than launching on a baseline that says nothing is open.
      let baseline = if text(order, "phase") == Some("planning") {
        worktree_state(&Checkout::local(orders::text(order, "cwd").unwrap_or_default()))?
      } else {
        nothing_open()
      };

      order["worktreeBaseline"] = serde_json::to_value(baseline).unwrap_or(Value::Null);
    }

    let attempt = orders::number(task_at(order, place), "attempt").unwrap_or_default() + 1;
    let unit = format!("bmo-work-{todo}-{}-{attempt}", task_key(order, task_at(order, place)));

    // No run has carried this name before, so a cancel standing on it, or on the root its retries share, is
    // left over from a stop nothing took back, and would end the attempt as it starts.
    for name in std::iter::once(unit.clone()).chain(execution_root(&unit)) {
      ground.units.drop_cancels(&name).await.map_err(faulted)?;
    }

    let started = now_ms();
    let branch = branch_task(task_at(order, place));
    let task = task_at_mut(order, place);

    task["attempt"] = json!(attempt);
    task["unit"] = json!(unit);
    task["state"] = json!("running");
    task["startedAt"] = json!(started);

    if !branch && let Some(turns) = order.get_mut("checkoutTurns").and_then(Value::as_array_mut) {
      turns.push(json!({ "unit": unit, "from": started }));
    }

    ground.units.bind_child(&active.token, &unit).await.map_err(faulted)?;
    // Durable intent precedes the launch.
    self.save(order).await?;

    if !branch && let Some(merged) = context::merged_results(order) {
      let path = Path::new(&merged.path);

      if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|why| format!("{}: {why}", parent.display()))?;
      }

      write_whole(path, &merged.text)?;
    }

    let held = order.clone();
    let launched = launch_task(ground, &held, task_at_mut(order, place), &prompt, &self.hands).await;
    let task = task_at_mut(order, place);
    let mut branch_launched = false;

    match launched {
      Ok(job_id) => {
        task["jobId"] = json!(job_id);
        branch_launched = branch;
      }
      Err(why) => {
        self.finish_pool(task, &format!("launch failed: {}", thrown(&why)), false).await?;
        task["state"] = json!("failed");
        task["error"] = json!(thrown(&why));
      }
    }

    self.save(order).await?;

    Ok(branch_launched)
  }

  // The phase moved once every task of the stage it stood at is settled: the plan read into its stages, a
  // stage on to the next, the build list read once the stages end, the final turn once it is built, back to
  // the build where the final turn added steps, and the order done once the final turn has closed its item.
  async fn next(&self, roster: &Roster, order: &mut Value) -> Result<(), Fault> {
    let ground = self.ground;
    let model = text(order, "model").unwrap_or_default().to_owned();
    let answer = |order: &Value| {
      order
        .get("control")
        .and_then(|control| text(control, "result"))
        .unwrap_or_default()
        .to_owned()
    };

    match text(order, "phase").unwrap_or_default() {
      "planning" => {
        let stages = read_plan(roster, ground.policy, &answer(order), &model)?;

        retire_control(order);
        order["stages"] = Value::Array(stages);
        order["phase"] = json!("research");
        order["stage"] = json!(0);
      }
      "research" => order["stage"] = json!(orders::number(order, "stage").unwrap_or_default() + 1),
      "synthesis" => {
        let tasks = read_build_tasks(roster, &answer(order))?;

        retire_control(order);
        order["tasks"] = Value::Array(orders::with_added(order, tasks));
        order["phase"] = json!("implementation");
      }
      "finalization" => {
        let todo = orders::number(order, "todoId").unwrap_or_default();

        match final_ending(todos::item(ground.db, todo).await.map_err(faulted)?.as_ref()) {
          FinalEnding::Closed => order["status"] = json!("done"),
          // A final turn that added steps hands the order back to them, and a fresh final turn follows.
          _ if orders::steps_ahead(order) => orders::back_to_build(order),
          // The hold stops the order where `advance` meets it, as any held item's order stops, uncounted.
          FinalEnding::Held => {}
          FinalEnding::Refused => return Err(FINAL_NOT_CLOSED.to_owned()),
        }

        return Ok(());
      }
      _ => {}
    }

    let stages = order.get("stages").and_then(Value::as_array).map_or(0, Vec::len);

    if text(order, "phase") == Some("research")
      && usize::try_from(orders::number(order, "stage").unwrap_or_default())
        .is_ok_and(|stage| stage >= stages)
    {
      order["phase"] = json!("synthesis");
      order["control"] =
        orders::task("synthesis", BUILDER, &model, &context::synthesis_order(ground.policy));
    }

    if text(order, "phase") == Some("implementation")
      && order
        .get("tasks")
        .and_then(Value::as_array)
        .is_none_or(|tasks| tasks.iter().all(settled))
    {
      order["phase"] = json!("finalization");

      let todo = orders::number(order, "todoId").unwrap_or_default();
      let add_step = context::add_step(ground.policy, &ground.tree, todo);
      let mut control =
        orders::task("final", BUILDER, &model, &context::final_order(ground.policy, &add_step));

      // A final turn coming after one that handed the order back to its build counts its attempts on from
      // that one, so its unit and its result keep names of their own.
      control["attempt"] = json!(orders::final_attempt(order));
      order["control"] = control;
    }

    Ok(())
  }
}

// The control turn that has answered moved into the order's history.
fn retire_control(order: &mut Value) {
  let control = order.get_mut("control").map(Value::take).unwrap_or(Value::Null);

  if let Some(history) = order.get_mut("history").and_then(Value::as_array_mut) {
    history.push(control);
  } else {
    order["history"] = json!([control]);
  }
}
