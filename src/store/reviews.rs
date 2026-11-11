//! The entries of an item's review (docs/api.md, "Reviews").

use bmo_job_client::{EventKind, NewReview, Page, Review, ReviewKind, ReviewQuery};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::events::emit;
use super::items::{item_at, item_keys};
use super::{Store, filled, from_json, page_limit, paged, stored, to_json};
use crate::refusal::Refusal;

const REVIEW_COLUMNS: &str = "id, item_id, ref, stage, round, kind, body, actor_name, labels, created_at";

fn review_from(row: &SqliteRow) -> Result<Review, Refusal> {
  Ok(Review {
    id: row.try_get("id")?,
    item_id: row.try_get("item_id")?,
    r#ref: row.try_get("ref")?,
    stage: row.try_get("stage")?,
    round: row.try_get("round")?,
    kind: stored(row.try_get("kind")?, ReviewKind::parse)?,
    body: row.try_get("body")?,
    actor_name: row.try_get("actor_name")?,
    labels: from_json(row.try_get::<&str, _>("labels")?)?,
    created_at: row.try_get("created_at")?,
  })
}

// A count the review keeps from 1 upward, or the sentence refusing it.
fn from_one(field: &str, value: Option<i64>) -> Result<(), Refusal> {
  match value {
    Some(value) if value < 1 => {
      Err(Refusal::bad_request(format!("{field} counts from 1, and {value} does not")))
    }
    _ => Ok(()),
  }
}

impl Store {
  /// The item's review past `after`, oldest first, narrowed to one run's entries where `ref` names one.
  pub async fn reviews(&self, item_id: i64, query: &ReviewQuery) -> Result<Page<Review>, Refusal> {
    let limit = page_limit(query.limit)?;
    let mut tx = self.begin().await?;
    item_at(&mut tx, item_id).await?;
    let rows = sqlx::query(&format!(
      "SELECT {REVIEW_COLUMNS} FROM reviews WHERE item_id = $1 AND id > $2 AND ($3 IS NULL OR ref = $3) \
       ORDER BY id LIMIT $4"
    ))
    .bind(item_id)
    .bind(query.after.unwrap_or(0))
    .bind(query.r#ref.as_deref())
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let reviews = rows.iter().map(review_from).collect::<Result<Vec<_>, _>>()?;
    Ok(paged(reviews, limit))
  }

  /// Appends an entry to the item's review.
  pub async fn add_review(&self, item_id: i64, new: NewReview) -> Result<Review, Refusal> {
    filled("body", &new.body)?;
    filled("actorName", &new.actor_name)?;
    from_one("stage", new.stage)?;
    from_one("round", new.round)?;
    let round = new.round.unwrap_or(1);
    let now = self.now();
    let mut tx = self.begin().await?;
    let item = item_at(&mut tx, item_id).await?;
    let id: i64 = sqlx::query_scalar(
      "INSERT INTO reviews (item_id, ref, stage, round, kind, body, actor_name, labels, created_at) \
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9) RETURNING id",
    )
    .bind(item_id)
    .bind(new.r#ref.as_deref())
    .bind(new.stage)
    .bind(round)
    .bind(new.kind.as_str())
    .bind(&new.body)
    .bind(&new.actor_name)
    .bind(to_json(&new.labels))
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    let review = Review {
      id,
      item_id,
      r#ref: new.r#ref,
      stage: new.stage,
      round,
      kind: new.kind,
      body: new.body,
      actor_name: new.actor_name,
      labels: new.labels,
      created_at: now,
    };
    emit(&mut tx, now, EventKind::ReviewCreated.as_str(), &review, &item_keys(&item)).await?;
    self.commit(tx).await?;
    Ok(review)
  }
}
