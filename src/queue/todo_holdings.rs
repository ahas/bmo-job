//! What a room holds of the queue, as the service answers bmo-receiver before it removes the room
//! (docs/api.md, "A room's holdings"): the items filed from it and the jobs run for it stay, keeping the
//! room as where they came from, and the open items and the questions still waiting are what a removal
//! warns of.

use sea_orm::ConnectionTrait;

use crate::{Refusal, one};

async fn count(db: &impl ConnectionTrait, sql: &str, chat: i64) -> Result<u32, Refusal> {
  let counted = match one(db, sql, vec![chat.into()]).await? {
    Some(row) => row.try_get::<i64>("", "n")?,
    None => 0,
  };

  Ok(u32::try_from(counted).unwrap_or(u32::MAX))
}

/// The items filed from a room that are not finished nor merged.
pub async fn open_items(db: &impl ConnectionTrait, chat: i64) -> Result<u32, Refusal> {
  count(
    db,
    r#"SELECT count(*) AS "n" FROM "todos" WHERE "chat_id" = $1 AND "done" = 0 AND "merged_into" IS NULL"#,
    chat,
  )
  .await
}

/// The questions asked in a room that are not settled.
pub async fn waiting_questions(db: &impl ConnectionTrait, chat: i64) -> Result<u32, Refusal> {
  count(
    db,
    r#"SELECT count(*) AS "n" FROM "job_questions" WHERE "chat_id" = $1 AND "settled_at" IS NULL"#,
    chat,
  )
  .await
}
