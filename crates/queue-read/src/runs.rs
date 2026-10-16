//! A room's jobs (docs/reading.md, "A room's jobs"): what one job is labelled by, and a room's jobs listed
//! newest first.

use job_base::all;
use job_codes::work_lines::JobLabel;
use sea_orm::ConnectionTrait;
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
    let prompt = match row.try_get::<Option<String>>("", "prompt_korean")? {
      Some(kept) => kept,
      None => row.try_get("", "prompt")?,
    };
    let english = row
      .try_get::<Option<String>>("", "prompt_english")?
      .filter(|english| english.trim() != prompt.trim());

    let todo_id: Option<i64> = row.try_get("", "todo_id")?;

    listed.push(ListedJob {
      label: work_label(todo_id, row.try_get("", "attempt")?),
      purpose: row
        .try_get::<Option<String>>("", "purpose")?
        .map(|purpose| purpose.trim().to_owned())
        .filter(|purpose| todo_id.is_none() && !purpose.is_empty()),
      unit: row.try_get("", "unit")?,
      launched_at: row.try_get("", "launched_at")?,
      finished_at: row.try_get("", "finished_at")?,
      ok: row.try_get::<Option<i64>>("", "ok")?.map(|ok| ok == 1),
      prompt,
      english,
      report: row.try_get("", "report")?,
    });
  }

  Ok(listed)
}
