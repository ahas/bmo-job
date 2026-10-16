//! A work order's body as `work_orders` keeps it: the body stored read and the body written back. The draft
//! an item is filed with is `queue_read::work_order`'s; the body is the work order runner's own JSON, which
//! the verbs change member by member.

use job_base::{Refusal, now_ms, one, run};
use sea_orm::ConnectionTrait;
use serde_json::{Value, json};

/// The body stored for an item, where one is.
pub async fn read(db: &impl ConnectionTrait, todo_id: i64) -> Result<Option<Value>, Refusal> {
  let Some(row) =
    one(db, r#"SELECT "body" FROM "work_orders" WHERE "todo_id" = $1"#, vec![todo_id.into()]).await?
  else {
    return Ok(None);
  };
  let body: String = row.try_get("", "body")?;

  serde_json::from_str(&body)
    .map(Some)
    .map_err(|error| sea_orm::DbErr::Custom(format!("work order #{todo_id}: {error}")).into())
}

/// The body written back with the write clock on it.
pub async fn save(db: &impl ConnectionTrait, order: &mut Value) -> Result<(), Refusal> {
  order["updatedAt"] = json!(now_ms());

  let todo_id = order.get("todoId").and_then(Value::as_i64).unwrap_or_default();

  run(
    db,
    r#"INSERT INTO "work_orders" ("todo_id", "body") VALUES ($1, $2)
       ON CONFLICT ("todo_id") DO UPDATE SET "body" = excluded."body""#,
    vec![todo_id.into(), order.to_string().into()],
  )
  .await
  .map(drop)
}
