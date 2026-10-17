//! A work order's stops (docs/queue.md, "Stops and retries"): a running task watched for a runaway and stopped
//! or told about with its reason, a stop written down and counted toward the retries with the item set aside
//! past the last, and a stopped order resumed at its turn in its worker's queue — again, on a fresh session,
//! or on the retry model — or moved to another provider where its own could not carry it.

use std::collections::HashSet;
use std::sync::LazyLock;

use job_codes::WorkOrderStop;
use job_codes::checkout_holds::{LinePlace, PlaceReason};
use job_codes::work_order_stops::{RetryStep, RunawayAction, RunawayCause};
use job_policy::Policy;
use queue_rows::todos::{self, Item};
use regex::Regex;
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::hold::Hold;
use super::pool::{self, CLAUDE_CODE, Wanted, branch_task, finish_pool};
use super::{Fault, Ground, faulted};
use crate::job_run::plan_check::Roster;
use crate::job_run::recovery::{ROUTE_EXHAUSTED, task_key, text_failure};
use crate::queue::holds::{deploy_held, queue_paused};
use crate::queue::jobs;
use crate::queue::launches::todo_worker;
use crate::queue::outcomes::{JOB_DIED, JOB_LIMIT};
use crate::queue::routes::Roads;
use crate::queue::settings;
use crate::queue::turn::{Routable, Turn, open_items, worker_queue_turn};
use crate::queue::work_orders::{self as orders, Place, current_places, task_at, task_at_mut};
use crate::queue::workers::workers;
use crate::{now_ms, one};

/// How many counted stops are retried before the item is set aside.
pub const RETRY_LIMIT: u32 = 3;

/// The setting naming the model a stopped order's last retry runs on; where it names none, the policy's.
pub const RETRY_MODEL_SETTING: &str = "work_retry_model";

/// The kind of hold an item set aside by its failures is held as.
pub const FAILURE_HOLD: &str = "failure";

// The kind of hold the owner's stop is held as.
const STOP_HOLD: &str = "stop";

// A Claude model a retry may be named to run on.
static CLAUDE_MODEL: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^claude-[a-z0-9][a-z0-9.-]*$").expect("the Claude model pattern builds"));

// How long a stop asked of a unit that is still there waits before it is asked again.
const STOP_AGAIN_MS: i64 = 60_000;

fn text<'v>(value: &'v Value, key: &str) -> Option<&'v str> {
  value.get(key).and_then(Value::as_str)
}

/// What to do about a run that has not reported, and why: nothing, tell or stop, the measure that fired, and
/// the one sentence every record of it reads from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Verdict {
  pub action: Option<RunawayAction>,
  pub cause: Option<RunawayCause>,
  pub reason: Option<String>,
}

/// Past the re-invocation ceiling a run is ended where ending it costs only the run and told about where it
/// does not; past the stopping clock it is ended where ending it costs only the run. A count of zero is never
/// a spin.
pub fn runaway_verdict(policy: &Policy, elapsed_ms: i64, starts: u64, disposable: bool) -> Verdict {
  let runaway = &policy.runaway;

  if i64::try_from(starts).unwrap_or(i64::MAX) > runaway.max_starts {
    let action = if disposable { RunawayAction::Stopped } else { RunawayAction::Told };

    return Verdict {
      action: Some(action),
      cause: Some(RunawayCause::Spin),
      reason: Some(reason_line(policy, true, action, elapsed_ms, starts)),
    };
  }

  if elapsed_ms > runaway.stop_after_minutes * 60_000 && disposable {
    return Verdict {
      action: Some(RunawayAction::Stopped),
      cause: Some(RunawayCause::Clock),
      reason: Some(reason_line(policy, false, RunawayAction::Stopped, elapsed_ms, starts)),
    };
  }

  Verdict {
    action: None,
    cause: None,
    reason: None,
  }
}

fn reason_line(policy: &Policy, spin: bool, action: RunawayAction, elapsed_ms: i64, starts: u64) -> String {
  let runaway = &policy.runaway;
  let minutes = (elapsed_ms / 60_000).to_string();
  let starts = starts.to_string();
  let max = runaway.max_starts.to_string();
  let marks = [("starts", starts.as_str()), ("max", max.as_str()), ("minutes", minutes.as_str())];
  let said = &runaway.lines;
  let (label, head) = if spin {
    let waited = if elapsed_ms > 0 { policy.fill(&said.spin_waited, &marks) } else { String::new() };

    (&runaway.labels.spin, format!("{}{waited}", policy.fill(&said.spin, &marks)))
  } else {
    (&runaway.labels.stalled, policy.fill(&said.clock, &marks))
  };
  let tail = match action {
    RunawayAction::Stopped => policy.fill(&said.stopped, &marks),
    RunawayAction::Told => policy.fill(&said.told, &marks),
  };

  format!("{label}: {head} {tail}")
}

/// The name a task runs and is locked under: a branch's own, which nothing outlives, or the worker's.
pub fn task_worker(order: &Value, task: &Value) -> String {
  if branch_task(task) {
    format!(
      "work-order-{}-{}-{}",
      orders::number(order, "todoId").unwrap_or_default(),
      task_key(order, task),
      task.get("attempt").and_then(Value::as_i64).unwrap_or_default()
    )
  } else {
    text(order, "worker").unwrap_or_default().to_owned()
  }
}

// The fact of a runaway, as the owner's chat is told it.
fn runaway_fact(
  order: &Value,
  task: &Value,
  worker: &str,
  verdict: &Verdict,
  starts: u64,
  minutes: i64,
) -> WorkOrderStop {
  WorkOrderStop::StageRunaway {
    item: orders::number(order, "todoId").unwrap_or_default(),
    worker: worker.to_owned(),
    stage: text(task, "id").map(str::to_owned),
    unit: text(task, "unit").map(str::to_owned),
    cause: verdict.cause.unwrap_or(RunawayCause::Spin),
    starts,
    minutes: u64::try_from(minutes).unwrap_or_default(),
    action: verdict.action.unwrap_or(RunawayAction::Told),
  }
}

/// A stage that has stopped getting anywhere, read from outside off how often its run was started and the
/// clock: a branch, in a place nothing is taken back from, is ended; a builder turn is the owner's to end and
/// is told about once. A stop request is no terminal evidence: the task stays running until its unit is gone,
/// and a stop that did not take is asked again after a minute.
pub async fn watch(
  ground: &Ground<'_>,
  order: &Value,
  task: &mut Value,
  job_id: Option<i64>,
  stop: impl AsyncFn(&str),
) -> Result<(), Fault> {
  let Some(unit) = text(task, "unit").map(str::to_owned) else { return Ok(()) };
  let worker = task_worker(order, task);
  let starts = ground.units.run_starts(&worker, &unit).await.map_err(faulted)?;
  let now = now_ms();
  let elapsed = now - task.get("startedAt").and_then(Value::as_i64).unwrap_or(now);
  let verdict = runaway_verdict(ground.policy, elapsed, starts, branch_task(task));
  let Some(action) = verdict.action else { return Ok(()) };
  let fact = runaway_fact(order, task, &worker, &verdict, starts, elapsed / 60_000);

  if action == RunawayAction::Told {
    if task.get("told").and_then(Value::as_bool) == Some(true) {
      return Ok(());
    }

    task["told"] = json!(true);
    ground.tell(fact).await;

    return Ok(());
  }

  let requested = task.get("stopRequestedAt").and_then(Value::as_i64);

  if requested.is_none_or(|at| now >= at + STOP_AGAIN_MS) {
    task["stopRequestedAt"] = json!(now);
    task["stopReason"] =
      json!(verdict.reason.clone().unwrap_or_else(|| "The co-worker run was ended.".to_owned()));
    stop(&unit).await;
  }

  if ground
    .running()
    .active(std::slice::from_ref(&unit))
    .await
    .map_err(faulted)?
    .contains(&unit)
  {
    return Ok(());
  }

  let reason = verdict.reason.clone().unwrap_or_default();

  task["state"] = json!("failed");
  task["error"] = json!(text(task, "stopReason").map(str::to_owned).unwrap_or_else(|| reason.clone()));

  // The unit is collected, so nothing inside it will close the row it left open; the reason is plain, so
  // recovery reads no failure it knows how to route around, and the order waits for the owner.
  if let Some(job) = job_id {
    jobs::finish(ground.db, job, &reason, false, JOB_DIED, now_ms())
      .await
      .map_err(faulted)?;
  }

  ground.tell(fact).await;

  Ok(())
}

/// The model a stopped order's last retry runs on: the setting where it names a Claude Code model, bare or on
/// the job line's provider form, and the policy's otherwise.
pub fn retry_model(policy: &Policy, setting: Option<&str>) -> String {
  let value = setting.unwrap_or_default().trim();
  let model = match value.split_once("::") {
    Some((CLAUDE_CODE, model)) => model,
    Some(_) => "",
    None => value,
  };
  if CLAUDE_MODEL.is_match(model) {
    model.to_owned()
  } else {
    policy.queue.work_retry_model.clone()
  }
}

/// How the runner retries a stopped order at its turn, by how many of its stops were counted.
pub fn retry_step(stops: u32) -> RetryStep {
  match stops {
    0..=1 => RetryStep::Again,
    2 => RetryStep::FreshSession,
    _ => RetryStep::OtherModel,
  }
}

/// The runner's own resume of a stopped order: its failed tasks back to pending, carrying the retry.
pub fn retry(order: &mut Value, stops: u32, model: &str, now: i64) -> Result<RetryStep, Fault> {
  let failed = failed_places(order);
  let step = retry_step(stops);

  orders::resume(order, now).map_err(faulted)?;

  for place in failed {
    let task = task_at_mut(order, place);

    match step {
      RetryStep::FreshSession => task["retry"] = json!({ "freshSession": true }),
      RetryStep::OtherModel => task["retry"] = json!({ "model": model }),
      RetryStep::Again => {}
    }
  }

  Ok(step)
}

// Whether a failed task stopped on its provider before reaching the work: a limit, a road that refused, or
// every road tried.
async fn provider_stop(db: &DatabaseConnection, task: &Value) -> Result<bool, Fault> {
  let outcome = match text(task, "unit") {
    Some(unit) => one(
      db,
      r#"SELECT "outcome" FROM "jobs" WHERE "unit" = $1 ORDER BY "id" DESC LIMIT 1"#,
      vec![unit.into()],
    )
    .await
    .map_err(faulted)?
    .and_then(|row| row.try_get::<Option<String>>("", "outcome").ok().flatten()),
    None => None,
  };
  let error = text(task, "error");

  Ok(
    outcome.as_deref() == Some(JOB_LIMIT)
      || error.is_some_and(|error| text_failure(error).is_some())
      || error.is_some_and(|error| error.starts_with(ROUTE_EXHAUSTED)),
  )
}

fn failed_places(order: &Value) -> Vec<Place> {
  current_places(order)
    .into_iter()
    .filter(|place| text(task_at(order, *place), "state") == Some("failed"))
    .collect()
}

fn stops_of(order: &Value) -> u32 {
  order
    .get("failedStops")
    .and_then(Value::as_u64)
    .and_then(|stops| u32::try_from(stops).ok())
    .unwrap_or(0)
}

/// A stop written down with when it happened, and counted unless it stopped on its provider; a counted stop
/// is retried at the order's turn up to three times, and the fourth sets the item aside with the reason and
/// tells the owner.
pub async fn count_stop(ground: &Ground<'_>, order: &mut Value, refused: bool) -> Result<(), Fault> {
  order["stoppedAt"] = json!(now_ms());

  if !refused {
    let mut counted = false;

    for place in failed_places(order) {
      if !provider_stop(ground.db, task_at(order, place)).await? {
        counted = true;
        break;
      }
    }

    if !counted {
      return Ok(());
    }
  }

  let stops = stops_of(order) + 1;

  order["failedStops"] = json!(stops);

  if stops <= RETRY_LIMIT {
    return Ok(());
  }

  let last = text(order, "error")
    .unwrap_or_default()
    .split('\n')
    .map(str::trim)
    .find(|line| !line.is_empty())
    .unwrap_or("no reason was written")
    .to_owned();
  let reason = format!("The work order's work failed {stops} times (last: {last})");
  let todo = orders::number(order, "todoId").unwrap_or_default();

  if !todos::hold_as(ground.db, todo, Some(&reason), Some(FAILURE_HOLD), now_ms())
    .await
    .map_err(faulted)?
  {
    return Ok(());
  }

  ground
    .tell(WorkOrderStop::OrderSetAside {
      item: todo,
      worker: text(order, "worker").map(str::to_owned),
      failures: stops,
    })
    .await;

  Ok(())
}

// Whether an item has a route to run on now, as the roads read once for the turn say.
struct Routes<'r> {
  roads: Roads<'r, DatabaseConnection>,
  now: i64,
}

impl Routable for Routes<'_> {
  async fn routable(&self, item: &Item) -> bool {
    self
      .roads
      .item_route_open(item.model.as_deref(), item.worker.as_deref(), self.now)
      .await
  }
}

// Writes the order's place in its worker's queue only when it moved.
async fn note_place(ground: &Ground<'_>, order: &mut Value, place: Option<LinePlace>) -> Result<(), Fault> {
  let place = place.map(|place| serde_json::to_value(place).unwrap_or(Value::Null));

  if order.get("queuePlace") == place.as_ref() {
    return Ok(());
  }

  match place {
    None => {
      if let Some(order) = order.as_object_mut() {
        order.remove("queuePlace");
      }
    }
    Some(place) => order["queuePlace"] = place,
  }

  queue_rows::work_orders::save(ground.db, order).await.map_err(faulted)
}

/// A stopped order the runner resumes itself: only where its item may still run, the queue is not held, it is
/// the turn in its worker's queue by the one judgement the queue's telling asks too, and nothing is left in
/// the checkout. Where the judgement left the order is written on it first.
pub async fn resume_at_turn(
  ground: &Ground<'_>,
  roster: &Roster,
  hold: &Hold,
  order: &mut Value,
) -> Result<(), Fault> {
  let db = ground.db;
  let todo = orders::number(order, "todoId").unwrap_or_default();
  let Some(item) = todos::item(db, todo).await.map_err(faulted)? else {
    return note_place(ground, order, None).await;
  };

  if item.done || !item.approved || item.merged_into.is_some() {
    return note_place(ground, order, None).await;
  }

  if item.held_at.is_some() {
    let kind = one(db, r#"SELECT "held_kind" FROM "todos" WHERE "id" = $1"#, vec![todo.into()])
      .await
      .map_err(faulted)?
      .and_then(|row| row.try_get::<Option<String>>("", "held_kind").ok().flatten());
    let why = if kind.as_deref() == Some(STOP_HOLD) {
      PlaceReason::StopHeld
    } else {
      PlaceReason::Held {
        reason: item.held_reason.clone(),
      }
    };

    return note_place(ground, order, Some(LinePlace::Held { why })).await;
  }

  if current_places(order)
    .into_iter()
    .any(|place| text(task_at(order, place), "state") == Some("running"))
  {
    return note_place(ground, order, None).await;
  }

  let now = now_ms();
  let known = workers(db, &ground.source).await.map_err(faulted)?;
  let items = open_items(db).await.map_err(faulted)?;
  let routes = Routes {
    roads: Roads::read(db, ground.units, ground.policy).await.map_err(faulted)?,
    now,
  };
  let turn = worker_queue_turn(db, &items, todo_worker(&item, &known), &known, &routes, now)
    .await
    .map_err(faulted)?;
  let mine = matches!(&turn, Turn::Resume { item, .. } if *item == todo);
  let passed = turn.passed().iter().find(|one| one.todo_id == todo).map(|one| one.why);
  let held = if queue_paused(db).await.map_err(faulted)? {
    Some(PlaceReason::OwnerPause)
  } else if deploy_held(db).await.map_err(faulted)? {
    Some(PlaceReason::DeployHold)
  } else {
    None
  };
  let busy = if mine { hold.checkout_busy(ground, order).await? } else { None };
  let why = match (&held, passed, &busy) {
    (Some(held), _, _) => held.clone(),
    (None, Some(reason), _) => PlaceReason::Passed { reason },
    (None, None, Some(wait)) => PlaceReason::Busy { wait: wait.clone() },
    (None, None, None) => PlaceReason::WorkerFree,
  };
  let place = if passed.is_some() {
    Some(LinePlace::Passing { why })
  } else if mine {
    Some(LinePlace::ResumeTurn { why })
  } else {
    None
  };

  note_place(ground, order, place).await?;

  if !mine || held.is_some() || busy.is_some() || order.get("closing").is_some() {
    return Ok(());
  }

  if stopped_on_route(db, order).await? {
    return recover(ground, roster, order).await;
  }

  let stages: Vec<String> = failed_places(order)
    .into_iter()
    .filter_map(|place| text(task_at(order, place), "id").map(str::to_owned))
    .collect();
  let stops = stops_of(order);
  let model =
    retry_model(ground.policy, settings::get(db, RETRY_MODEL_SETTING).await.map_err(faulted)?.as_deref());
  let step = retry(order, stops, &model, now_ms())?;

  queue_rows::work_orders::save(db, order).await.map_err(faulted)?;
  ground
    .tell(WorkOrderStop::OrderResuming {
      item: todo,
      worker: text(order, "worker").map(str::to_owned),
      stages,
      failures: stops,
      retry: step,
    })
    .await;

  Ok(())
}

async fn stopped_on_route(db: &DatabaseConnection, order: &Value) -> Result<bool, Fault> {
  let failed = failed_places(order);

  if failed.is_empty() {
    return Ok(false);
  }

  for place in failed {
    if !provider_stop(db, task_at(order, place)).await? {
      return Ok(false);
    }
  }

  Ok(true)
}

// Where a task that stopped on its provider goes next: a provider it has not tried first, then — once every
// one has been tried — one whose block has lapsed.
async fn route(
  ground: &Ground<'_>,
  roster: &Roster,
  task: &Value,
) -> Result<Option<(String, Vec<String>)>, Fault> {
  let db = ground.db;
  let job = task
    .get("jobId")
    .and_then(Value::as_i64)
    .map_or_else(|| "null".to_owned(), |job| job.to_string());
  let attempted: Vec<String> = settings::get(db, &format!("job_route_attempts:{job}"))
    .await
    .map_err(faulted)?
    .and_then(|raw| serde_json::from_str(&raw).ok())
    .unwrap_or_default();
  let roads = Roads::read(db, ground.units, ground.policy).await.map_err(faulted)?;
  let provider_of =
    |value: &str| roads.choice(value).map_or(value.to_owned(), |choice| choice.provider.to_owned());
  let mut tried: Vec<String> = vec![];
  let mut add = |value: String| {
    if !tried.contains(&value) {
      tried.push(value);
    }
  };

  for provider in task
    .get("recoveryProviders")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
  {
    add(provider.to_owned());
  }

  for value in &attempted {
    add(provider_of(value));
  }

  add(provider_of(text(task, "model").unwrap_or_default()));

  let candidates = roads.job_route_candidates(Some("auto"), None, None).await.map_err(faulted)?;
  let now = now_ms();
  let mut open = vec![];

  for candidate in candidates {
    if !roads.route_blocked(&candidate, now).await.map_err(faulted)? {
      open.push(candidate);
    }
  }

  let tried_set: HashSet<&str> = tried.iter().map(String::as_str).collect();
  let Some(selection) = open
    .iter()
    .find(|value| roads.choice(value).is_some_and(|choice| !tried_set.contains(choice.provider)))
    .or(open.first())
    .cloned()
  else {
    return Ok(None);
  };
  let role = text(task, "role").unwrap_or_default();
  let tier = match text(task, "tier") {
    Some(tier) => tier.to_owned(),
    None => roster.role_tier(role)?,
  };
  let mapping = pool::mapping(
    ground,
    roster,
    &Wanted {
      role,
      selection: &selection,
      requested_tier: Some(&tier),
      provider: None,
    },
    branch_task(task),
  )
  .await?;

  Ok(Some((format!("{}::{}", mapping.provider, mapping.model), tried)))
}

// Moving a failed stage to another provider is the machine deciding to carry on, announced to the owner as a
// resuming, so it waits while the queue is held.
async fn recover(ground: &Ground<'_>, roster: &Roster, order: &mut Value) -> Result<(), Fault> {
  let db = ground.db;

  if queue_paused(db).await.map_err(faulted)? || deploy_held(db).await.map_err(faulted)? {
    return Ok(());
  }

  let todo = orders::number(order, "todoId").unwrap_or_default();
  let Some(item) = todos::item(db, todo).await.map_err(faulted)? else { return Ok(()) };

  if item.done || !item.approved || item.held_at.is_some() || item.merged_into.is_some() {
    return Ok(());
  }

  let failed = failed_places(order);

  if failed.is_empty()
    || current_places(order)
      .into_iter()
      .any(|place| text(task_at(order, place), "state") == Some("running"))
  {
    return Ok(());
  }

  let mut replacements = vec![];

  for place in &failed {
    let task = task_at(order, *place);

    if !provider_stop(db, task).await? {
      return Ok(());
    }

    let Some(next) = route(ground, roster, task).await? else { return Ok(()) };

    replacements.push((*place, next));
  }

  for (place, (selection, tried)) in replacements {
    let worker = text(order, "worker").map(str::to_owned);
    let task = task_at_mut(order, place);
    let outcome = format!("provider recovery after {}", text(task, "error").unwrap_or("failed attempt"));

    finish_pool(ground, task, &outcome, false).await?;

    let from = text(task, "model").unwrap_or_default().to_owned();
    let id = text(task, "id").unwrap_or_default().to_owned();

    task["recoveryProviders"] = json!(tried);
    task["model"] = json!(selection);
    task["state"] = json!("pending");

    for key in ["error", "result", "artifact", "jobId", "unit"] {
      task[key] = Value::Null;
    }

    task["told"] = json!(false);

    if id == "plan" {
      order["model"] = json!(selection);
    }

    ground
      .tell(WorkOrderStop::StageRerouted {
        item: todo,
        worker,
        stage: id,
        from,
        to: selection,
      })
      .await;
  }

  order["status"] = json!("active");
  order["error"] = Value::Null;

  if let Some(order) = order.as_object_mut() {
    order.remove("stoppedAt");
  }

  queue_rows::work_orders::save(db, order).await.map_err(faulted)
}
