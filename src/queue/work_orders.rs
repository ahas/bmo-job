//! A work order's body as the `work_orders` table keeps it, and the verbs that change it (docs/queue.md,
//! "A work order's verbs"). The body is the work-order runner's own JSON and is worked on as it stands,
//! member by member, so whatever the runner keeps that no verb here names comes back as it was written; a
//! member the runner deletes is removed, and one it sets to null stays null. Every verb is applied inside
//! the order's lock (queue-rows' `order_lock`) to a body read inside it, and each refusal is a code of
//! `WorkOrderRefusal`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use job_codes::{JobError, WorkOrderRefusal};
use regex::Regex;
use serde_json::{Map, Value, json};

use super::outcomes::{JOB_DONE, JOB_WORK_STEP};
use crate::Refusal;

/// The longest a task's instruction may be, in characters.
const INSTRUCTION_MOST: usize = 30_000;

/// The longest a skip, stop or close reason may be, in characters.
const REASON_MOST: usize = 2_000;

// The order's own frame — the plan, the synthesis and the final turn, a final turn that handed the order
// back to its build kept under its attempt — which no step verb touches.
static CONTROL: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"^(r\d+-)?(plan|synthesis|final(-\d+)?)$").expect("the control pattern builds")
});
// A build task of this round, by its number.
static BUILD: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^build-(\d+)$").expect("the build pattern builds"));
// A task carried over from an earlier round, named under it.
static CARRIED: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^r\d+-").expect("the carried pattern builds"));
// A work order step's unit: its item, the task's base name and the attempt.
static STEP_UNIT: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"^bmo-work-(\d+)-(?:r\d+-)?(.+)-(\d+)$").expect("the step unit pattern builds")
});

/// The tiers a task's model may name: the ladder, and the ones a co-worker is borrowed at, in the ladder's
/// order.
pub struct Tiers {
  pub ladder: Vec<String>,
  pub coworker: Vec<String>,
}

impl Tiers {
  fn is_tier(&self, value: &str) -> bool {
    self.ladder.iter().any(|tier| tier == value)
  }

  // Whether a task of this role may run at this tier: builder is the worker itself and takes any tier,
  // every other role is a borrowed co-worker and takes only a co-worker tier.
  fn fits_role(&self, role: &str, tier: &str) -> bool {
    role == "builder" || self.coworker.iter().any(|one| one == tier)
  }

  // The tiers only the worker itself runs at.
  fn worker_only(&self) -> Vec<String> {
    self.ladder.iter().filter(|tier| !self.coworker.contains(tier)).cloned().collect()
  }
}

/// A refusal of a change to an order.
pub fn refused(refusal: WorkOrderRefusal) -> Refusal {
  JobError::from(refusal).into()
}

/// The run a skip, a close or a stop has to end: its unit and job where it was launched, and the place it
/// waited for in the co-worker pool where it holds one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoppedRun {
  pub unit: Option<String>,
  pub job_id: Option<i64>,
  pub pool_request: Option<String>,
}

/// Where a task stands in its order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
  History(usize),
  Stage(usize, usize),
  Build(usize),
  Control,
}

/// Whether what a screen shows of the order changed: the write clock is not progress.
pub fn changed(before: Option<&Value>, after: &Value) -> bool {
  let Some(before) = before else { return true };
  let quiet = |order: &Value| {
    let mut order = order.clone();

    order["updatedAt"] = json!(0);
    order
  };

  quiet(before) != quiet(after)
}

/// A task as a plan or a launch makes one.
pub fn task(id: &str, role: &str, model: &str, instruction: &str) -> Value {
  json!({
    "id": id, "role": role, "model": model, "instruction": instruction, "state": "pending", "attempt": 0,
    "unit": null, "jobId": null, "startedAt": null, "result": null, "artifact": null, "error": null,
  })
}

pub fn text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
  value.get(key).and_then(Value::as_str)
}

pub fn number(value: &Value, key: &str) -> Option<i64> {
  value.get(key).and_then(Value::as_i64)
}

fn object(value: &mut Value) -> &mut Map<String, Value> {
  if !value.is_object() {
    *value = Value::Object(Map::new());
  }

  match value {
    Value::Object(map) => map,
    _ => unreachable!("the value was made an object above"),
  }
}

fn remove(value: &mut Value, keys: &[&str]) {
  let map = object(value);

  for key in keys {
    map.remove(*key);
  }
}

fn list<'a>(order: &'a Value, key: &str) -> &'a [Value] {
  order.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn list_mut<'a>(order: &'a mut Value, key: &str) -> &'a mut Vec<Value> {
  let entry = object(order).entry(key).or_insert_with(|| json!([]));

  if !entry.is_array() {
    *entry = json!([]);
  }

  match entry {
    Value::Array(list) => list,
    _ => unreachable!("the entry was made a list above"),
  }
}

fn stage_tasks(stage: &Value) -> &[Value] {
  list(stage, "tasks")
}

fn state(task: &Value) -> &str {
  text(task, "state").unwrap_or_default()
}

fn id(task: &Value) -> &str {
  text(task, "id").unwrap_or_default()
}

fn length(text: &str) -> usize {
  text.chars().count()
}

// A pool request a task holds, which is something only where it is a string with something in it.
fn pool_request(task: &Value) -> Option<String> {
  text(task, "poolRequest").filter(|request| !request.is_empty()).map(str::to_owned)
}

fn has(value: &Value, key: &str) -> bool {
  value.get(key).is_some()
}

fn settled(task: &Value) -> bool {
  matches!(state(task), "done" | "skipped")
}

/// The order's own tasks with where each stands: its earlier rounds, its stages, its build tasks and its
/// control turn.
pub fn places(order: &Value) -> Vec<(Place, &Value)> {
  let history = list(order, "history").iter().enumerate().map(|(at, task)| (Place::History(at), task));
  let stages = list(order, "stages").iter().enumerate().flat_map(|(stage, one)| {
    stage_tasks(one)
      .iter()
      .enumerate()
      .map(move |(at, task)| (Place::Stage(stage, at), task))
  });
  let builds = list(order, "tasks").iter().enumerate().map(|(at, task)| (Place::Build(at), task));
  let control = order
    .get("control")
    .filter(|control| control.is_object())
    .map(|task| (Place::Control, task));

  history.chain(stages).chain(builds).chain(control).collect()
}

pub fn task_at(order: &Value, place: Place) -> &Value {
  match place {
    Place::History(at) => &list(order, "history")[at],
    Place::Stage(stage, at) => &stage_tasks(&list(order, "stages")[stage])[at],
    Place::Build(at) => &list(order, "tasks")[at],
    Place::Control => &order["control"],
  }
}

pub fn task_at_mut(order: &mut Value, place: Place) -> &mut Value {
  match place {
    Place::History(at) => &mut list_mut(order, "history")[at],
    Place::Stage(stage, at) => &mut list_mut(&mut list_mut(order, "stages")[stage], "tasks")[at],
    Place::Build(at) => &mut list_mut(order, "tasks")[at],
    Place::Control => &mut order["control"],
  }
}

/// The open tasks a stop or a close reaches: the stages, the build tasks and the control turn.
fn open_places(order: &Value) -> Vec<Place> {
  places(order)
    .into_iter()
    .filter(|(place, _)| !matches!(place, Place::History(_)))
    .map(|(place, _)| place)
    .collect()
}

/// The tasks the order is at now.
pub fn current_places(order: &Value) -> Vec<Place> {
  match text(order, "phase") {
    Some("research") => {
      let stage = number(order, "stage").unwrap_or_default();

      usize::try_from(stage)
        .ok()
        .and_then(|stage| list(order, "stages").get(stage).map(|one| (stage, one)))
        .map(|(stage, one)| (0..stage_tasks(one).len()).map(|at| Place::Stage(stage, at)).collect())
        .unwrap_or_default()
    }
    Some("implementation") => list(order, "tasks")
      .iter()
      .position(|task| !settled(task))
      .map(|at| vec![Place::Build(at)])
      .unwrap_or_default(),
    _ => {
      if order.get("control").is_some_and(Value::is_object) {
        vec![Place::Control]
      } else {
        vec![]
      }
    }
  }
}

fn named(order: &Value, task: &str) -> Result<Place, Refusal> {
  places(order)
    .into_iter()
    .find(|(_, one)| id(one) == task)
    .map(|(place, _)| place)
    .ok_or_else(|| {
      refused(WorkOrderRefusal::TaskAbsent {
        task: task.to_owned(),
      })
    })
}

/// The pool request of the first task under this name.
pub fn pool_request_of(order: &Value, task: &str) -> Option<String> {
  places(order)
    .into_iter()
    .find(|(_, one)| id(one) == task)
    .and_then(|(_, one)| pool_request(one))
}

fn refuse_control(order: &Value, place: Place) -> Result<(), Refusal> {
  if place == Place::Control || CONTROL.is_match(id(task_at(order, place))) {
    return Err(refused(WorkOrderRefusal::ControlTurnUntouched));
  }

  Ok(())
}

fn refuse_unless_running(order: &Value) -> Result<(), Refusal> {
  match text(order, "status").unwrap_or_default() {
    "active" | "paused" => {}
    "done" => return Err(refused(WorkOrderRefusal::OrderFinished)),
    _ => return Err(refused(WorkOrderRefusal::OrderDraft)),
  }

  if has(order, "closing") {
    return Err(refused(WorkOrderRefusal::OrderClosing));
  }

  Ok(())
}

fn check_instruction(instruction: Option<&str>) -> Result<Option<String>, Refusal> {
  let Some(instruction) = instruction else { return Ok(None) };
  let trimmed = instruction.trim();

  if trimmed.is_empty() || length(trimmed) > INSTRUCTION_MOST {
    return Err(refused(WorkOrderRefusal::InstructionSize));
  }

  Ok(Some(trimmed.to_owned()))
}

// A reason trimmed, or the refusal of one empty or too long.
fn check_reason(why: &str, size: WorkOrderRefusal) -> Result<&str, Refusal> {
  let reason = why.trim();

  if reason.is_empty() || length(reason) > REASON_MOST {
    return Err(refused(size));
  }

  Ok(reason)
}

fn check_model(tiers: &Tiers, role: &str, model: Option<&str>) -> Result<Option<String>, Refusal> {
  let Some(model) = model else { return Ok(None) };

  if !tiers.is_tier(model) {
    return Err(refused(WorkOrderRefusal::ModelTierWrong {
      tiers: tiers.ladder.clone(),
    }));
  }

  if !tiers.fits_role(role, model) {
    return Err(refused(WorkOrderRefusal::CoworkerTierWrong {
      tiers: tiers.coworker.clone(),
      worker_only: tiers.worker_only(),
    }));
  }

  Ok(Some(model.to_owned()))
}

fn set_model(tiers: &Tiers, task: &mut Value, model: &str) {
  task["model"] = json!(model);

  if tiers.is_tier(model) {
    task["tier"] = json!(model);
  } else {
    remove(task, &["tier"]);
  }

  task["followRole"] = json!(false);
}

fn clear_run(task: &mut Value) {
  for key in ["result", "artifact", "jobId", "unit", "error", "startedAt", "slot"] {
    task[key] = Value::Null;
  }

  task["state"] = json!("pending");
  task["told"] = json!(false);
  remove(task, &["skip", "poolRequest", "stopRequestedAt", "stopReason"]);
}

fn stopped_run(task: &Value) -> StoppedRun {
  StoppedRun {
    unit: text(task, "unit").map(str::to_owned),
    job_id: number(task, "jobId"),
    pool_request: pool_request(task),
  }
}

fn todo_id(order: &Value) -> i64 {
  number(order, "todoId").unwrap_or_default()
}

/// Changes one waiting, failed or skipped task's instruction, and a branch's model with it, at `now`.
pub fn edit(
  order: &mut Value,
  tiers: &Tiers,
  task: &str,
  instruction: Option<&str>,
  model: Option<&str>,
  now: i64,
) -> Result<(), Refusal> {
  refuse_unless_running(order)?;

  let place = named(order, task)?;

  refuse_control(order, place)?;

  if matches!(place, Place::History(_)) {
    return Err(refused(WorkOrderRefusal::HistoryEdit));
  }

  match state(task_at(order, place)) {
    "running" => {
      return Err(refused(WorkOrderRefusal::RunningTaskEdit {
        item: todo_id(order),
      }));
    }
    "done" => {
      return Err(refused(WorkOrderRefusal::DoneTaskEdit {
        task: task.to_owned(),
      }));
    }
    _ => {}
  }

  let role = text(task_at(order, place), "role").unwrap_or_default().to_owned();
  let instruction = check_instruction(instruction)?;
  let model = check_model(tiers, &role, model)?;

  if instruction.is_none() && model.is_none() {
    return Err(refused(WorkOrderRefusal::NothingToEdit));
  }

  let one = task_at_mut(order, place);

  if let Some(instruction) = instruction {
    one["instruction"] = json!(instruction);
  }

  if let Some(model) = model {
    set_model(tiers, one, &model);
  }

  remove(one, &["poolRequest"]);
  order["updatedAt"] = json!(now);

  Ok(())
}

/// Marks one task as decided against, at `now`. A waiting or failed task simply becomes skipped; a branch
/// still running comes back as the run the caller must stop; a done task keeps its result and document
/// and is only relabelled, which is taken on any order.
pub fn skip(order: &mut Value, task: &str, why: &str, now: i64) -> Result<Option<StoppedRun>, Refusal> {
  let reason = check_reason(why, WorkOrderRefusal::SkipReasonSize)?;
  let place = named(order, task)?;

  refuse_control(order, place)?;

  let one = task_at(order, place);

  if state(one) == "skipped" {
    return Err(refused(WorkOrderRefusal::TaskAlreadySkipped {
      task: task.to_owned(),
    }));
  }

  let mut stopped = pool_request(one).map(|_| stopped_run(one));

  if state(one) != "done" {
    refuse_unless_running(order)?;

    let one = task_at(order, place);

    if state(one) == "running" {
      if text(one, "role") == Some("builder") {
        return Err(refused(WorkOrderRefusal::BuilderRunning {
          item: todo_id(order),
        }));
      }

      stopped = Some(stopped_run(one));
    }

    task_at_mut(order, place)["error"] = Value::Null;
  }

  let one = task_at_mut(order, place);

  remove(one, &["poolRequest"]);
  one["state"] = json!("skipped");
  one["skip"] = json!({ "why": reason, "at": now });
  order["updatedAt"] = json!(now);

  Ok(stopped)
}

/// What has to move for a waiting task at this place to be reached.
enum Reach {
  Stays,
  BackTo(i64),
}

fn reachable(
  order: &Value,
  place: Place,
  history: WorkOrderRefusal,
  later: bool,
) -> Result<Reach, Refusal> {
  let phase = text(order, "phase").unwrap_or_default();

  if matches!(place, Place::History(_)) {
    return Err(refused(history));
  }

  if phase == "research"
    && let Place::Stage(stage, _) = place
  {
    let current = number(order, "stage").unwrap_or_default();
    let stage = i64::try_from(stage).unwrap_or(i64::MAX);

    if stage > current {
      if !later {
        return Err(refused(WorkOrderRefusal::StageNotYet {
          task: id(task_at(order, place)).to_owned(),
        }));
      }

      return Ok(Reach::Stays);
    }

    let running_now = usize::try_from(current)
      .ok()
      .and_then(|current| list(order, "stages").get(current))
      .is_some_and(|one| stage_tasks(one).iter().any(|task| state(task) == "running"));

    if stage < current && running_now {
      return Err(refused(WorkOrderRefusal::StageRunning));
    }

    return Ok(Reach::BackTo(stage));
  }

  if phase == "implementation" && matches!(place, Place::Build(_)) {
    if list(order, "tasks").iter().any(|task| state(task) == "running") {
      return Err(refused(WorkOrderRefusal::BuildRunning));
    }

    return Ok(Reach::Stays);
  }

  if matches!(place, Place::Stage(..)) && phase != "planning" {
    return Err(refused(WorkOrderRefusal::ResearchOver));
  }

  Err(refused(WorkOrderRefusal::PhaseWrong))
}

fn reach(order: &mut Value, reach: Reach) {
  if let Reach::BackTo(stage) = reach {
    order["stage"] = json!(stage);
  }
}

/// Puts one done, failed or skipped task back to waiting at `now`, with a new instruction or model if
/// asked.
pub fn rerun(
  order: &mut Value,
  tiers: &Tiers,
  task: &str,
  instruction: Option<&str>,
  model: Option<&str>,
  now: i64,
) -> Result<(), Refusal> {
  refuse_unless_running(order)?;

  let place = named(order, task)?;

  refuse_control(order, place)?;

  match state(task_at(order, place)) {
    "running" => {
      return Err(refused(WorkOrderRefusal::TaskRunning {
        task: task.to_owned(),
      }));
    }
    "pending" => {
      return Err(refused(WorkOrderRefusal::TaskPending {
        task: task.to_owned(),
      }));
    }
    _ => {}
  }

  let role = text(task_at(order, place), "role").unwrap_or_default().to_owned();
  let instruction = check_instruction(instruction)?;
  let model = check_model(tiers, &role, model)?;
  let moved = reachable(order, place, WorkOrderRefusal::HistoryRerun, false)?;
  let one = task_at_mut(order, place);

  clear_run(one);

  if let Some(instruction) = instruction {
    one["instruction"] = json!(instruction);
  }

  if let Some(model) = model {
    set_model(tiers, one, &model);
  }

  reach(order, moved);
  order["updatedAt"] = json!(now);

  Ok(())
}

/// Undoes a skip at `now`: a task that holds a result is done again, one that holds none is waiting again.
pub fn restore(order: &mut Value, task: &str, now: i64) -> Result<(), Refusal> {
  let place = named(order, task)?;
  let one = task_at(order, place);

  if state(one) != "skipped" {
    return Err(refused(WorkOrderRefusal::TaskNotSkipped {
      task: task.to_owned(),
    }));
  }

  if one.get("result") != Some(&Value::Null) {
    let one = task_at_mut(order, place);

    one["state"] = json!("done");
    remove(one, &["skip"]);
  } else {
    refuse_unless_running(order)?;

    let moved = reachable(order, place, WorkOrderRefusal::HistoryRestore, true)?;

    clear_run(task_at_mut(order, place));
    reach(order, moved);
  }

  order["updatedAt"] = json!(now);

  Ok(())
}

/// Moves a waiting build task, or a whole waiting research stage, before or after another, at `now`.
pub fn shift(order: &mut Value, task: &str, anchor: &str, after: bool, now: i64) -> Result<(), Refusal> {
  refuse_unless_running(order)?;

  if task == anchor {
    return Err(refused(WorkOrderRefusal::SameAnchor));
  }

  let from = named(order, task)?;
  let to = named(order, anchor)?;

  refuse_control(order, from)?;
  refuse_control(order, to)?;

  match (from, to) {
    (Place::Build(from), Place::Build(to)) => {
      let tasks = list_mut(order, "tasks");

      if state(&tasks[from]) != "pending" || state(&tasks[to]) != "pending" {
        return Err(refused(WorkOrderRefusal::BuildMove));
      }

      let moving = tasks.remove(from);
      let target = if to > from { to - 1 } else { to };

      tasks.insert(target + usize::from(after), moving);
    }
    (Place::Stage(from, _), Place::Stage(to, _)) => {
      if from == to {
        return Err(refused(WorkOrderRefusal::TaskMove));
      }

      let current = match text(order, "phase") {
        Some("research") => number(order, "stage").unwrap_or_default(),
        Some("planning") => -1,
        _ => i64::MAX,
      };
      let at = |stage: usize| i64::try_from(stage).unwrap_or(i64::MAX);

      if at(from) <= current || at(to) <= current {
        return Err(refused(WorkOrderRefusal::StageMovePast));
      }

      let stages = list_mut(order, "stages");

      if stage_tasks(&stages[from])
        .iter()
        .chain(stage_tasks(&stages[to]))
        .any(|task| state(task) != "pending")
      {
        return Err(refused(WorkOrderRefusal::StageMovePending));
      }

      let moving = stages.remove(from);
      let target = if to > from { to - 1 } else { to };

      stages.insert(target + usize::from(after), moving);
    }
    _ => return Err(refused(WorkOrderRefusal::MoveKinds)),
  }

  order["updatedAt"] = json!(now);

  Ok(())
}

/// Whether the build list holds a step not yet passed over: work the order still has to build.
pub fn steps_ahead(order: &Value) -> bool {
  list(order, "tasks").iter().any(|task| !settled(task))
}

// The next free build name: one past the highest number the list holds, whatever its order.
fn next_build(order: &Value) -> String {
  let next = list(order, "tasks")
    .iter()
    .filter_map(|task| BUILD.captures(id(task)).and_then(|found| found[1].parse::<u64>().ok()))
    .max()
    .map_or(0, |most| most + 1);

  format!("build-{next}")
}

/// Puts a step on the order's build list under the next free build name, at its end or before or after
/// (`anchor`, `after`) a pending build task, and answers that name. At the final turn the order goes back
/// to its build: now where the turn is not running, once it ends where it is.
pub fn add(
  order: &mut Value,
  mut step: Value,
  anchor: Option<(&str, bool)>,
  now: i64,
) -> Result<String, Refusal> {
  refuse_unless_running(order)?;

  let name = next_build(order);
  let at = match anchor {
    None => list(order, "tasks").len(),
    Some((anchor, after)) => match named(order, anchor)? {
      Place::Build(at) if state(&list(order, "tasks")[at]) == "pending" => at + usize::from(after),
      _ => return Err(refused(WorkOrderRefusal::AddAnchor)),
    },
  };

  step["id"] = json!(name);
  list_mut(order, "tasks").insert(at, step);

  if text(order, "phase") == Some("finalization")
    && order.get("control").is_none_or(|control| state(control) != "running")
  {
    back_to_build(order);
  }

  order["updatedAt"] = json!(now);

  Ok(name)
}

/// The build list a synthesis read, followed by the steps added before it stood, each renamed on from it.
pub fn with_added(order: &Value, read: Vec<Value>) -> Vec<Value> {
  let added: Vec<Value> = list(order, "tasks").to_vec();
  let mut tasks = read;

  for mut step in added {
    step["id"] = json!(format!("build-{}", tasks.len()));
    tasks.push(step);
  }

  tasks
}

/// The order handed back from its final turn to its build, where steps stand ahead of it. A final turn
/// that ended is kept in the history under its attempt, one that ended nothing is let go, and the attempts
/// it counted are kept for the final turn that follows the steps.
pub fn back_to_build(order: &mut Value) {
  let mut control = order.get_mut("control").map(Value::take).unwrap_or(Value::Null);
  let attempt = number(&control, "attempt").unwrap_or_default();

  if matches!(state(&control), "done" | "failed") {
    control["id"] = json!(format!("final-{attempt}"));
    list_mut(order, "history").push(control);
  }

  order["finalAttempt"] = json!(final_attempt(order).max(attempt));
  order["phase"] = json!("implementation");
}

/// The attempts the final turns this round handed back to the build have counted, which a fresh final
/// turn counts on from.
pub fn final_attempt(order: &Value) -> i64 {
  number(order, "finalAttempt").unwrap_or_default()
}

/// The owner's stop at `now`: every run of the order ends and the order stands stopped, ready to resume
/// from where it was cut. Each running task goes back to waiting with its run cleared and its attempt
/// kept, and comes back as a run the caller must stop; a waiting task's pool request comes back to be
/// cancelled. Done, skipped and failed tasks stay as they are.
pub fn stop(order: &mut Value, why: &str, now: i64) -> Result<Vec<StoppedRun>, Refusal> {
  refuse_unless_running(order)?;

  let reason = check_reason(why, WorkOrderRefusal::StopReasonSize)?.to_owned();
  let mut stopped = vec![];

  for place in open_places(order) {
    let one = task_at_mut(order, place);

    match state(one) {
      "running" => {
        stopped.push(stopped_run(one));

        for key in ["result", "artifact", "jobId", "unit", "error", "startedAt"] {
          one[key] = Value::Null;
        }

        one["state"] = json!("pending");
        one["told"] = json!(false);
        remove(one, &["stopRequestedAt", "stopReason"]);
      }
      "pending" if pool_request(one).is_some() => {
        stopped.push(StoppedRun {
          unit: None,
          job_id: None,
          pool_request: pool_request(one),
        });
      }
      _ => continue,
    }

    remove(one, &["poolRequest"]);
  }

  order["status"] = json!("paused");
  order["error"] = Value::Null;
  order["stoppedAt"] = json!(now);
  order["updatedAt"] = json!(now);
  order["ownerStop"] = json!({ "why": reason, "at": now });
  remove(order, &["queuePlace"]);

  Ok(stopped)
}

/// Ends an active or paused order midway at `now`. Every task not yet passed over is skipped with the
/// reason, and the branches still running come back as the runs the caller must stop. A builder turn
/// still running refuses the whole close.
pub fn close(order: &mut Value, why: &str, now: i64) -> Result<Vec<StoppedRun>, Refusal> {
  refuse_unless_running(order)?;

  let reason = check_reason(why, WorkOrderRefusal::CloseReasonSize)?.to_owned();
  let open: Vec<Place> = open_places(order)
    .into_iter()
    .filter(|place| !settled(task_at(order, *place)))
    .collect();

  if open.iter().any(|place| {
    let one = task_at(order, *place);

    state(one) == "running" && text(one, "role") == Some("builder")
  }) {
    return Err(refused(WorkOrderRefusal::CloseBuilderRunning {
      item: todo_id(order),
    }));
  }

  let mut stopped = vec![];

  for place in open {
    let one = task_at_mut(order, place);

    if state(one) == "running" || pool_request(one).is_some() {
      stopped.push(stopped_run(one));
    }

    one["state"] = json!("skipped");
    one["error"] = Value::Null;
    one["skip"] = json!({ "why": reason, "at": now });
    remove(one, &["poolRequest"]);
  }

  order["error"] = Value::Null;
  order["updatedAt"] = json!(now);

  // A closed order is done, and a done order holds no checkout. One that holds it may have a change of its
  // own open there, so it stops instead and is done once the runner has taken that change out of the tree
  // the way it does for any stopped order.
  if order.get("holdsCheckout") == Some(&Value::Bool(true)) {
    order["status"] = json!("paused");
    order["closing"] = json!({ "why": reason, "at": now });
  } else {
    order["status"] = json!("done");
  }

  Ok(stopped)
}

/// The stopped order under way again at `now`: its failed tasks go back to waiting, and so does a final
/// turn that ended done and held its item rather than closing it, to run again from the account it left.
/// A final turn with steps added ahead of it hands the order back to its build instead.
pub fn resume(order: &mut Value, now: i64) -> Result<(), Refusal> {
  if has(order, "closing") {
    return Err(refused(WorkOrderRefusal::OrderClosing));
  }

  if text(order, "status") != Some("paused") {
    return Err(refused(WorkOrderRefusal::ResumeNotStopped));
  }

  if text(order, "phase") == Some("finalization") && steps_ahead(order) {
    back_to_build(order);
  }

  let finalization = text(order, "phase") == Some("finalization");

  for place in current_places(order) {
    let one = task_at_mut(order, place);

    if state(one) == "failed" || (finalization && state(one) == "done") {
      for key in ["error", "jobId", "unit", "result", "artifact"] {
        one[key] = Value::Null;
      }

      one["state"] = json!("pending");
      one["told"] = json!(false);
      remove(one, &["retry"]);
    }
  }

  order["status"] = json!("active");
  order["error"] = Value::Null;
  order["updatedAt"] = json!(now);
  remove(order, &["stoppedAt", "queuePlace", "ownerStop"]);

  Ok(())
}

/// Opens a finished order again for another run at `now`, keeping what the runs before it did: every task
/// done or skipped moves into the history renamed under the round it was done in, the purpose and the
/// instruction that run had are kept beside them, and the order goes back to a draft.
pub fn reopen(order: &mut Value, now: i64) -> Result<(), Refusal> {
  if text(order, "status") != Some("done") {
    return Err(refused(WorkOrderRefusal::ReopenNotDone));
  }

  let round = number(order, "round").unwrap_or(1);
  let carried: Vec<Value> = places(order)
    .into_iter()
    .filter(|(_, one)| settled(one))
    .map(|(_, one)| {
      let mut one = one.clone();

      if !CARRIED.is_match(id(&one)) {
        one["id"] = json!(format!("r{round}-{}", id(&one)));
        one["round"] = json!(round);
      }

      one
    })
    .collect();
  let earlier = json!({ "round": round, "intent": order["intent"].clone(), "instruction": order["instruction"].clone() });

  list_mut(order, "earlier").push(earlier);
  order["history"] = Value::Array(carried);
  order["stages"] = json!([]);
  order["tasks"] = json!([]);
  order["control"] = Value::Null;
  order["status"] = json!("draft");
  order["phase"] = json!("planning");
  order["stage"] = json!(0);
  order["error"] = Value::Null;
  order["round"] = json!(round + 1);
  order["updatedAt"] = json!(now);
  remove(
    order,
    &[
      "worktreeBaseline",
      "checkoutBaseline",
      "holdsCheckout",
      "checkoutTurns",
      "releaseWait",
      "stoppedAt",
      "failedStops",
      "finalAttempt",
      "finalHolds",
    ],
  );

  Ok(())
}

/// One job row of a lost run, as the `jobs` table keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LostRunJob {
  pub id: i64,
  pub unit: String,
  pub launched_at: i64,
  pub finished_at: Option<i64>,
  pub ok: Option<bool>,
  pub outcome: Option<String>,
  pub model: Option<String>,
  pub prompt: String,
  pub report: Option<String>,
}

/// A result document a recovered task points at, for the caller to write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveredDocument {
  pub path: PathBuf,
  pub text: String,
}

fn between<'a>(text: &'a str, start: &str, end: &str) -> Option<&'a str> {
  let from = text.find(start)? + start.len();
  let to = text[from..].find(end)? + from;

  Some(&text[from..to])
}

// The role and the instruction a step was launched with, read back off the prompt it was sent. A branch's
// prompt opens on its own line naming the item, the role and the task; a builder turn's carries them in
// its current-task block. Both are the runner's own prompt shapes.
fn launched_as(job: &LostRunJob, base: &str) -> Option<(String, String)> {
  static BRANCH: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^TODO #\d+ · (\S+) · (\S+)\n").expect("the branch pattern builds"));
  static BLOCK: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[Current task\]\n(\S+) · (\S+)\n").expect("the block pattern builds"));

  if let Some(branch) = BRANCH.captures(&job.prompt)
    && CARRIED.replace(&branch[2], "") == base
  {
    let instruction = between(&job.prompt, &branch[0], "\n\n[Earlier results]")?;

    return Some((branch[1].to_owned(), instruction.to_owned()));
  }

  let block = BLOCK.captures(&job.prompt)?;

  if CARRIED.replace(&block[1], "") != base {
    return None;
  }

  let instruction = between(&job.prompt, &block[0], "\n\n[Where this task sits]")?;

  Some((block[2].to_owned(), instruction.to_owned()))
}

/// The document a task's result is kept in: its name heading it, then its id, role and model.
pub fn result_document(task: &Value, report: &str) -> String {
  let title = text(task, "title").map(str::trim).filter(|title| !title.is_empty());
  let name = title.unwrap_or_else(|| id(task));

  format!(
    "# {name}\n{} · {} · {}\n\n{report}",
    id(task),
    text(task, "role").unwrap_or_default(),
    text(task, "model").unwrap_or_default()
  )
}

/// Brings back a run of a finished order that a later run wrote over, from the job rows it left, at `now`.
/// The lost run becomes the round the order's body is numbered at now, placed after the rounds already in
/// its history, and the body's own run moves one round up. Each task is read from the last attempt its
/// unit names, and only one that ended well is carried. Answers the documents to write under `artifacts`.
pub fn recover(
  order: &mut Value,
  jobs: &[LostRunJob],
  artifacts: &Path,
  now: i64,
) -> Result<Vec<RecoveredDocument>, Refusal> {
  if text(order, "status") != Some("done") {
    return Err(refused(WorkOrderRefusal::RecoverNotDone));
  }

  if jobs.is_empty() {
    return Err(refused(WorkOrderRefusal::RecoverNoJobs));
  }

  let todo = todo_id(order);
  let known: HashSet<i64> = places(order).iter().filter_map(|(_, one)| number(one, "jobId")).collect();
  let mut latest: Vec<(String, &LostRunJob, i64)> = vec![];

  for job in jobs {
    if known.contains(&job.id) {
      return Err(refused(WorkOrderRefusal::RecoverJobKnown { job: job.id }));
    }

    if job.finished_at.is_none() {
      return Err(refused(WorkOrderRefusal::RecoverJobRunning { job: job.id }));
    }

    let Some(unit) = STEP_UNIT.captures(&job.unit).filter(|unit| unit[1].parse::<i64>().ok() == Some(todo))
    else {
      return Err(refused(WorkOrderRefusal::RecoverNotStep {
        job: job.id,
        unit: job.unit.clone(),
      }));
    };
    let attempt: i64 = unit[3].parse().unwrap_or_default();
    let base = unit[2].to_owned();

    match latest.iter_mut().find(|(seen, _, _)| *seen == base) {
      Some(seen) if attempt >= seen.2 => *seen = (base, job, attempt),
      Some(_) => {}
      None => latest.push((base, job, attempt)),
    }
  }

  latest.sort_by_key(|(_, job, _)| job.id);

  let round = number(order, "round").unwrap_or(1);
  let ids: HashSet<String> = places(order).iter().map(|(_, one)| id(one).to_owned()).collect();
  let mut documents = vec![];
  let mut recovered = vec![];
  let mut earlier: Option<Value> = None;
  let fallback_model = text(order, "model").unwrap_or_default().to_owned();

  for (base, job, attempt) in latest {
    let report = job.report.as_deref().map(str::trim).unwrap_or_default();
    let outcome = job.outcome.as_deref();

    if job.ok != Some(true) || !matches!(outcome, Some(JOB_WORK_STEP | JOB_DONE)) || report.is_empty() {
      continue;
    }

    let Some((role, instruction)) = launched_as(job, &base) else {
      return Err(refused(WorkOrderRefusal::RecoverPromptUnread { job: job.id, base }));
    };
    let task_id = format!("r{round}-{base}");

    if ids.contains(&task_id) {
      return Err(refused(WorkOrderRefusal::RecoverTaskTwice { task: task_id }));
    }

    let path = artifacts.join(todo.to_string()).join(format!("{task_id}-{attempt}.md"));
    let mut one =
      task(&task_id, &role, job.model.as_deref().unwrap_or(&fallback_model), instruction.trim());

    one["state"] = json!("done");
    one["attempt"] = json!(attempt);
    one["unit"] = json!(job.unit);
    one["jobId"] = json!(job.id);
    one["startedAt"] = json!(job.launched_at);
    one["result"] = json!(report);
    one["artifact"] = json!(path.to_string_lossy());
    one["round"] = json!(round);
    // A run read back off its job rows was never named by a plan, so its document is headed with the
    // task's own name.
    documents.push(RecoveredDocument {
      path,
      text: result_document(&one, report),
    });
    recovered.push(one);

    if earlier.is_none() && role == "builder" {
      let intent = between(&job.prompt, "\nPurpose: ", "\nInstruction: ");
      let said = between(&job.prompt, "\nInstruction: ", "\n\n[Completed steps]");

      if let (Some(intent), Some(said)) = (intent, said) {
        earlier = Some(json!({ "round": round, "intent": intent.trim(), "instruction": said.trim() }));
      }
    }
  }

  if recovered.is_empty() {
    return Err(refused(WorkOrderRefusal::RecoverNoSuccess));
  }

  let Some(earlier) = earlier else {
    return Err(refused(WorkOrderRefusal::RecoverNoBuilder));
  };
  let history = list(order, "history").to_vec();
  let (carried, own): (Vec<Value>, Vec<Value>) =
    history.into_iter().partition(|one| CARRIED.is_match(id(one)));

  order["history"] = Value::Array(carried.into_iter().chain(recovered).chain(own).collect());
  list_mut(order, "earlier").push(earlier);
  order["round"] = json!(round + 1);
  order["updatedAt"] = json!(now);

  Ok(documents)
}
