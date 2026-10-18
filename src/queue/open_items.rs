//! The items still open on the queue, as bmo-receiver reads them to show an agent the work standing under
//! the other agents (docs/api.md, "The open items").

use sea_orm::{ConnectionTrait, QueryResult};
use serde::Serialize;

use crate::{Refusal, all};

/// One item not finished, merged or set aside, with the worker it is assigned to where it is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenItem {
  pub id: i64,
  pub approved: bool,
  pub status: String,
  pub title: String,
  pub worker: Option<String>,
}

impl OpenItem {
  fn of(row: &QueryResult) -> Result<Self, sea_orm::DbErr> {
    Ok(Self {
      id: row.try_get("", "id")?,
      approved: row.try_get::<i64>("", "approved")? != 0,
      status: row.try_get("", "status")?,
      title: row.try_get("", "title")?,
      worker: row.try_get("", "worker")?,
    })
  }
}

/// Every item not finished, merged or held, oldest first.
pub async fn listed(db: &impl ConnectionTrait) -> Result<Vec<OpenItem>, Refusal> {
  let rows = all(
    db,
    r#"SELECT "id", "approved", "status", "title", "worker" FROM "todos"
       WHERE "done" = 0 AND "merged_into" IS NULL AND "held_at" IS NULL ORDER BY "id""#,
    vec![],
  )
  .await?;

  Ok(rows.iter().map(OpenItem::of).collect::<Result<_, _>>()?)
}
