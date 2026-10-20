//! Notes on an item (docs/api.md, "Notes").

use bmo_job_client::{EventKind, NewNote, Note, Page};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::events::{emit, key_alone};
use super::items::{item_at, item_keys};
use super::{Store, filled, from_json, page_limit, paged, to_json};
use crate::refusal::Refusal;

fn note_from(row: &SqliteRow) -> Result<Note, Refusal> {
  Ok(Note {
    id: row.try_get("id")?,
    item_id: row.try_get("item_id")?,
    text: row.try_get("text")?,
    actor_name: row.try_get("actor_name")?,
    labels: from_json(row.try_get::<&str, _>("labels")?)?,
    created_at: row.try_get("created_at")?,
  })
}

impl Store {
  /// The item's notes past `after`.
  pub async fn notes(
    &self,
    item_id: i64,
    after: Option<i64>,
    limit: Option<i64>,
  ) -> Result<Page<Note>, Refusal> {
    let limit = page_limit(limit)?;
    let mut tx = self.begin().await?;
    item_at(&mut tx, item_id).await?;
    let rows = sqlx::query(
      "SELECT id, item_id, text, actor_name, labels, created_at FROM notes WHERE item_id = $1 AND id > $2 \
       ORDER BY id LIMIT $3",
    )
    .bind(item_id)
    .bind(after.unwrap_or(0))
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let notes = rows.iter().map(note_from).collect::<Result<Vec<_>, _>>()?;
    Ok(paged(notes, limit))
  }

  /// Appends a note to the item.
  pub async fn add_note(&self, item_id: i64, new: NewNote) -> Result<Note, Refusal> {
    filled("text", &new.text)?;
    filled("actorName", &new.actor_name)?;
    let now = self.now();
    let mut tx = self.begin().await?;
    let item = item_at(&mut tx, item_id).await?;
    let id: i64 = sqlx::query_scalar(
      "INSERT INTO notes (item_id, text, actor_name, labels, created_at) VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(item_id)
    .bind(&new.text)
    .bind(&new.actor_name)
    .bind(to_json(&new.labels))
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    let note = Note {
      id,
      item_id,
      text: new.text,
      actor_name: new.actor_name,
      labels: new.labels,
      created_at: now,
    };
    emit(&mut tx, now, EventKind::NoteCreated.as_str(), &note, &item_keys(&item)).await?;
    self.commit(tx).await?;
    Ok(note)
  }

  /// Takes a note of the item back.
  pub async fn delete_note(&self, item_id: i64, note_id: i64) -> Result<(), Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let item = item_at(&mut tx, item_id).await?;
    let deleted = sqlx::query("DELETE FROM notes WHERE id = $1 AND item_id = $2")
      .bind(note_id)
      .bind(item_id)
      .execute(&mut *tx)
      .await?;
    if deleted.rows_affected() == 0 {
      return Err(Refusal::not_found(format!("The item {item_id} has no note {note_id}.")));
    }
    emit(
      &mut tx,
      now,
      EventKind::NoteDeleted.as_str(),
      &key_alone(note_id, Some(item_id)),
      &item_keys(&item),
    )
    .await?;
    self.commit(tx).await?;
    Ok(())
  }
}
