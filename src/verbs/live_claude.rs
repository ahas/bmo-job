//! A job opening the live-Claude guard for itself (docs/verbs.md, "The live-Claude guard"): `--purpose`
//! opens it for this job, `--list` says what this job opened and what each opening let through. Nobody is
//! asked; every run the guard lets through afterwards is kept against the opening.

use sea_orm::DatabaseConnection;

use super::stand::{VerbError, last_job, running_job};
use crate::{all, one};

/// The verb's two forms.
pub const USAGE: &str = "usage: job live-claude --purpose '<what it measures and why>' | --list";

fn ended(said: String) -> VerbError {
  VerbError::Ended { code: 1, said }
}

/// What the job running as `run` opened, oldest first, each with the runs it let through.
pub async fn list(db: &DatabaseConnection, run: &str) -> Result<Vec<String>, VerbError> {
  let Some(job) = running_job(db, run).await? else {
    return Ok(vec!["live-claude: this job has opened nothing".into()]);
  };
  let openings = all(
    db,
    r#"SELECT "id", "purpose" FROM "live_claude_openings" WHERE "job_id" = $1 ORDER BY "id""#,
    vec![job.id.into()],
  )
  .await?;

  if openings.is_empty() {
    return Ok(vec!["live-claude: this job has opened nothing".into()]);
  }

  let mut lines = vec![];

  for opening in openings {
    let id: i64 = opening.try_get("", "id")?;
    let purpose: String = opening.try_get("", "purpose")?;
    let runs = all(
      db,
      r#"SELECT "what", "ran_at" FROM "live_claude_runs" WHERE "opening_id" = $1 ORDER BY "id""#,
      vec![id.into()],
    )
    .await?;

    lines.push(format!(
      "#{id} {purpose} — {} run{}",
      runs.len(),
      if runs.len() == 1 { "" } else { "s" }
    ));

    for ran in runs {
      let at = chrono::DateTime::from_timestamp_millis(ran.try_get::<i64>("", "ran_at")?)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

      lines.push(format!("  {at} {}", ran.try_get::<String>("", "what")?));
    }
  }

  Ok(lines)
}

/// The guard opened for the job running as `run`, for the purpose it says, at `now`.
pub async fn open(
  db: &DatabaseConnection,
  run: &str,
  purpose: &str,
  now: i64,
) -> Result<Vec<String>, VerbError> {
  let purpose = purpose.trim();

  if purpose.is_empty() {
    return Err(ended(format!("live-claude: say what the runs are for with --purpose.\n{USAGE}")));
  }

  let Some(job) = last_job(db, run).await? else {
    return Err(ended(format!("live-claude: no job carries the run name {run}.")));
  };
  let opened = one(
    db,
    r#"INSERT INTO "live_claude_openings" ("job_id", "todo_id", "purpose", "opened_at") VALUES ($1, $2, $3, $4) RETURNING "id""#,
    vec![job.id.into(), job.todo_id.into(), purpose.into(), now.into()],
  )
  .await?
  .map(|row| row.try_get::<i64>("", "id"))
  .transpose()?
  .unwrap_or_default();

  Ok(vec![format!(
    "live-claude: opened #{opened} for this job — {purpose}. Every run it lets through is recorded; name them in the report."
  )])
}

/// The refusal outside a job.
pub fn not_a_job() -> VerbError {
  ended("live-claude: this process is not a job's, so there is no job to open the guard for.".into())
}
