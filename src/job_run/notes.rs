//! What a running job is handed of its item while it runs (docs/runs.md, "The turns"): the agent's changes to
//! the item's brief and the service's own wind-down line, read off the item's notes, handed to the worker in
//! the blocks the item's instruction is made of — pushed into the turn that is working, or asked over once a
//! turn has answered — and marked delivered once they reached it. The words a person or a companion adds are
//! never among them: they go to the agent, which folds what they ask into the brief and hands the job its
//! change. The agent is named by the deployment's name for it.

use chrono::{TimeZone, Utc};
use queue_read::blocks::render_block;
use queue_read::worker_lines::CHANGE_NOTE;
use queue_rows::todos::WIND_DOWN_NOTE_MARK;
use sea_orm::ConnectionTrait;

use crate::{Refusal, all, run};

/// One note handed to the job: the agent's change to the brief, or the wind-down line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
  pub id: i64,
  /// Whether it is the agent's change to the brief rather than the wind-down line.
  pub change: bool,
  pub text: String,
  /// When it was written, in epoch milliseconds.
  pub added_at: i64,
}

/// What was written onto the item for the job since `since` that no turn has been handed yet, oldest first:
/// the agent's changes to the brief and the wind-down line, and no words a person or a companion added.
pub async fn undelivered(db: &impl ConnectionTrait, todo: i64, since: i64) -> Result<Vec<Note>, Refusal> {
  let rows = all(
    db,
    r#"SELECT "id", "kind", "text", "added_at" FROM "todo_notes"
       WHERE "todo_id" = $1 AND "added_at" >= $2 AND "delivered_at" IS NULL AND "removed_at" IS NULL
         AND ("kind" = $3 OR substr("text", 1, length($4)) = $4)
       ORDER BY "id""#,
    vec![todo.into(), since.into(), CHANGE_NOTE.into(), WIND_DOWN_NOTE_MARK.into()],
  )
  .await?;

  rows
    .iter()
    .map(|row| {
      Ok(Note {
        id: row.try_get("", "id")?,
        change: row.try_get::<Option<String>>("", "kind")?.as_deref() == Some(CHANGE_NOTE),
        text: row.try_get("", "text")?,
        added_at: row.try_get("", "added_at")?,
      })
    })
    .collect()
}

/// The notes marked as handed to the job at `now`.
pub async fn mark_delivered(db: &impl ConnectionTrait, ids: &[i64], now: i64) -> Result<(), Refusal> {
  if ids.is_empty() {
    return Ok(());
  }

  let marks = (2..ids.len() + 2).map(|at| format!("${at}")).collect::<Vec<_>>().join(", ");
  let mut values: Vec<sea_orm::Value> = vec![now.into()];

  values.extend(ids.iter().map(|id| (*id).into()));
  run(
    db,
    &format!(r#"UPDATE "todo_notes" SET "delivered_at" = $1 WHERE "id" IN ({marks})"#),
    values,
  )
  .await?;

  Ok(())
}

fn iso(at_ms: i64) -> String {
  Utc
    .timestamp_millis_opt(at_ms)
    .single()
    .map_or_else(String::new, |at| at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
}

// The notes in the blocks the item's instruction is made of, so they read as that document going on: each
// change under its own label, and the wind-down line under its.
fn rendered(notes: &[Note], agent: &str) -> String {
  notes
    .iter()
    .map(|note| {
      render_block(
        if note.change { "Change to the brief" } else { "Wind-down" },
        &format!("note {}, {agent}, {}", note.id, iso(note.added_at)),
        &note.text,
      )
    })
    .collect::<Vec<_>>()
    .join("\n\n")
}

/// What the agent hands the run while it is still working: it belongs to this same piece of work, and a run
/// told to stop commits, reports and ends rather than simply stopping.
pub fn aside_task(todo: i64, notes: &[Note], agent: &str) -> String {
  [
    format!(
      "While you are working on TODO item #{todo} — right now, in this turn — {agent} changed what it asks of you. This belongs to this same piece of work and is not a new job."
    ),
    rendered(notes, agent),
    "Take it into what you are doing now and into the report you were going to write. If it tells you to stop, do not simply stop: commit and push what you have, write your report covering what you did and what you are leaving unfinished, and end normally.".to_owned(),
  ]
  .join("\n\n")
}

/// What the run is told when the agent changed what the item asks after its turn answered: the item it has
/// been doing rather than a new job, nothing it wrote handed over yet, and an item it already closed one to
/// reopen with the platform's command line `bmo`.
pub fn follow_up_task(todo: i64, notes: &[Note], bmo: &str, agent: &str) -> String {
  [
    format!(
      "While you were working on TODO item #{todo}, {agent} changed what it asks of you. This belongs to that item — the same piece of work you have just been doing — and is not a new job."
    ),
    rendered(notes, agent),
    format!(
      "Your report has not been handed over: nothing you wrote has reached anybody yet, and what you answer now is what {agent} is handed in its place. So do what this asks for, and then write your whole report again, in the same shape and covering the whole job — not a note about what changed since the last answer."
    ),
    format!(
      "If you had already marked the item done, reopen it with {bmo} todo --reopen {todo}, do this work, and close it again with an account that includes it."
    ),
  ]
  .join("\n\n")
}
