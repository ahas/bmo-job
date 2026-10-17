//! One step of a work order launched (docs/queue.md, "Launching a step"): a branch held to the pool lease it
//! was admitted on and given a branch place of the order's repository laid out of the checkout's open change,
//! a builder turn in the worker's own checkout; the prompt and the baseline the final turn reads written where
//! only the service's user reads them, the job's row opened, and the unit started as the runner's `job-run`
//! verb with the work step's arguments.

use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use serde_json::Value;

use super::pool::{self, branch_task, owner_of, token_of};
use super::stops::task_worker;
use super::{Fault, Ground, faulted, places};
use crate::job_run::recovery::task_key;
use crate::job_run::worktree::{nothing_open, read_worktree_state};
use crate::job_run::{
  WORK_FINAL_PHASE, WORK_SESSION_FRESH, WORK_SESSION_HANDOVER, WORK_SESSION_RESUME, WORK_STEP_PHASE,
};
use crate::queue::job_start::runner_command;
use crate::queue::jobs;
use crate::queue::launches::WORK_ORDER_STARTED_BY;
use crate::queue::outcomes::JOB_ERROR;
use crate::queue::work_orders as orders;
use crate::{now_ms, one};

fn text<'v>(value: &'v Value, key: &str) -> Option<&'v str> {
  value.get(key).and_then(Value::as_str)
}

/// How a unit is started: on the arguments systemd-run is given.
pub trait Spawn {
  fn spawn(&self, args: &[String]) -> impl Future<Output = Result<(), String>> + Send;
}

/// The session word a task is launched with: a builder turn carries the worker's session on, or hands it
/// over first where its retry asks for a fresh one; a branch opens one it throws away.
pub fn session_word(task: &Value) -> &'static str {
  if branch_task(task) {
    WORK_SESSION_FRESH
  } else if task.get("retry").and_then(|retry| retry.get("freshSession")).and_then(Value::as_bool)
    == Some(true)
  {
    WORK_SESSION_HANDOVER
  } else {
    WORK_SESSION_RESUME
  }
}

// A file only the service's user reads.
fn write_private(path: &Path, body: &str) -> Result<(), Fault> {
  use std::io::Write as _;

  std::fs::OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .mode(0o600)
    .open(path)
    .and_then(|mut file| file.write_all(body.as_bytes()))
    .map_err(|why| format!("{}: {why}", path.display()))
}

/// The task's unit started on its prompt; the job row it runs under. Where it could not be, the row is closed
/// as an error, the place the task had before is put back and the prompt taken away.
pub async fn launch_task(
  ground: &Ground<'_>,
  order: &Value,
  task: &mut Value,
  prompt: &str,
  spawn: &impl Spawn,
) -> Result<i64, Fault> {
  let todo = orders::number(order, "todoId").unwrap_or_default();
  let root = ground.order_root(todo);

  std::fs::create_dir_all(&root).map_err(|why| format!("{}: {why}", root.display()))?;

  let previous_slot = task.get("slot").cloned();
  let prompt_path = root.join(format!(
    "{}-{}.prompt.txt",
    task_key(order, task),
    orders::number(task, "attempt").unwrap_or_default()
  ));
  let mut job: Option<i64> = None;
  let launched = launch(ground, order, task, prompt, &prompt_path, &mut job, spawn).await;

  match launched {
    Ok(job_id) => Ok(job_id),
    Err(why) => {
      if let Some(job_id) = job {
        jobs::finish(ground.db, job_id, &format!("Error: {why}"), false, JOB_ERROR, now_ms())
          .await
          .map_err(faulted)?;
      }

      match previous_slot {
        Some(slot) => task["slot"] = slot,
        None => {
          if let Some(task) = task.as_object_mut() {
            task.remove("slot");
          }
        }
      }

      let _ = std::fs::remove_file(&prompt_path);

      Err(why)
    }
  }
}

async fn launch(
  ground: &Ground<'_>,
  order: &Value,
  task: &mut Value,
  prompt: &str,
  prompt_path: &Path,
  job: &mut Option<i64>,
  spawn: &impl Spawn,
) -> Result<i64, Fault> {
  let unit = text(task, "unit").unwrap_or_default().to_owned();
  let model = text(task, "model").unwrap_or_default().to_owned();
  let branch = branch_task(task);

  if branch {
    let (provider, chosen) =
      model.split_once("::").ok_or("Co-worker launch has no resolved provider model")?;
    let token = token_of(task).ok_or("Co-worker launch holds no place in the pool")?;
    let held = ground
      .units
      .pool_request(token)
      .await
      .map_err(faulted)?
      .ok_or("Stored co-worker pool request is missing")?;

    pool::require(
      &held,
      &owner_of(order),
      text(task, "role").unwrap_or_default(),
      &unit,
      provider,
      chosen,
    )?;

    // The place is taken before the plan is read, and its number stays on the task: what a branch ran in has
    // to be known again when its result lands and the list is written.
    task["slot"] = places::take_slot(ground, order, task, now_ms()).await?.into();
  }

  let worker = task_worker(order, task);
  let checkout = orders::text(order, "cwd").unwrap_or_default().to_owned();
  let cwd = match task.get("slot").and_then(Value::as_i64) {
    Some(slot) if branch => {
      let cwd = places::slot_cwd(ground, order, slot).await?;

      places::prepare_slot(&checkout, &cwd).await?;
      cwd.to_string_lossy().into_owned()
    }
    _ if branch => return Err("the co-worker task was given no branch place".to_owned()),
    _ => checkout,
  };
  let root = prompt_path.parent().unwrap_or(Path::new("."));
  let baseline_path = root.join("worktree-baseline.json");
  // Written in the shape a baseline is read in, whatever order the body keeps its members in.
  let baseline = order.get("worktreeBaseline").map_or_else(nothing_open, read_worktree_state);

  write_private(&baseline_path, &serde_json::to_string(&baseline).map_err(|why| why.to_string())?)?;
  write_private(prompt_path, prompt)?;

  let row = one(
    ground.db,
    r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "model", "started_by", "todo_id", "worker", "purpose", "activity_version")
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NULL, 1) RETURNING "id""#,
    vec![
      orders::number(order, "chatId").unwrap_or_default().into(),
      unit.as_str().into(),
      now_ms().into(),
      prompt.into(),
      model.as_str().into(),
      WORK_ORDER_STARTED_BY.into(),
      orders::number(order, "todoId").unwrap_or_default().into(),
      worker.as_str().into(),
    ],
  )
  .await
  .map_err(faulted)?
  .ok_or("the job's row was not opened")?;
  let job_id: i64 = row.try_get("", "id").map_err(|why| why.to_string())?;

  *job = Some(job_id);

  let phase = if orders::text(order, "phase") == Some("finalization") {
    WORK_FINAL_PHASE
  } else {
    WORK_STEP_PHASE
  };
  let structure = match text(task, "id") {
    Some("plan") => "plan",
    Some("synthesis") => "synthesis",
    _ => "",
  };
  let args = runner_command(
    ground.host,
    &unit,
    None,
    vec![
      orders::number(order, "chatId").unwrap_or_default().to_string(),
      prompt_path.to_string_lossy().into_owned(),
      job_id.to_string(),
      model,
      unit.clone(),
      worker,
      cwd,
      phase.to_owned(),
      session_word(task).to_owned(),
      structure.to_owned(),
      baseline_path.to_string_lossy().into_owned(),
      task.get("retry").and_then(|retry| text(retry, "model")).unwrap_or_default().to_owned(),
    ],
  );

  spawn.spawn(&args).await?;

  Ok(job_id)
}
