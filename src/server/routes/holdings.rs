//! What the service holds of a room bmo-receiver is about to remove (docs/api.md, "A room's holdings"):
//! the open items filed from it and the questions of its jobs still waiting, each named only where it holds
//! any.

use std::sync::Arc;

use axum::Router;
use axum::extract::Path;
use axum::routing::get;
use job_base::Refusal;
use job_codes::RoomHoldings;
use sea_orm::ConnectionTrait;

use super::{Answered, State, body};
use crate::one;
use crate::server::Server;
use crate::verbs::Verbs;

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/rooms/{chat}/holdings", get(holdings::<V>))
}

async fn count(db: &impl ConnectionTrait, sql: &str, chat: i64) -> Result<u32, Refusal> {
  let counted = match one(db, sql, vec![chat.into()]).await? {
    Some(row) => row.try_get::<i64>("", "n")?,
    None => 0,
  };

  Ok(u32::try_from(counted).unwrap_or(u32::MAX))
}

async fn holdings<V: Verbs>(state: State<V>, Path(chat): Path<i64>) -> Answered {
  let items = count(
    &state.db,
    r#"SELECT count(*) AS "n" FROM "todos" WHERE "chat_id" = $1 AND "done" = 0 AND "merged_into" IS NULL"#,
    chat,
  )
  .await?;
  let questions = count(
    &state.db,
    r#"SELECT count(*) AS "n" FROM "job_questions" WHERE "chat_id" = $1 AND "settled_at" IS NULL"#,
    chat,
  )
  .await?;
  let held = [
    (items > 0).then_some(RoomHoldings::RoomOpenItems { count: items }),
    (questions > 0).then_some(RoomHoldings::RoomWaitingQuestions { count: questions }),
  ];

  body(held.into_iter().flatten().collect::<Vec<_>>())
}
