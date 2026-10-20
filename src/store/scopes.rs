//! What the service holds under a caller's scope, and letting it all go (docs/api.md, "Scopes").

use bmo_job_client::{EventKind, ScopeCounts, ScopeHolding};

use super::events::{emit, key_alone};
use super::items::{item_at, item_keys};
use super::leases::end_standing_lease;
use super::questions::delete_question;
use super::subscriptions::{end_subscription, under_scope};
use super::{Store, Tx};
use crate::refusal::Refusal;

/// The items under the scope, and the questions under it or about those items.
const SCOPE_ITEMS: &str = "SELECT id FROM items WHERE scope = $1";
const SCOPE_QUESTIONS: &str =
  "SELECT id FROM questions WHERE scope = $1 OR item_id IN (SELECT id FROM items WHERE scope = $1)";

async fn count(tx: &mut Tx, sql: &str, scope: &str) -> Result<i64, Refusal> {
  Ok(sqlx::query_scalar(sql).bind(scope).fetch_one(&mut **tx).await?)
}

async fn counts(tx: &mut Tx, scope: &str) -> Result<ScopeCounts, Refusal> {
  Ok(ScopeCounts {
    items: count(tx, "SELECT COUNT(*) FROM items WHERE scope = $1", scope).await?,
    notes: count(tx, &format!("SELECT COUNT(*) FROM notes WHERE item_id IN ({SCOPE_ITEMS})"), scope)
      .await?,
    reports: count(tx, &format!("SELECT COUNT(*) FROM reports WHERE item_id IN ({SCOPE_ITEMS})"), scope)
      .await?,
    work_orders: count(
      tx,
      &format!("SELECT COUNT(*) FROM work_orders WHERE item_id IN ({SCOPE_ITEMS})"),
      scope,
    )
    .await?,
    tasks: count(tx, &format!("SELECT COUNT(*) FROM tasks WHERE item_id IN ({SCOPE_ITEMS})"), scope)
      .await?,
    leases: count(
      tx,
      &format!("SELECT COUNT(*) FROM leases WHERE ended_at IS NULL AND item_id IN ({SCOPE_ITEMS})"),
      scope,
    )
    .await?,
    questions: count(tx, &format!("SELECT COUNT(*) FROM ({SCOPE_QUESTIONS})"), scope).await?,
    replies: count(
      tx,
      &format!("SELECT COUNT(*) FROM replies WHERE question_id IN ({SCOPE_QUESTIONS})"),
      scope,
    )
    .await?,
    subscriptions: count(
      tx,
      "SELECT COUNT(*) FROM subscriptions WHERE ended_at IS NULL AND scope = $1",
      scope,
    )
    .await?,
  })
}

async fn held(tx: &mut Tx, scope: &str) -> Result<ScopeCounts, Refusal> {
  let counts = counts(tx, scope).await?;
  if counts.is_empty() {
    return Err(Refusal::not_found(format!("Nothing is held under the scope {scope:?}.")));
  }
  Ok(counts)
}

impl Store {
  /// What is held under the scope, counted by resource.
  pub async fn scope(&self, scope: &str) -> Result<ScopeHolding, Refusal> {
    let mut tx = self.begin().await?;
    let counts = held(&mut tx, scope).await?;
    tx.commit().await?;
    Ok(ScopeHolding {
      scope: scope.to_owned(),
      counts,
      state: None,
    })
  }

  /// Lets go of everything held under the scope, each member told as deleted; or, as a preview, nothing.
  pub async fn delete_scope(&self, scope: &str, dry_run: bool) -> Result<ScopeHolding, Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let counts = held(&mut tx, scope).await?;
    if dry_run {
      tx.commit().await?;
      return Ok(ScopeHolding {
        scope: scope.to_owned(),
        counts,
        state: Some("preview".to_owned()),
      });
    }

    let questions: Vec<i64> = sqlx::query_scalar(SCOPE_QUESTIONS).bind(scope).fetch_all(&mut *tx).await?;
    for id in questions {
      delete_question(&mut tx, id, now).await?;
    }

    let items: Vec<i64> = sqlx::query_scalar(SCOPE_ITEMS).bind(scope).fetch_all(&mut *tx).await?;
    for id in items {
      let mut item = item_at(&mut tx, id).await?;
      end_standing_lease(&mut tx, &mut item, now).await?;
      for table in ["notes", "reports", "work_orders", "tasks", "leases"] {
        sqlx::query(&format!("DELETE FROM {table} WHERE item_id = $1"))
          .bind(id)
          .execute(&mut *tx)
          .await?;
      }
      sqlx::query("DELETE FROM items WHERE id = $1").bind(id).execute(&mut *tx).await?;
      emit(&mut tx, now, EventKind::ItemDeleted.as_str(), &key_alone(id, None), &item_keys(&item)).await?;
    }

    for mut kept in under_scope(&mut tx, scope).await? {
      end_subscription(&mut tx, &mut kept, now).await?;
    }

    self.commit(tx).await?;
    Ok(ScopeHolding {
      scope: scope.to_owned(),
      counts,
      state: Some("deleted".to_owned()),
    })
  }
}
