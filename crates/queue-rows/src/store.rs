//! The writes of an item's whole life (docs/rows.md, "The rows"): an item filed, rewritten and finished,
//! merged and unmerged, wound down to wait or held for a deployment, and the words added to it read whole,
//! added, marked handed over and taken back.

use job_base::{Refusal, all, one, run};
use sea_orm::{ConnectionTrait, DatabaseConnection, QueryResult, TransactionTrait};

pub use queue_read::worker_lines::CHANGE_NOTE;

use crate::todos::{DONE_STATUS, WAITING_STATUS};

/// The database's failure to answer a write with the row it wrote.
pub(crate) fn not_written(what: &str) -> Refusal {
  sea_orm::DbErr::Custom(format!("{what} was not written")).into()
}

/// One row as a verb reads it before it changes it.
#[derive(Clone, Debug)]
pub struct Row {
  pub id: i64,
  pub title: String,
  pub instruction: String,
  pub done: bool,
  pub priority: i64,
  pub note: Option<String>,
  pub report: Option<String>,
  pub unfinished: Option<String>,
  pub unfinished_resolution: Option<String>,
  pub held_reason: Option<String>,
  pub held_kind: Option<String>,
  pub merged_into: Option<i64>,
}

impl Row {
  fn of(row: &QueryResult) -> Result<Self, Refusal> {
    Ok(Self {
      id: row.try_get("", "id")?,
      title: row.try_get("", "title")?,
      instruction: row.try_get("", "instruction")?,
      done: row.try_get::<i64>("", "done")? == 1,
      priority: row.try_get("", "priority")?,
      note: row.try_get("", "note")?,
      report: row.try_get("", "report")?,
      unfinished: row.try_get("", "unfinished")?,
      unfinished_resolution: row.try_get("", "unfinished_resolution")?,
      held_reason: row.try_get("", "held_reason")?,
      held_kind: row.try_get("", "held_kind")?,
      merged_into: row.try_get("", "merged_into")?,
    })
  }

  /// Whether its work is carried by another item that is not finished yet.
  pub fn merged(&self) -> bool {
    self.merged_into.is_some() && !self.done
  }
}

fn rows(found: &[QueryResult]) -> Result<Vec<Row>, Refusal> {
  found.iter().map(Row::of).collect()
}

fn ids(found: &[QueryResult]) -> Result<Vec<i64>, Refusal> {
  let mut ids = found.iter().map(|row| row.try_get::<i64>("", "id")).collect::<Result<Vec<_>, _>>()?;

  ids.sort_unstable();

  Ok(ids)
}

/// One item by its number.
pub async fn row(db: &impl ConnectionTrait, id: i64) -> Result<Option<Row>, Refusal> {
  one(db, r#"SELECT * FROM "todos" WHERE "id" = $1"#, vec![id.into()])
    .await?
    .as_ref()
    .map(Row::of)
    .transpose()
}

/// The unfinished item a title names, the first in the queue's order where several carry it.
pub async fn row_titled(db: &impl ConnectionTrait, title: &str) -> Result<Option<Row>, Refusal> {
  one(
    db,
    r#"SELECT * FROM "todos" WHERE "title" = $1 AND "done" = 0 ORDER BY "priority" ASC, "id" ASC"#,
    vec![title.into()],
  )
  .await?
  .as_ref()
  .map(Row::of)
  .transpose()
}

/// An item as a caller names it: by its number written as the number itself would be, or by its title.
pub async fn named_row(db: &impl ConnectionTrait, selector: &str) -> Result<Option<Row>, Refusal> {
  let trimmed = selector.trim();

  match trimmed.parse::<i64>().ok().filter(|id| id.to_string() == trimmed) {
    Some(id) => row(db, id).await,
    None => row_titled(db, selector).await,
  }
}

/// A new item, waiting for the owner's approval behind everything already waiting.
pub struct NewItem<'a> {
  pub title: &'a str,
  pub instruction: &'a str,
  pub requester: &'a str,
  pub requester_id: Option<i64>,
  pub chat_id: Option<i64>,
  pub status: &'a str,
  pub model: &'a str,
  /// In epoch milliseconds.
  pub created_at: i64,
  /// The items whose unfinished note it was raised out of.
  pub sources: &'a [i64],
  /// The agent whose turn filed it, which the item's ending is told to; none where no agent's turn filed
  /// it.
  pub agent: Option<&'a str>,
}

/// The priority an item filed now takes: behind every unfinished one.
pub async fn next_priority(db: &impl ConnectionTrait) -> Result<i64, Refusal> {
  let row = one(
    db,
    r#"SELECT COALESCE(MAX("priority"), 0) AS "last" FROM "todos" WHERE "done" = 0"#,
    vec![],
  )
  .await?;

  Ok(row.map(|row| row.try_get::<i64>("", "last")).transpose()?.unwrap_or(0) + 1)
}

/// The item written, with the links to the notes it came out of, in one transaction; its number.
pub async fn add(db: &impl TransactionTrait, item: &NewItem<'_>) -> Result<i64, Refusal> {
  let unit = db.begin().await?;
  let priority = next_priority(&unit).await?;
  let id: i64 = one(
    &unit,
    r#"INSERT INTO "todos" ("title", "instruction", "requester", "requester_id", "chat_id", "approved", "done", "priority",
         "status", "model", "created_at", "agent")
       VALUES ($1, $2, $3, $4, $5, 0, 0, $6, $7, $8, $9, $10)
       RETURNING "id""#,
    vec![
      item.title.into(),
      item.instruction.into(),
      item.requester.into(),
      item.requester_id.into(),
      item.chat_id.into(),
      priority.into(),
      item.status.into(),
      item.model.into(),
      item.created_at.into(),
      item.agent.into(),
    ],
  )
  .await?
  .ok_or_else(|| not_written("the item"))?
  .try_get("", "id")?;

  for &source in item.sources.iter().filter(|&&source| source != id) {
    run(
      &unit,
      r#"INSERT INTO "todo_sources" ("todo_id", "source_todo_id") VALUES ($1, $2) ON CONFLICT DO NOTHING"#,
      vec![id.into(), source.into()],
    )
    .await?;
  }

  unit.commit().await?;

  Ok(id)
}

/// The item's title and instruction written again.
pub async fn rewrite(
  db: &impl ConnectionTrait,
  id: i64,
  title: &str,
  instruction: &str,
) -> Result<(), Refusal> {
  run(
    db,
    r#"UPDATE "todos" SET "title" = $1, "instruction" = $2 WHERE "id" = $3"#,
    vec![title.into(), instruction.into(), id.into()],
  )
  .await
  .map(drop)
}

/// The columns a verb sets on their own.
#[derive(Clone, Copy, Debug)]
pub enum Column {
  Status,
  Model,
  Repairs,
}

impl Column {
  fn name(self) -> &'static str {
    match self {
      Self::Status => "status",
      Self::Model => "model",
      Self::Repairs => "repairs_todo_id",
    }
  }
}

/// One column of the item set: its status line, its model or the item it repairs.
pub async fn set(
  db: &impl ConnectionTrait,
  id: i64,
  column: Column,
  value: sea_orm::Value,
) -> Result<(), Refusal> {
  run(
    db,
    &format!(r#"UPDATE "todos" SET "{}" = $1 WHERE "id" = $2"#, column.name()),
    vec![value, id.into()],
  )
  .await
  .map(drop)
}

/// The item finished with its report and unfinished note, and every item merged into it closed with it
/// under `carried_report`; the numbers of those. An unfinished note the closing changed stands open again.
pub async fn finish(
  db: &impl TransactionTrait,
  id: i64,
  now: i64,
  report: Option<&str>,
  unfinished: Option<&str>,
  carried_report: &str,
) -> Result<Vec<i64>, Refusal> {
  let unit = db.begin().await?;

  run(
    &unit,
    r#"UPDATE "todos" SET "done" = 1, "status" = $1, "done_at" = $2, "report" = $3,
         "unfinished_resolved_at" = CASE WHEN "unfinished" IS NOT DISTINCT FROM $4 THEN "unfinished_resolved_at" ELSE NULL END,
         "unfinished_resolution" = CASE WHEN "unfinished" IS NOT DISTINCT FROM $4 THEN "unfinished_resolution" ELSE NULL END,
         "unfinished" = $4
       WHERE "id" = $5"#,
    vec![DONE_STATUS.into(), now.into(), report.into(), unfinished.into(), id.into()],
  )
  .await?;

  let carried = all(
    &unit,
    r#"UPDATE "todos" SET "done" = 1, "status" = $1, "done_at" = $2, "report" = $3 WHERE "merged_into" = $4 AND "done" = 0 RETURNING "id""#,
    vec![DONE_STATUS.into(), now.into(), carried_report.into(), id.into()],
  )
  .await?;

  unit.commit().await?;

  ids(&carried)
}

/// A finished item opened again, taken out of any merge, and the items merged into it opened with it; the
/// numbers of those.
pub async fn reopen(db: &impl TransactionTrait, id: i64) -> Result<Vec<i64>, Refusal> {
  let unit = db.begin().await?;
  let reopen = r#"UPDATE "todos" SET "done" = 0, "done_at" = NULL, "report" = NULL, "unfinished" = NULL,
         "unfinished_resolved_at" = NULL, "unfinished_resolution" = NULL, "status" = $1"#;

  run(
    &unit,
    &format!(r#"{reopen}, "merged_into" = NULL, "merged_at" = NULL WHERE "id" = $2"#),
    vec![WAITING_STATUS.into(), id.into()],
  )
  .await?;

  let carried = all(
    &unit,
    &format!(r#"{reopen} WHERE "merged_into" = $2 AND "done" = 1 RETURNING "id""#),
    vec![WAITING_STATUS.into(), id.into()],
  )
  .await?;

  unit.commit().await?;

  ids(&carried)
}

/// Why a merge was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeRefusal {
  Missing(i64),
  Itself(i64),
  Done(i64),
  Merged { id: i64, into: i64 },
  Running(i64),
}

/// Items merged into another, each one's own merges carried to the target; none written where any is
/// refused.
pub async fn merge(
  db: &DatabaseConnection,
  ids: &[i64],
  into: i64,
  now: i64,
) -> Result<Option<MergeRefusal>, Refusal> {
  let Some(target) = row(db, into).await? else {
    return Ok(Some(MergeRefusal::Missing(into)));
  };

  if target.done {
    return Ok(Some(MergeRefusal::Done(into)));
  }

  if let Some(further) = target.merged_into {
    return Ok(Some(MergeRefusal::Merged {
      id: into,
      into: further,
    }));
  }

  for &id in ids {
    let Some(item) = row(db, id).await? else {
      return Ok(Some(MergeRefusal::Missing(id)));
    };

    if id == into {
      return Ok(Some(MergeRefusal::Itself(id)));
    }

    if item.done {
      return Ok(Some(MergeRefusal::Done(id)));
    }

    if let Some(further) = item.merged_into {
      return Ok(Some(MergeRefusal::Merged { id, into: further }));
    }

    if crate::todos::running_job_for(db, id).await?.is_some() {
      return Ok(Some(MergeRefusal::Running(id)));
    }
  }

  let unit = db.begin().await?;

  for &id in ids {
    run(
      &unit,
      r#"UPDATE "todos" SET "merged_into" = $1 WHERE "merged_into" = $2"#,
      vec![into.into(), id.into()],
    )
    .await?;
    run(
      &unit,
      r#"UPDATE "todos" SET "merged_into" = $1, "merged_at" = $2, "held_at" = NULL, "held_reason" = NULL, "held_kind" = NULL,
           "resume_after" = NULL, "resume_after_deploy" = NULL WHERE "id" = $3"#,
      vec![into.into(), now.into(), id.into()],
    )
    .await?;
  }

  unit.commit().await?;

  Ok(None)
}

/// An item merged into another taken back out as work of its own; whether it was merged and unfinished.
pub async fn unmerge(db: &impl ConnectionTrait, id: i64) -> Result<bool, Refusal> {
  Ok(
    run(
      db,
      r#"UPDATE "todos" SET "merged_into" = NULL, "merged_at" = NULL WHERE "id" = $1 AND "merged_into" IS NOT NULL AND "done" = 0"#,
      vec![id.into()],
    )
    .await?
      > 0,
  )
}

/// The item wound down to resume later, with its reason and the item it waits on where it waits on one;
/// whether it landed. An item the owner stopped stays stopped.
pub async fn wind_down(
  db: &impl ConnectionTrait,
  id: i64,
  reason: &str,
  resume_after: Option<i64>,
  now: i64,
) -> Result<bool, Refusal> {
  Ok(
    run(
      db,
      r#"UPDATE "todos" SET "held_at" = COALESCE("held_at", $1), "held_reason" = $2, "held_kind" = 'pause', "resume_after" = $3,
           "resume_after_deploy" = NULL
         WHERE "id" = $4 AND "done" = 0 AND "merged_into" IS NULL AND ("held_kind" IS NULL OR "held_kind" <> 'stop')"#,
      vec![now.into(), reason.into(), resume_after.into(), id.into()],
    )
    .await?
      > 0,
  )
}

/// The commits a held item waits to see live; whether it was held to take them.
pub async fn hold_until_deployed(
  db: &impl ConnectionTrait,
  id: i64,
  commits: &[String],
) -> Result<bool, Refusal> {
  Ok(
    run(
      db,
      r#"UPDATE "todos" SET "resume_after_deploy" = $1, "resume_after" = NULL WHERE "id" = $2 AND "done" = 0 AND "held_at" IS NOT NULL"#,
      vec![commits.join(" ").into(), id.into()],
    )
    .await?
      > 0,
  )
}

/// The held items waiting for commits to go live, each with the commits it waits for.
pub async fn awaiting_deploy(db: &impl ConnectionTrait) -> Result<Vec<(Row, Vec<String>)>, Refusal> {
  let found = all(
    db,
    r#"SELECT * FROM "todos" WHERE "resume_after_deploy" IS NOT NULL AND "held_at" IS NOT NULL AND "done" = 0
       AND "merged_into" IS NULL ORDER BY "id""#,
    vec![],
  )
  .await?;

  found
    .iter()
    .map(|row| {
      let commits: String = row.try_get("", "resume_after_deploy")?;

      Ok((
        Row::of(row)?,
        commits.split(' ').filter(|commit| !commit.is_empty()).map(str::to_owned).collect(),
      ))
    })
    .collect()
}

/// A condition the agent was told is met comes off the row, so the next deployment does not tell it again;
/// the hold and its reason stay.
pub async fn clear_resume_after_deploy(db: &impl ConnectionTrait, ids: &[i64]) -> Result<(), Refusal> {
  for &id in ids {
    run(
      db,
      r#"UPDATE "todos" SET "resume_after_deploy" = NULL WHERE "id" = $1"#,
      vec![id.into()],
    )
    .await?;
  }

  Ok(())
}

/// The items wound down to wait on this one, which its finishing makes ready.
pub async fn resuming_after(db: &impl ConnectionTrait, id: i64) -> Result<Vec<Row>, Refusal> {
  rows(
    &all(
      db,
      r#"SELECT * FROM "todos" WHERE "resume_after" = $1 AND "held_kind" = 'pause' AND "done" = 0 ORDER BY "id""#,
      vec![id.into()],
    )
    .await?,
  )
}

/// The rows an unfinished note was broken into, each with its title and whether it is finished.
pub async fn raised_from(
  db: &impl ConnectionTrait,
  source: i64,
) -> Result<Vec<(i64, String, bool)>, Refusal> {
  let found = all(
    db,
    r#"SELECT "todos"."id" AS "id", "todos"."title" AS "title", "todos"."done" AS "done"
       FROM "todo_sources" JOIN "todos" ON "todos"."id" = "todo_sources"."todo_id"
       WHERE "todo_sources"."source_todo_id" = $1 ORDER BY "todos"."id""#,
    vec![source.into()],
  )
  .await?;

  found
    .iter()
    .map(|row| {
      Ok((row.try_get("", "id")?, row.try_get("", "title")?, row.try_get::<i64>("", "done")? == 1))
    })
    .collect()
}

/// The items filed to repair this one.
pub async fn repairs_of(db: &impl ConnectionTrait, id: i64) -> Result<Vec<Row>, Refusal> {
  rows(
    &all(
      db,
      r#"SELECT * FROM "todos" WHERE "repairs_todo_id" = $1 ORDER BY "id""#,
      vec![id.into()],
    )
    .await?,
  )
}

/// The item a row was filed to repair.
pub async fn repairs_target(db: &impl ConnectionTrait, id: i64) -> Result<Option<i64>, Refusal> {
  Ok(
    one(db, r#"SELECT "repairs_todo_id" FROM "todos" WHERE "id" = $1"#, vec![id.into()])
      .await?
      .map(|row| row.try_get::<Option<i64>>("", "repairs_todo_id"))
      .transpose()?
      .flatten(),
  )
}

/// One of the words added to an item, standing or taken back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Note {
  pub id: i64,
  pub author: String,
  pub text: String,
  pub added_at: i64,
  pub removed_at: Option<i64>,
  pub removed_by: Option<String>,
  /// The agent's change to the brief where it is one, none for the words people and jobs add.
  pub kind: Option<String>,
  /// When a running job was handed them, where one was.
  pub delivered_at: Option<i64>,
  /// Whether the job they came with is one of the item's own: the job that wrote them, or the one at work
  /// on the item when somebody else added them.
  pub with_items_job: bool,
}

/// Every word ever added to an item, taken back or not, oldest first.
pub async fn every_note(db: &impl ConnectionTrait, todo: i64) -> Result<Vec<Note>, Refusal> {
  let found = all(
    db,
    r#"SELECT "todo_notes".*, "jobs"."todo_id" AS "job_todo"
       FROM "todo_notes" LEFT JOIN "jobs" ON "jobs"."id" = "todo_notes"."job_id"
       WHERE "todo_notes"."todo_id" = $1 ORDER BY "todo_notes"."id""#,
    vec![todo.into()],
  )
  .await?;

  found
    .iter()
    .map(|row| {
      Ok(Note {
        id: row.try_get("", "id")?,
        author: row.try_get("", "author")?,
        text: row.try_get("", "text")?,
        added_at: row.try_get("", "added_at")?,
        removed_at: row.try_get("", "removed_at")?,
        removed_by: row.try_get("", "removed_by")?,
        kind: row.try_get("", "kind")?,
        delivered_at: row.try_get("", "delivered_at")?,
        with_items_job: row.try_get::<Option<i64>>("", "job_todo")? == Some(todo),
      })
    })
    .collect()
}

/// Who added words: the name they stand under and, for a person known as a sender, that sender.
#[derive(Clone, Copy, Debug)]
pub struct Author<'a> {
  pub name: &'a str,
  pub sender: Option<i64>,
}

/// Words added to an item, with the job they landed in the middle of; their number.
pub async fn add_note(
  db: &impl ConnectionTrait,
  todo: i64,
  author: Author<'_>,
  text: &str,
  job: Option<i64>,
  now: i64,
) -> Result<i64, Refusal> {
  insert_note(db, todo, author, text, None, job, now).await
}

/// The agent's change to an item's brief, kept under the agent's name as a note of its own kind with the
/// job it was handed to; its number.
pub async fn add_change(
  db: &impl ConnectionTrait,
  todo: i64,
  agent: &str,
  text: &str,
  job: Option<i64>,
  now: i64,
) -> Result<i64, Refusal> {
  let author = Author {
    name: agent,
    sender: None,
  };

  insert_note(db, todo, author, text, Some(CHANGE_NOTE), job, now).await
}

async fn insert_note(
  db: &impl ConnectionTrait,
  todo: i64,
  author: Author<'_>,
  text: &str,
  kind: Option<&str>,
  job: Option<i64>,
  now: i64,
) -> Result<i64, Refusal> {
  Ok(
    one(
      db,
      r#"INSERT INTO "todo_notes" ("todo_id", "author", "author_id", "text", "added_at", "job_id", "kind")
         VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING "id""#,
      vec![
        todo.into(),
        author.name.into(),
        author.sender.into(),
        text.into(),
        now.into(),
        job.into(),
        kind.into(),
      ],
    )
    .await?
    .ok_or_else(|| not_written("the words"))?
    .try_get("", "id")?,
  )
}

/// Words taken back and marked so; for the ones that stood, whether they may be in a run already.
///
/// Which of the agent's changes carries which words is not recorded, so words count as carried once a change
/// made after them has been handed to a job, or once they carry a handing time of their own.
pub async fn remove_note(
  db: &impl ConnectionTrait,
  todo: i64,
  note: i64,
  by: &str,
  now: i64,
) -> Result<Option<bool>, Refusal> {
  let Some(row) = one(
    db,
    r#"UPDATE "todo_notes" SET "removed_at" = $1, "removed_by" = $2 WHERE "id" = $3 AND "todo_id" = $4 AND "removed_at" IS NULL
       RETURNING "delivered_at", "added_at""#,
    vec![now.into(), by.into(), note.into(), todo.into()],
  )
  .await?
  else {
    return Ok(None);
  };

  if row.try_get::<Option<i64>>("", "delivered_at")?.is_some() {
    return Ok(Some(true));
  }

  let added: i64 = row.try_get("", "added_at")?;
  let handed = one(
    db,
    r#"SELECT 1 AS "handed" FROM "todo_notes" WHERE "todo_id" = $1 AND "kind" = $2 AND "delivered_at" IS NOT NULL
       AND "added_at" > $3 LIMIT 1"#,
    vec![todo.into(), CHANGE_NOTE.into(), added.into()],
  )
  .await?;

  Ok(Some(handed.is_some()))
}

/// Words marked handed over, so the run that wrote them is not handed them back.
pub async fn mark_delivered(db: &impl ConnectionTrait, notes: &[i64], now: i64) -> Result<(), Refusal> {
  for &note in notes {
    run(
      db,
      r#"UPDATE "todo_notes" SET "delivered_at" = $1 WHERE "id" = $2"#,
      vec![now.into(), note.into()],
    )
    .await?;
  }

  Ok(())
}
