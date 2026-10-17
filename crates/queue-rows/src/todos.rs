//! The items of the queue as the `todos`, `todo_sources` and `todo_approvals` tables hold them, read and
//! written the way the queue's routes change them (docs/rows.md, "The rows"): approved and its approval
//! recorded, ordered, assigned, weighed, held and let go, resolved, thrown out and raised. An item is never
//! deleted for being finished; throwing out a request that should never have been filed is the one way a
//! row leaves.

use job_base::{Refusal, all, one, run};
use queue_read::impact::ImpactGrounds;
use sea_orm::{ConnectionTrait, QueryResult, TransactionTrait};

/// What an item's status line says while nobody is doing it yet.
pub const WAITING_STATUS: &str = "waiting";

/// What an item's status line says once it is finished.
pub const DONE_STATUS: &str = "done";

/// The first words of the line a wind-down puts on its item for the run it interrupts; letting the hold
/// go withdraws the notes that open with it.
pub const WIND_DOWN_NOTE_MARK: &str = "[Stop asked]";

/// Who a note withdrawn by a hold let go is marked as taken back by.
const RESUMED_BY: &str = "resumed";

/// The way an item's approval came, which a launch reads to say whose work it is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Approval {
  /// The owner's own word.
  Owner,
  /// A standing rule's holder's message, under a rule only the owner writes.
  Rule,
  /// Anyone else's `approve` grant.
  Grant,
}

impl Approval {
  /// The word the row keeps it by.
  pub fn word(self) -> &'static str {
    match self {
      Self::Owner => "owner",
      Self::Rule => "rule",
      Self::Grant => "grant",
    }
  }

  /// The way a row's word names, where it names one.
  pub fn of(word: &str) -> Option<Self> {
    [Self::Owner, Self::Rule, Self::Grant].into_iter().find(|way| way.word() == word)
  }
}

/// Who gave an approval or took it back, and by which road.
pub struct Hand<'a> {
  pub who: Who<'a>,
  pub road: Road,
}

/// Whose hand it was, by the name each kind is known by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who<'a> {
  /// A person, by their console login or the machine's user at a shell.
  Person(&'a str),
  /// An agent, by its turn's name.
  Agent(&'a str),
  /// A job, by its run.
  Job(&'a str),
}

impl Who<'_> {
  /// The word the record keeps its kind by.
  pub fn kind(self) -> &'static str {
    match self {
      Self::Person(_) => "person",
      Self::Agent(_) => "agent",
      Self::Job(_) => "job",
    }
  }

  pub fn name(self) -> String {
    match self {
      Self::Person(name) | Self::Agent(name) | Self::Job(name) => name.to_owned(),
    }
  }
}

/// The road a change of an approval came by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Road {
  /// The web console, through the API.
  Console,
  /// The command line's approve verb.
  Verb,
  /// A chat tool.
  ChatTool,
}

impl Road {
  /// The word the record keeps it by.
  pub fn word(self) -> &'static str {
    match self {
      Self::Console => "console",
      Self::Verb => "verb",
      Self::ChatTool => "chat_tool",
    }
  }
}

/// One item, as much of it as the queue's routes read.
#[derive(Clone, Debug)]
pub struct Item {
  pub id: i64,
  pub title: String,
  pub instruction: String,
  pub approved: bool,
  /// The way its approval came; none on a row not approved.
  pub approved_via: Option<Approval>,
  pub done: bool,
  pub held_at: Option<i64>,
  pub held_reason: Option<String>,
  pub model: Option<String>,
  pub worker: Option<String>,
  pub cowork: bool,
  /// The grounds its co-work setting was weighed on, where it was filed with any that read.
  pub impact: Option<ImpactGrounds>,
  pub requester_id: Option<i64>,
  pub priority: i64,
  pub unfinished: Option<String>,
  pub merged_into: Option<i64>,
}

impl Item {
  fn of(row: &QueryResult) -> Result<Self, Refusal> {
    Ok(Self {
      id: row.try_get("", "id")?,
      title: row.try_get("", "title")?,
      instruction: row.try_get("", "instruction")?,
      approved: row.try_get::<i64>("", "approved")? == 1,
      approved_via: row.try_get::<Option<String>>("", "approved_via")?.as_deref().and_then(Approval::of),
      done: row.try_get::<i64>("", "done")? == 1,
      held_at: row.try_get("", "held_at")?,
      held_reason: row.try_get("", "held_reason")?,
      model: row.try_get("", "model")?,
      worker: row.try_get("", "worker")?,
      cowork: row.try_get::<Option<i64>>("", "cowork")?.unwrap_or_default() == 1,
      impact: row
        .try_get::<Option<String>>("", "impact")?
        .and_then(|written| ImpactGrounds::read(&written).ok()),
      requester_id: row.try_get("", "requester_id")?,
      priority: row.try_get("", "priority")?,
      unfinished: row.try_get("", "unfinished")?,
      merged_into: row.try_get("", "merged_into")?,
    })
  }

  /// Whether its work is carried by another item that is not finished yet.
  pub fn merged(&self) -> bool {
    self.merged_into.is_some() && !self.done
  }
}

pub async fn item(db: &impl ConnectionTrait, id: i64) -> Result<Option<Item>, Refusal> {
  one(db, r#"SELECT * FROM "todos" WHERE "id" = $1"#, vec![id.into()])
    .await?
    .as_ref()
    .map(Item::of)
    .transpose()
}

/// The queue: what is not done and what the owner approved, neither held nor merged, in his order.
pub async fn open_items(db: &impl ConnectionTrait) -> Result<Vec<Item>, Refusal> {
  let found = all(
    db,
    r#"SELECT * FROM "todos" WHERE "done" = 0 AND "approved" = 1 AND "held_at" IS NULL AND "merged_into" IS NULL
       ORDER BY "priority" ASC, "id" ASC"#,
    vec![],
  )
  .await?;

  found.iter().map(Item::of).collect()
}

/// Approving, the way it came, carries whatever the owner said with it; taking an approval back, as no
/// way, clears the way and touches nothing else. Either is kept in the item's approval record with the hand
/// that made it, in the same transaction.
pub async fn approve(
  db: &impl TransactionTrait,
  id: i64,
  approval: Option<Approval>,
  note: Option<&str>,
  hand: &Hand<'_>,
  now: i64,
) -> Result<(), Refusal> {
  let unit = db.begin().await?;

  match approval {
    Some(way) => {
      run(
        &unit,
        r#"UPDATE "todos" SET "approved" = 1, "approved_via" = $1, "note" = $2 WHERE "id" = $3"#,
        vec![way.word().into(), note.into(), id.into()],
      )
      .await?
    }
    None => {
      run(
        &unit,
        r#"UPDATE "todos" SET "approved" = 0, "approved_via" = NULL WHERE "id" = $1"#,
        vec![id.into()],
      )
      .await?
    }
  };

  run(
    &unit,
    r#"INSERT INTO "todo_approvals" ("todo_id", "approved", "via", "who_kind", "who", "road", "at")
       VALUES ($1, $2, $3, $4, $5, $6, $7)"#,
    vec![
      id.into(),
      i64::from(approval.is_some()).into(),
      approval.map(Approval::word).into(),
      hand.who.kind().into(),
      hand.who.name().into(),
      hand.road.word().into(),
      now.into(),
    ],
  )
  .await?;
  unit.commit().await?;

  Ok(())
}

pub async fn set_priority(db: &impl ConnectionTrait, id: i64, priority: i64) -> Result<(), Refusal> {
  run(
    db,
    r#"UPDATE "todos" SET "priority" = $1 WHERE "id" = $2"#,
    vec![priority.into(), id.into()],
  )
  .await
  .map(drop)
}

/// Which worker is to do the item; none puts it back to the built-in worker.
pub async fn set_worker(db: &impl ConnectionTrait, id: i64, worker: Option<&str>) -> Result<(), Refusal> {
  run(
    db,
    r#"UPDATE "todos" SET "worker" = $1 WHERE "id" = $2"#,
    vec![worker.into(), id.into()],
  )
  .await
  .map(drop)
}

pub async fn set_cowork(db: &impl ConnectionTrait, id: i64, cowork: bool) -> Result<(), Refusal> {
  run(
    db,
    r#"UPDATE "todos" SET "cowork" = $1 WHERE "id" = $2"#,
    vec![i64::from(cowork).into(), id.into()],
  )
  .await
  .map(drop)
}

/// The grounds the item's co-work setting is weighed on.
pub async fn set_impact(
  db: &impl ConnectionTrait,
  id: i64,
  grounds: &ImpactGrounds,
) -> Result<(), Refusal> {
  run(
    db,
    r#"UPDATE "todos" SET "impact" = $1 WHERE "id" = $2"#,
    vec![grounds.written().into(), id.into()],
  )
  .await
  .map(drop)
}

/// The grounds a job of the item weighed again, kept beside the ones its setting was weighed on.
pub async fn set_job_impact(
  db: &impl ConnectionTrait,
  id: i64,
  grounds: &ImpactGrounds,
) -> Result<(), Refusal> {
  run(
    db,
    r#"UPDATE "todos" SET "job_impact" = $1 WHERE "id" = $2"#,
    vec![grounds.written().into(), id.into()],
  )
  .await
  .map(drop)
}

/// The item's brief, the agent's organized English instruction its every later reading for a worker reads.
/// Its rendering in the owner's language is cleared with it; a brief that did not change keeps the
/// rendering it has. Whether it changed.
pub async fn set_brief(db: &impl ConnectionTrait, id: i64, brief: &str) -> Result<bool, Refusal> {
  let changed = run(
    db,
    r#"UPDATE "todos" SET "brief" = $1, "brief_korean" = NULL WHERE "id" = $2 AND "brief" IS DISTINCT FROM $1"#,
    vec![brief.into(), id.into()],
  )
  .await?;

  Ok(changed > 0)
}

/// The item set aside, where it is neither finished, merged nor held already; whether it landed.
pub async fn hold(
  db: &impl ConnectionTrait,
  id: i64,
  reason: Option<&str>,
  now: i64,
) -> Result<bool, Refusal> {
  hold_as(db, id, reason, None, now).await
}

/// The item set aside as a hold of a kind, `failure` where its runs set it aside, where it is neither
/// finished, merged nor held already; whether it landed.
pub async fn hold_as(
  db: &impl ConnectionTrait,
  id: i64,
  reason: Option<&str>,
  kind: Option<&str>,
  now: i64,
) -> Result<bool, Refusal> {
  let held = run(
    db,
    r#"UPDATE "todos" SET "held_at" = $1, "held_reason" = $2, "held_kind" = $3
       WHERE "id" = $4 AND "done" = 0 AND "held_at" IS NULL AND "merged_into" IS NULL"#,
    vec![now.into(), reason.into(), kind.into(), id.into()],
  )
  .await?;

  Ok(held > 0)
}

/// The item held as the owner's stop with his reason, where it is neither finished nor merged; a hold
/// already standing keeps when it began. Whether it landed.
pub async fn stop_hold(
  db: &impl ConnectionTrait,
  id: i64,
  reason: &str,
  now: i64,
) -> Result<bool, Refusal> {
  let held = run(
    db,
    r#"UPDATE "todos" SET "held_at" = COALESCE("held_at", $1), "held_reason" = $2, "held_kind" = 'stop', "resume_after" = NULL,
         "resume_after_deploy" = NULL
       WHERE "id" = $3 AND "done" = 0 AND "merged_into" IS NULL"#,
    vec![now.into(), reason.into(), id.into()],
  )
  .await?;

  Ok(held > 0)
}

/// The item taken back to where it stood, with its resume condition, and the line a wind-down put on it for
/// the run it interrupted withdrawn; whether it was set aside at all.
pub async fn release(db: &impl TransactionTrait, id: i64, now: i64) -> Result<bool, Refusal> {
  let unit = db.begin().await?;
  let released = run(
    &unit,
    r#"UPDATE "todos" SET "held_at" = NULL, "held_reason" = NULL, "held_kind" = NULL, "resume_after" = NULL,
         "resume_after_deploy" = NULL
       WHERE "id" = $1 AND "held_at" IS NOT NULL"#,
    vec![id.into()],
  )
  .await?;

  if released == 0 {
    return Ok(false);
  }

  run(
    &unit,
    r#"UPDATE "todo_notes" SET "removed_at" = $1, "removed_by" = $2
       WHERE "todo_id" = $3 AND "removed_at" IS NULL AND substr("text", 1, length($4)) = $4"#,
    vec![now.into(), RESUMED_BY.into(), id.into(), WIND_DOWN_NOTE_MARK.into()],
  )
  .await?;
  unit.commit().await?;

  Ok(true)
}

/// An unfinished note resolved with one line of why; refused where the item left none or it is resolved
/// already.
pub async fn resolve_unfinished(
  db: &impl ConnectionTrait,
  id: i64,
  why: &str,
  now: i64,
) -> Result<bool, Refusal> {
  let resolved = run(
    db,
    r#"UPDATE "todos" SET "unfinished_resolved_at" = $1, "unfinished_resolution" = $2
       WHERE "id" = $3 AND "unfinished" IS NOT NULL AND "unfinished_resolved_at" IS NULL"#,
    vec![now.into(), why.into(), id.into()],
  )
  .await?;

  Ok(resolved > 0)
}

/// A resolution that stopped being true taken back; whether there was one.
pub async fn reopen_unfinished(db: &impl ConnectionTrait, id: i64) -> Result<bool, Refusal> {
  let reopened = run(
    db,
    r#"UPDATE "todos" SET "unfinished_resolved_at" = NULL, "unfinished_resolution" = NULL
       WHERE "id" = $1 AND "unfinished_resolved_at" IS NOT NULL"#,
    vec![id.into()],
  )
  .await?;

  Ok(reopened > 0)
}

/// The notes a row was raised out of, by the item that left each.
pub async fn sources(db: &impl ConnectionTrait, id: i64) -> Result<Vec<i64>, Refusal> {
  let found = all(
    db,
    r#"SELECT "source_todo_id" AS "source" FROM "todo_sources" WHERE "todo_id" = $1 ORDER BY "source_todo_id""#,
    vec![id.into()],
  )
  .await?;

  Ok(found.iter().map(|row| row.try_get("", "source")).collect::<Result<_, _>>()?)
}

/// A request that should never have been filed, thrown out with the links it stood in either way and its
/// work order; the items merged into it stand as work of their own again.
pub async fn drop_item(db: &impl TransactionTrait, id: i64) -> Result<(), Refusal> {
  let unit = db.begin().await?;

  run(
    &unit,
    r#"UPDATE "todos" SET "merged_into" = NULL, "merged_at" = NULL WHERE "merged_into" = $1"#,
    vec![id.into()],
  )
  .await?;
  run(
    &unit,
    r#"DELETE FROM "todo_sources" WHERE "todo_id" = $1 OR "source_todo_id" = $1"#,
    vec![id.into()],
  )
  .await?;
  run(&unit, r#"DELETE FROM "work_orders" WHERE "todo_id" = $1"#, vec![id.into()]).await?;
  run(&unit, r#"DELETE FROM "todos" WHERE "id" = $1"#, vec![id.into()]).await?;
  unit.commit().await?;

  Ok(())
}

/// A raised row, waiting with the other requests: the owner's, raised from the console, or the agent's,
/// raised by a watch, which names no person as its requester.
pub struct Raised<'a> {
  pub title: &'a str,
  pub instruction: &'a str,
  pub requester: &'a str,
  pub requester_id: Option<i64>,
  pub chat_id: i64,
  pub created_at: i64,
  pub sources: &'a [i64],
}

/// The row written behind everything already waiting, with the links to the notes it came out of; its
/// number.
pub async fn add_raised(db: &impl TransactionTrait, raised: &Raised<'_>) -> Result<i64, Refusal> {
  crate::store::add(
    db,
    &crate::store::NewItem {
      title: raised.title,
      instruction: raised.instruction,
      requester: raised.requester,
      requester_id: raised.requester_id,
      chat_id: Some(raised.chat_id),
      status: WAITING_STATUS,
      model: "auto",
      created_at: raised.created_at,
      sources: raised.sources,
      agent: None,
    },
  )
  .await
}

/// The job at work on the item now, where one is: the last of its rows not yet finished.
pub async fn running_job_for(db: &impl ConnectionTrait, id: i64) -> Result<Option<i64>, Refusal> {
  let row = one(
    db,
    r#"SELECT "id" FROM "jobs" WHERE "finished_at" IS NULL AND "todo_id" = $1 ORDER BY "id" DESC LIMIT 1"#,
    vec![id.into()],
  )
  .await?;

  Ok(row.map(|row| row.try_get("", "id")).transpose()?)
}

/// The job a run is at work for, where there is one: its row and the item it runs, where it runs one.
pub async fn at_work_for(
  db: &impl ConnectionTrait,
  run: &str,
) -> Result<Option<(i64, Option<i64>)>, Refusal> {
  let row = one(
    db,
    r#"SELECT "id", "todo_id" FROM "jobs" WHERE "finished_at" IS NULL AND "unit" = $1 ORDER BY "id" DESC LIMIT 1"#,
    vec![run.into()],
  )
  .await?;

  row
    .map(|row| -> Result<(i64, Option<i64>), Refusal> {
      Ok((row.try_get("", "id")?, row.try_get("", "todo_id")?))
    })
    .transpose()
}
