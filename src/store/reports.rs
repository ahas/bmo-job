//! Reports on an item (docs/api.md, "Reports").

use bmo_job_client::{EventKind, NewReport, Page, Report, ReportKind};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::events::emit;
use super::items::{item_at, item_keys};
use super::{Store, filled, from_json, page_limit, paged, stored, to_json};
use crate::refusal::Refusal;

fn report_from(row: &SqliteRow) -> Result<Report, Refusal> {
  Ok(Report {
    id: row.try_get("id")?,
    item_id: row.try_get("item_id")?,
    kind: stored(row.try_get("kind")?, ReportKind::parse)?,
    body: row.try_get("body")?,
    actor_name: row.try_get("actor_name")?,
    labels: from_json(row.try_get::<&str, _>("labels")?)?,
    created_at: row.try_get("created_at")?,
  })
}

impl Store {
  /// The item's reports past `after`, oldest first.
  pub async fn reports(
    &self,
    item_id: i64,
    after: Option<i64>,
    limit: Option<i64>,
  ) -> Result<Page<Report>, Refusal> {
    let limit = page_limit(limit)?;
    let mut tx = self.begin().await?;
    item_at(&mut tx, item_id).await?;
    let rows = sqlx::query(
      "SELECT id, item_id, kind, body, actor_name, labels, created_at FROM reports WHERE item_id = $1 AND id > $2 \
       ORDER BY id LIMIT $3",
    )
    .bind(item_id)
    .bind(after.unwrap_or(0))
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let reports = rows.iter().map(report_from).collect::<Result<Vec<_>, _>>()?;
    Ok(paged(reports, limit))
  }

  /// Appends a report to the item.
  pub async fn add_report(&self, item_id: i64, new: NewReport) -> Result<Report, Refusal> {
    filled("body", &new.body)?;
    filled("actorName", &new.actor_name)?;
    let now = self.now();
    let mut tx = self.begin().await?;
    let item = item_at(&mut tx, item_id).await?;
    let id: i64 = sqlx::query_scalar(
      "INSERT INTO reports (item_id, kind, body, actor_name, labels, created_at) VALUES ($1, $2, $3, $4, $5, $6) \
       RETURNING id",
    )
    .bind(item_id)
    .bind(new.kind.as_str())
    .bind(&new.body)
    .bind(&new.actor_name)
    .bind(to_json(&new.labels))
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    let report = Report {
      id,
      item_id,
      kind: new.kind,
      body: new.body,
      actor_name: new.actor_name,
      labels: new.labels,
      created_at: now,
    };
    emit(&mut tx, now, EventKind::ReportCreated.as_str(), &report, &item_keys(&item)).await?;
    self.commit(tx).await?;
    Ok(report)
  }
}
