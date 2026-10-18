//! Jobs as a person reads them (docs/reading.md, "A room's jobs", "A worker's job line" and "The jobs that
//! have just run"): what one job is labelled by, a room's jobs listed newest first, the line a worker's
//! screen keeps for one job with its prompt, and every room's jobs that have just run.

use job_base::{all, one};
use job_codes::work_lines::JobLabel;
use sea_orm::{ConnectionTrait, QueryResult, Value};
use serde::Serialize;

use crate::queue::Read;

/// The most a room's job list is asked for at once, and what it gives where nothing is asked.
pub const JOBS_MOST: i64 = 200;
pub const JOBS_DEFAULT: i64 = 20;

/// What one job is labelled by: its item and, from the second on, which attempt at the item it was.
pub fn work_label(todo_id: Option<i64>, attempt: i64) -> JobLabel {
  match todo_id {
    None => JobLabel::Itemless,
    Some(id) => JobLabel::Item {
      id,
      attempt: u32::try_from(attempt).ok().filter(|attempt| *attempt > 1),
    },
  }
}

/// One job of a room.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ListedJob {
  pub label: JobLabel,
  /// What an itemless job was launched for, where it says.
  pub purpose: Option<String>,
  pub unit: String,
  pub launched_at: i64,
  pub finished_at: Option<i64>,
  pub ok: Option<bool>,
  /// The prompt in the deployment's language where that reading was kept, otherwise as written.
  pub prompt: String,
  /// The English its worker was handed, where it differs from the prompt.
  pub english: Option<String>,
  pub report: Option<String>,
}

/// A room's jobs, newest first: `limit` of them, twenty where none is asked for and never more than
/// `JOBS_MOST`.
pub async fn jobs<C: ConnectionTrait>(db: &C, chat_id: i64, limit: Option<i64>) -> Read<Vec<ListedJob>> {
  let limit = limit.unwrap_or(JOBS_DEFAULT).clamp(1, JOBS_MOST);
  let rows = all(
    db,
    r#"SELECT "unit", "launched_at", "prompt", "prompt_english", "prompt_korean", "finished_at", "report", "ok",
              "todo_id", "purpose",
              CASE WHEN "todo_id" IS NULL THEN 0
                   ELSE (SELECT COUNT(*) FROM "jobs" AS "earlier"
                         WHERE "earlier"."todo_id" = "jobs"."todo_id" AND "earlier"."id" <= "jobs"."id")
              END AS "attempt"
       FROM "jobs" WHERE "chat_id" = $1
       ORDER BY "id" DESC LIMIT $2"#,
    vec![chat_id.into(), limit.into()],
  )
  .await?;
  let mut listed = Vec::new();

  for row in &rows {
    let JobPrompt { text, english } = job_prompt_of(row)?;
    let todo_id: Option<i64> = row.try_get("", "todo_id")?;

    listed.push(ListedJob {
      label: work_label(todo_id, row.try_get("", "attempt")?),
      purpose: itemless_purpose(row, todo_id)?,
      unit: row.try_get("", "unit")?,
      launched_at: row.try_get("", "launched_at")?,
      finished_at: row.try_get("", "finished_at")?,
      ok: ok_of(row)?,
      prompt: text,
      english,
      report: row.try_get("", "report")?,
    });
  }

  Ok(listed)
}

/// A job's prompt as a person reads it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JobPrompt {
  /// The prompt in the deployment's language where that reading was kept, otherwise as written.
  pub text: String,
  /// The English its worker was handed, where it differs from the prompt.
  pub english: Option<String>,
}

// A job's prompt out of a row holding `prompt`, `prompt_english` and `prompt_korean`.
fn job_prompt_of(row: &QueryResult) -> Read<JobPrompt> {
  let text = match row.try_get::<Option<String>>("", "prompt_korean")? {
    Some(kept) => kept,
    None => row.try_get("", "prompt")?,
  };
  let english = row
    .try_get::<Option<String>>("", "prompt_english")?
    .filter(|english| english.trim() != text.trim());

  Ok(JobPrompt { text, english })
}

// What a job with no item was launched for, where it says.
fn itemless_purpose(row: &QueryResult, todo_id: Option<i64>) -> Read<Option<String>> {
  Ok(
    row
      .try_get::<Option<String>>("", "purpose")?
      .map(|purpose| purpose.trim().to_owned())
      .filter(|purpose| todo_id.is_none() && !purpose.is_empty()),
  )
}

fn ok_of(row: &QueryResult) -> Read<Option<bool>> {
  Ok(row.try_get::<Option<i64>>("", "ok")?.map(|ok| ok == 1))
}

/// The job a worker's screen keeps its line for: the one of the unit `run` names where that job is the
/// worker's, otherwise the worker's latest job still running, otherwise its last; its number where it has
/// any.
pub async fn worker_job<C: ConnectionTrait>(db: &C, worker: &str, run: Option<&str>) -> Read<Option<i64>> {
  let mut asked: Vec<(&str, Vec<Value>)> = vec![];

  if let Some(run) = run {
    asked.push((
      r#"SELECT "id" FROM "jobs" WHERE "worker" = $1 AND "unit" = $2 ORDER BY "id" DESC LIMIT 1"#,
      vec![worker.into(), run.into()],
    ));
  }

  asked.push((
    r#"SELECT "id" FROM "jobs" WHERE "worker" = $1 AND "finished_at" IS NULL ORDER BY "id" DESC LIMIT 1"#,
    vec![worker.into()],
  ));
  asked.push((
    r#"SELECT "id" FROM "jobs" WHERE "worker" = $1 ORDER BY "id" DESC LIMIT 1"#,
    vec![worker.into()],
  ));

  for (sql, values) in asked {
    if let Some(row) = one(db, sql, values).await? {
      return Ok(Some(row.try_get("", "id")?));
    }
  }

  Ok(None)
}

/// What one model spent on a job, each kind of token where it was written.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelSpent {
  pub model: String,
  pub input_tokens: Option<i64>,
  pub output_tokens: Option<i64>,
  pub cache_read_tokens: Option<i64>,
  pub cache_write_tokens: Option<i64>,
}

/// The line a worker's screen keeps for one job.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JobLine {
  /// The job's unit.
  pub run: String,
  pub item: Option<i64>,
  /// The item's title.
  pub title: Option<String>,
  pub purpose: Option<String>,
  pub launched_at: i64,
  pub finished_at: Option<i64>,
  /// How many tools it called, as its spans count them; none written while it runs.
  pub tools: i64,
  /// What each model spent, the one that spent most first; none written while it runs.
  pub models: Vec<ModelSpent>,
}

/// The line of job `job`, with what its spans say it called and spent.
pub async fn job_line<C: ConnectionTrait>(db: &C, job: i64) -> Read<Option<JobLine>> {
  let row = one(
    db,
    r#"SELECT "jobs"."unit", "jobs"."todo_id", "todos"."title", "jobs"."purpose", "jobs"."launched_at",
              "jobs"."finished_at"
       FROM "jobs" LEFT JOIN "todos" ON "todos"."id" = "jobs"."todo_id"
       WHERE "jobs"."id" = $1"#,
    vec![job.into()],
  )
  .await?;
  let Some(row) = row else {
    return Ok(None);
  };
  let tools = one(
    db,
    r#"SELECT CAST(COALESCE(SUM("times"), 0) AS BIGINT) AS "tools" FROM "job_spans"
       WHERE "job_id" = $1 AND "kind" = 'tool'"#,
    vec![job.into()],
  )
  .await?
  .map(|row| row.try_get("", "tools"))
  .transpose()?
  .unwrap_or(0);
  let spent = all(
    db,
    r#"SELECT "name", "input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens"
       FROM "job_spans" WHERE "job_id" = $1 AND "kind" = 'usage'
       ORDER BY "tokens" DESC, "id""#,
    vec![job.into()],
  )
  .await?;
  let mut models = vec![];

  for row in &spent {
    models.push(ModelSpent {
      model: row.try_get("", "name")?,
      input_tokens: row.try_get("", "input_tokens")?,
      output_tokens: row.try_get("", "output_tokens")?,
      cache_read_tokens: row.try_get("", "cache_read_tokens")?,
      cache_write_tokens: row.try_get("", "cache_write_tokens")?,
    });
  }

  Ok(Some(JobLine {
    run: row.try_get("", "unit")?,
    item: row.try_get("", "todo_id")?,
    title: row.try_get("", "title")?,
    purpose: row
      .try_get::<Option<String>>("", "purpose")?
      .map(|purpose| purpose.trim().to_owned())
      .filter(|purpose| !purpose.is_empty()),
    launched_at: row.try_get("", "launched_at")?,
    finished_at: row.try_get("", "finished_at")?,
    tools,
    models,
  }))
}

/// The prompt of job `job`.
pub async fn job_prompt<C: ConnectionTrait>(db: &C, job: i64) -> Read<Option<JobPrompt>> {
  let row = one(
    db,
    r#"SELECT "prompt", "prompt_english", "prompt_korean" FROM "jobs" WHERE "id" = $1"#,
    vec![job.into()],
  )
  .await?;

  row.as_ref().map(job_prompt_of).transpose()
}

/// What the jobs that have just run are asked for at most at once, and what they give where nothing is
/// asked.
pub const RECENT_MOST: i64 = 200;
pub const RECENT_DEFAULT: i64 = 10;

/// One job that has just run.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RecentJob {
  pub id: i64,
  pub unit: String,
  pub chat: i64,
  /// The model it was launched on.
  pub model: Option<String>,
  pub launched_at: i64,
  pub finished_at: Option<i64>,
  pub outcome: Option<String>,
  pub ok: Option<bool>,
  pub label: JobLabel,
  /// What an itemless job was launched for, where it says.
  pub purpose: Option<String>,
  pub worker: Option<String>,
}

/// Every room's jobs, newest first: `limit` of them, ten where none is asked for and never more than
/// `RECENT_MOST`.
pub async fn recent_jobs<C: ConnectionTrait>(db: &C, limit: Option<i64>) -> Read<Vec<RecentJob>> {
  let limit = limit.unwrap_or(RECENT_DEFAULT).clamp(1, RECENT_MOST);
  let rows = all(
    db,
    r#"SELECT "id", "unit", "chat_id", "model", "launched_at", "finished_at", "outcome", "ok", "todo_id",
              "purpose", "worker",
              CASE WHEN "todo_id" IS NULL THEN 0
                   ELSE (SELECT COUNT(*) FROM "jobs" AS "earlier"
                         WHERE "earlier"."todo_id" = "jobs"."todo_id" AND "earlier"."id" <= "jobs"."id")
              END AS "attempt"
       FROM "jobs" ORDER BY "id" DESC LIMIT $1"#,
    vec![limit.into()],
  )
  .await?;
  let mut recent = Vec::new();

  for row in &rows {
    let todo_id: Option<i64> = row.try_get("", "todo_id")?;

    recent.push(RecentJob {
      id: row.try_get("", "id")?,
      unit: row.try_get("", "unit")?,
      chat: row.try_get("", "chat_id")?,
      model: row.try_get("", "model")?,
      launched_at: row.try_get("", "launched_at")?,
      finished_at: row.try_get("", "finished_at")?,
      outcome: row.try_get("", "outcome")?,
      ok: ok_of(row)?,
      label: work_label(todo_id, row.try_get("", "attempt")?),
      purpose: itemless_purpose(row, todo_id)?,
      worker: row.try_get("", "worker")?,
    });
  }

  Ok(recent)
}
