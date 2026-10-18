//! The work a worker session holds (docs/api.md, "The work a worker session holds"): the item it last
//! served and the co-work results it was sent in full, so its next turn sends only what changed and a fold
//! forgets what it folded away.

use std::sync::Arc;

use axum::Router;
use axum::extract::Path;
use axum::routing::{delete, get, put};
use serde::Serialize;

use super::{Answered, State, body, nothing};
use crate::server::Server;
use crate::server::request::{Body, Fields};
use crate::verbs::Verbs;
use crate::{all, now_ms, one, run};

/// How long an item served or a result sent is kept after the latest write: seven days.
const KEPT_MS: i64 = 7 * 86_400 * 1000;

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new()
    .route("/sessions/{session}/work", get(work::<V>))
    .route("/sessions/{session}/item", put(served::<V>))
    .route("/sessions/{session}/results/{result}", put(sent::<V>))
    .route("/sessions/{session}/results", delete(folded::<V>))
}

#[derive(Serialize)]
struct Work {
  item: Option<i64>,
  results: Vec<SentResult>,
}

/// A co-work result a session was sent in full.
#[derive(Serialize)]
struct SentResult {
  result: String,
  version: String,
  loaded_at: i64,
}

async fn work<V: Verbs>(state: State<V>, Path(session): Path<String>) -> Answered {
  let item = one(
    &state.db,
    r#"SELECT "todo_id" FROM "session_work_items" WHERE "session_id" = $1"#,
    vec![session.clone().into()],
  )
  .await?
  .map(|row| row.try_get::<i64>("", "todo_id"))
  .transpose()?;
  let rows = all(
    &state.db,
    r#"SELECT "result", "version", "loaded_at" FROM "session_work_results" WHERE "session_id" = $1 ORDER BY "result""#,
    vec![session.into()],
  )
  .await?;
  let results = rows
    .iter()
    .map(|row| {
      Ok(SentResult {
        result: row.try_get("", "result")?,
        version: row.try_get("", "version")?,
        loaded_at: row.try_get("", "loaded_at")?,
      })
    })
    .collect::<Result<Vec<_>, job_base::Refusal>>()?;

  body(Work { item, results })
}

async fn served<V: Verbs>(state: State<V>, Path(session): Path<String>, Body(asked): Body) -> Answered {
  let item = Fields(&asked).whole("item")?;
  let now = now_ms();

  run(
    &state.db,
    r#"INSERT INTO "session_work_items" ("session_id", "todo_id", "served_at") VALUES ($1, $2, $3)
       ON CONFLICT ("session_id") DO UPDATE SET "todo_id" = excluded."todo_id", "served_at" = excluded."served_at""#,
    vec![session.into(), item.into(), now.into()],
  )
  .await?;
  run(
    &state.db,
    r#"DELETE FROM "session_work_items" WHERE "served_at" < $1"#,
    vec![(now - KEPT_MS).into()],
  )
  .await?;

  nothing()
}

async fn sent<V: Verbs>(
  state: State<V>,
  Path((session, result)): Path<(String, String)>,
  Body(asked): Body,
) -> Answered {
  let version = Fields(&asked).words("version")?;
  let now = now_ms();

  run(
    &state.db,
    r#"INSERT INTO "session_work_results" ("session_id", "result", "version", "loaded_at") VALUES ($1, $2, $3, $4)
       ON CONFLICT ("session_id", "result") DO UPDATE SET "version" = excluded."version", "loaded_at" = excluded."loaded_at""#,
    vec![session.into(), result.into(), version.into(), now.into()],
  )
  .await?;
  run(
    &state.db,
    r#"DELETE FROM "session_work_results" WHERE "session_id" IN
         (SELECT "session_id" FROM "session_work_results" GROUP BY "session_id" HAVING MAX("loaded_at") < $1)"#,
    vec![(now - KEPT_MS).into()],
  )
  .await?;

  nothing()
}

async fn folded<V: Verbs>(state: State<V>, Path(session): Path<String>) -> Answered {
  run(
    &state.db,
    r#"DELETE FROM "session_work_results" WHERE "session_id" = $1"#,
    vec![session.into()],
  )
  .await?;

  nothing()
}
