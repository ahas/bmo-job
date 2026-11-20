//! Label values: the values a label key may take, as a caller published them, and the check an item's labels
//! are held to (docs/api.md, "Label values").

use std::collections::BTreeSet;

use bmo_job_client::{Code, LabelValues, LabelValuesBody, LabelValuesQuery, Labels, Page};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::{Store, Tx, filled, from_json, page_limit, paged, to_json};
use crate::refusal::Refusal;

fn label_values_from(row: &SqliteRow) -> Result<LabelValues, Refusal> {
  Ok(LabelValues {
    key: row.try_get("key")?,
    values: from_json(row.try_get::<&str, _>("allowed")?)?,
    changed_at: row.try_get("changed_at")?,
  })
}

async fn find_label_values(tx: &mut Tx, key: &str) -> Result<Option<LabelValues>, Refusal> {
  let row = sqlx::query("SELECT key, allowed, changed_at FROM label_values WHERE key = $1")
    .bind(key)
    .fetch_optional(&mut **tx)
    .await?;
  row.as_ref().map(label_values_from).transpose()
}

fn none_for(key: &str) -> Refusal {
  Refusal::not_found(format!("No values are published for the label {key}."))
}

/// Refuses the first label in key order whose value is set, differs from what `before` holds under its key, and
/// stands on no list its key has. `before` is the item's labels before a change, or none for a raise.
pub(crate) async fn refuse_unknown_label_values(
  tx: &mut Tx,
  before: Option<&Labels>,
  labels: &Labels,
) -> Result<(), Refusal> {
  for (key, value) in labels {
    let set = value.trim();
    if set.is_empty() || before.and_then(|before| before.get(key)) == Some(value) {
      continue;
    }
    let Some(list) = find_label_values(tx, key).await? else {
      continue;
    };
    if list.values.iter().any(|allowed| allowed == set) {
      continue;
    }
    let sentence = if list.values.is_empty() {
      format!("The label {key} names {set:?}, and its list holds no value.")
    } else {
      format!("The label {key} names {set:?}, which is none of: {}.", list.values.join(", "))
    };
    return Err(Refusal::new(Code::LabelValueUnknown, sentence));
  }
  Ok(())
}

impl Store {
  /// Every key's list, in key order.
  pub async fn label_values_list(&self, query: &LabelValuesQuery) -> Result<Page<LabelValues>, Refusal> {
    let limit = page_limit(query.limit)?;
    let mut tx = self.begin().await?;
    let rows =
      sqlx::query("SELECT key, allowed, changed_at FROM label_values WHERE key > $1 ORDER BY key LIMIT $2")
        .bind(query.after.clone().unwrap_or_default())
        .bind(limit + 1)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;
    let lists = rows.iter().map(label_values_from).collect::<Result<Vec<_>, _>>()?;
    Ok(paged(lists, limit))
  }

  pub async fn label_values(&self, key: &str) -> Result<LabelValues, Refusal> {
    let mut tx = self.begin().await?;
    let list = find_label_values(&mut tx, key).await?.ok_or_else(|| none_for(key))?;
    tx.commit().await?;
    Ok(list)
  }

  /// Replaces the key's list whole, its values trimmed, the blank ones dropped, each kept once and sorted.
  pub async fn put_label_values(&self, key: &str, body: LabelValuesBody) -> Result<LabelValues, Refusal> {
    filled("label key", key)?;
    let values: BTreeSet<String> = body
      .values
      .iter()
      .map(|value| value.trim())
      .filter(|value| !value.is_empty())
      .map(str::to_owned)
      .collect();
    let list = LabelValues {
      key: key.to_owned(),
      values: values.into_iter().collect(),
      changed_at: self.now(),
    };
    let mut tx = self.begin().await?;
    sqlx::query(
      "INSERT INTO label_values (key, allowed, changed_at) VALUES ($1, $2, $3) \
       ON CONFLICT (key) DO UPDATE SET allowed = excluded.allowed, changed_at = excluded.changed_at",
    )
    .bind(&list.key)
    .bind(to_json(&list.values))
    .bind(list.changed_at)
    .execute(&mut *tx)
    .await?;
    self.commit(tx).await?;
    Ok(list)
  }

  /// Removes the key's list.
  pub async fn delete_label_values(&self, key: &str) -> Result<(), Refusal> {
    let mut tx = self.begin().await?;
    let removed = sqlx::query("DELETE FROM label_values WHERE key = $1")
      .bind(key)
      .execute(&mut *tx)
      .await?;
    if removed.rows_affected() == 0 {
      return Err(none_for(key));
    }
    self.commit(tx).await?;
    Ok(())
  }
}
