//! The jobs and their endings, as the `jobs` and `work_orders` tables hold them (docs/queue.md, "How a
//! job ends").

use queue_read::lines::BUILTIN_WORKER;
use sea_orm::ConnectionTrait;
use serde_json::Value;

use super::launches::WORK_ORDER_STARTED_BY;
use super::outcomes::RUN_STOPPED;
use super::settings;
use super::work_orders::{LostRunJob, steps_ahead};
use crate::{Refusal, all, one, run};

/// The prefix of the setting a stop asked of a job is marked under, before its unit is asked.
pub const JOB_STOP_PREFIX: &str = "job_stop:";

/// The phase of a work order whose turn is the item's own final job.
const FINAL_PHASE: &str = "finalization";

/// One run that ended: the job, the item it was a run of, the worker it ran on and how it ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobEnding {
  pub job_id: Option<i64>,
  pub todo_id: Option<i64>,
  pub worker: Option<String>,
  pub outcome: String,
}

/// One run's ending as the record reads it, out of the job the runner names and how the runner says it
/// ended. The item is read off the record, because what a failed end does to an item is the queue's to
/// settle and not the runner's; a stage of a work order is told with no item against it, since the order
/// counts its own stops, while its final turn is the item's own job ending and carries it; and a run a
/// person stopped ended as stopped whatever its runner read of its last answer.
pub async fn ending(
  db: &impl ConnectionTrait,
  job_id: Option<i64>,
  outcome: &str,
) -> Result<JobEnding, Refusal> {
  let row =
    match job_id {
      Some(job_id) => one(
        db,
        r#"SELECT "todo_id", "started_by", "worker", "outcome", "finished_at" FROM "jobs" WHERE "id" = $1"#,
        vec![job_id.into()],
      )
      .await?,
      None => None,
    };
  let Some(row) = row else {
    return Ok(JobEnding {
      job_id,
      todo_id: None,
      worker: None,
      outcome: outcome.trim().to_owned(),
    });
  };
  let todo_id: Option<i64> = row.try_get("", "todo_id")?;
  let started_by: Option<String> = row.try_get("", "started_by")?;
  let finished = row.try_get::<Option<i64>>("", "finished_at")?.is_some();
  let recorded: Option<String> = row.try_get("", "outcome")?;
  let todo_id = match todo_id {
    Some(todo_id) if started_by.as_deref() == Some(WORK_ORDER_STARTED_BY) => {
      final_turn(db, todo_id, job_id).await?.then_some(todo_id)
    }
    other => other,
  };
  let outcome = if finished && recorded.as_deref() == Some(RUN_STOPPED) {
    RUN_STOPPED
  } else {
    outcome.trim()
  };

  Ok(JobEnding {
    job_id,
    todo_id,
    worker: row.try_get("", "worker")?,
    outcome: outcome.to_owned(),
  })
}

// Whether the job is the item's work order's final turn: the order at its finalization, under the control
// of that job, with no step added ahead of it.
async fn final_turn(db: &impl ConnectionTrait, todo_id: i64, job_id: Option<i64>) -> Result<bool, Refusal> {
  let Some(body) = queue_rows::work_orders::read(db, todo_id).await? else {
    return Ok(false);
  };

  Ok(
    job_id.is_some()
      && body.get("phase").and_then(Value::as_str) == Some(FINAL_PHASE)
      && body.pointer("/control/jobId").and_then(Value::as_i64) == job_id
      && !steps_ahead(&body),
  )
}

/// One job not yet finished: its row, the unit it runs in, the item it is a run of and the worker.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpenJob {
  pub id: i64,
  pub unit: String,
  pub todo_id: Option<i64>,
  pub worker: String,
  pub started_by: Option<String>,
}

/// Every job not yet finished, oldest first.
pub async fn unfinished(db: &impl ConnectionTrait) -> Result<Vec<OpenJob>, Refusal> {
  let rows = all(
    db,
    r#"SELECT "id", "unit", "todo_id", "worker", "started_by" FROM "jobs" WHERE "finished_at" IS NULL ORDER BY "id""#,
    vec![],
  )
  .await?;

  rows
    .iter()
    .map(|row| {
      Ok(OpenJob {
        id: row.try_get("", "id")?,
        unit: row.try_get("", "unit")?,
        todo_id: row.try_get("", "todo_id")?,
        worker: row
          .try_get::<Option<String>>("", "worker")?
          .unwrap_or_else(|| BUILTIN_WORKER.to_owned()),
        started_by: row.try_get("", "started_by")?,
      })
    })
    .collect()
}

/// The items a job of which is not yet finished, each once, in order.
pub async fn items_at_work(db: &impl ConnectionTrait) -> Result<Vec<i64>, Refusal> {
  let mut items: Vec<i64> = unfinished(db).await?.into_iter().filter_map(|job| job.todo_id).collect();

  items.sort_unstable();
  items.dedup();

  Ok(items)
}

/// The last job a unit ran, where one is: its row and whether it has finished.
pub async fn of_unit(db: &impl ConnectionTrait, unit: &str) -> Result<Option<(i64, bool)>, Refusal> {
  let row = one(
    db,
    r#"SELECT "id", "finished_at" FROM "jobs" WHERE "unit" = $1 ORDER BY "id" DESC LIMIT 1"#,
    vec![unit.into()],
  )
  .await?;

  Ok(
    row
      .map(|row| -> Result<(i64, bool), sea_orm::DbErr> {
        Ok((row.try_get("", "id")?, row.try_get::<Option<i64>>("", "finished_at")?.is_some()))
      })
      .transpose()?,
  )
}

/// Whether a stop has been asked of a job and not yet closed with its row.
pub async fn stop_asked(db: &impl ConnectionTrait, job_id: i64) -> Result<bool, Refusal> {
  Ok(settings::get(db, &format!("{JOB_STOP_PREFIX}{job_id}")).await?.is_some())
}

/// A job marked stopped with the reason, before its unit is asked: whoever closes its row closes it as
/// stopped, the sweep after a restart included.
pub async fn request_stop(db: &impl ConnectionTrait, job_id: i64, why: &str) -> Result<(), Refusal> {
  settings::set(db, &format!("{JOB_STOP_PREFIX}{job_id}"), why).await
}

/// A job's row closed with its report and how it ended, at `now`. A job a person stopped ends as stopped
/// whoever closes it and whatever that closer read, and the stop's marker goes with the row it shaped.
pub async fn finish(
  db: &impl ConnectionTrait,
  job_id: i64,
  report: &str,
  ok: bool,
  outcome: &str,
  now: i64,
) -> Result<(), Refusal> {
  let asked = stop_asked(db, job_id).await?;
  let ended = one(db, r#"SELECT "outcome" FROM "jobs" WHERE "id" = $1"#, vec![job_id.into()])
    .await?
    .map(|row| row.try_get::<Option<String>>("", "outcome"))
    .transpose()?
    .flatten();
  let stopped = asked || ended.as_deref() == Some(RUN_STOPPED);

  run(
    db,
    r#"UPDATE "jobs" SET "finished_at" = $1, "report" = $2, "ok" = $3, "outcome" = $4 WHERE "id" = $5"#,
    vec![
      now.into(),
      report.into(),
      i64::from(ok && !stopped).into(),
      (if stopped { RUN_STOPPED } else { outcome }).into(),
      job_id.into(),
    ],
  )
  .await?;
  settings::forget(db, &format!("{JOB_STOP_PREFIX}{job_id}")).await
}

/// The job rows an item's runs left within a range of job numbers, as a lost run is read back off them.
pub async fn item_records(
  db: &impl ConnectionTrait,
  todo_id: i64,
  from: i64,
  to: i64,
) -> Result<Vec<LostRunJob>, Refusal> {
  let rows = all(
    db,
    r#"SELECT "id", "unit", "launched_at", "finished_at", "ok", "outcome", "model", "prompt", "report"
       FROM "jobs" WHERE "todo_id" = $1 AND "id" BETWEEN $2 AND $3 ORDER BY "id""#,
    vec![todo_id.into(), from.into(), to.into()],
  )
  .await?;

  rows
    .iter()
    .map(|row| {
      Ok(LostRunJob {
        id: row.try_get("", "id")?,
        unit: row.try_get("", "unit")?,
        launched_at: row.try_get("", "launched_at")?,
        finished_at: row.try_get("", "finished_at")?,
        ok: row.try_get::<Option<i64>>("", "ok")?.map(|ok| ok != 0),
        outcome: row.try_get("", "outcome")?,
        model: row.try_get("", "model")?,
        prompt: row.try_get("", "prompt")?,
        report: row.try_get("", "report")?,
      })
    })
    .collect()
}
