//! Leases: an item taken by one holder for a span, renewed, given back, or run out (docs/api.md, "Leases").

use bmo_job_client::{
  Code, EventKind, Frozen, Item, ItemState, Lease, LeaseAsk, LeaseQuery, LeaseRenewal, Page,
};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::events::{emit, key_alone};
use super::items::{item_at, item_keys, save_item, told_changed};
use super::queues::{find_queue, gives_leases};
use super::{OWN_NAME, Store, Tx, filled, from_json, page_limit, paged, queue_name, to_json};
use crate::access::Caller;
use crate::refusal::Refusal;

/// The span a lease may be taken or renewed for.
const LEASE_SPAN_MS: std::ops::RangeInclusive<i64> = 1_000..=3_600_000;

const LEASE_COLUMNS: &str =
  "id, queue_name, item_id, holder_name, ttl_ms, expires_at, created_at, ended_at, frozen";

/// A lease row: the lease short of its item, and when it ended.
struct LeaseRow {
  id: i64,
  queue_name: String,
  item_id: i64,
  holder_name: String,
  ttl_ms: i64,
  expires_at: i64,
  created_at: i64,
  ended_at: Option<i64>,
  frozen: Option<Frozen>,
}

fn lease_row(row: &SqliteRow) -> Result<LeaseRow, Refusal> {
  Ok(LeaseRow {
    id: row.try_get("id")?,
    queue_name: row.try_get("queue_name")?,
    item_id: row.try_get("item_id")?,
    holder_name: row.try_get("holder_name")?,
    ttl_ms: row.try_get("ttl_ms")?,
    expires_at: row.try_get("expires_at")?,
    created_at: row.try_get("created_at")?,
    ended_at: row.try_get("ended_at")?,
    frozen: row.try_get::<Option<&str>, _>("frozen")?.map(from_json).transpose()?,
  })
}

impl LeaseRow {
  fn with(self, item: Item) -> Lease {
    Lease {
      id: self.id,
      queue_name: self.queue_name,
      item_id: self.item_id,
      holder_name: self.holder_name,
      ttl_ms: self.ttl_ms,
      expires_at: self.expires_at,
      created_at: self.created_at,
      frozen: self.frozen,
      item,
    }
  }
}

async fn lease_row_at(tx: &mut Tx, id: i64) -> Result<LeaseRow, Refusal> {
  let row = sqlx::query(&format!("SELECT {LEASE_COLUMNS} FROM leases WHERE id = $1"))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Refusal::not_found(format!("There is no lease {id}.")))?;
  lease_row(&row)
}

/// The lease at `id`, standing; one that ended is gone.
async fn standing_lease(tx: &mut Tx, id: i64) -> Result<LeaseRow, Refusal> {
  let lease = lease_row_at(tx, id).await?;
  if lease.ended_at.is_some() {
    return Err(Refusal::gone(format!("The lease {id} has ended.")));
  }
  Ok(lease)
}

/// What a lease holds fixed, as a body gave it: every label key named.
fn frozen_checked(frozen: Option<&Frozen>) -> Result<Option<String>, Refusal> {
  let Some(frozen) = frozen else {
    return Ok(None);
  };
  if frozen.labels.iter().any(|key| key.trim().is_empty()) {
    return Err(Refusal::bad_request("A label key a lease holds fixed is empty."));
  }
  Ok(Some(to_json(frozen)))
}

/// Refuses a change that gives anything the lease standing on `before` holds fixed another value in `after`
/// (docs/api.md, "While leased").
pub(crate) async fn refuse_frozen_change(tx: &mut Tx, before: &Item, after: &Item) -> Result<(), Refusal> {
  let Some(lease_id) = before.lease_id else {
    return Ok(());
  };
  let lease = lease_row_at(tx, lease_id).await?;
  let Some(frozen) = lease.frozen else {
    return Ok(());
  };
  let fields = frozen
    .fields
    .iter()
    .filter(|field| field.differs(before, after))
    .map(|field| format!("field {}", field.as_str()));
  let labels = frozen
    .labels
    .iter()
    .filter(|key| before.labels.get(*key) != after.labels.get(*key))
    .map(|key| format!("label {key}"));
  let touched: Vec<String> = fields.chain(labels).collect();
  if touched.is_empty() {
    return Ok(());
  }
  let why = match frozen.reason.as_deref().map(str::trim) {
    Some(reason) if !reason.is_empty() => format!(": {reason}"),
    _ => ".".to_owned(),
  };
  Err(Refusal::new(
    Code::ItemFrozen,
    format!(
      "The lease {lease_id} held by {} holds the item {}'s {} fixed until it ends{why}",
      lease.holder_name,
      before.id,
      touched.join(", "),
    ),
  ))
}

fn span(ttl_ms: i64) -> Result<(), Refusal> {
  if !LEASE_SPAN_MS.contains(&ttl_ms) {
    return Err(Refusal::bad_request("A lease's ttlMs is 1000 to 3600000."));
  }
  Ok(())
}

/// Ends the lease standing on `item`, telling it as `kind`, and leaves the item's state to the caller.
async fn end_lease(tx: &mut Tx, item: &mut Item, now: i64, kind: EventKind) -> Result<(), Refusal> {
  let Some(lease_id) = item.lease_id.take() else {
    return Ok(());
  };
  sqlx::query("UPDATE leases SET ended_at = $1 WHERE id = $2")
    .bind(now)
    .bind(lease_id)
    .execute(&mut **tx)
    .await?;
  let data = match kind {
    EventKind::LeaseExpired => {
      let lease = lease_row_at(tx, lease_id).await?;
      serde_json::to_value(lease.with(item.clone()))?
    }
    _ => key_alone(lease_id, None),
  };
  emit(tx, now, kind.as_str(), &data, &item_keys(item)).await?;
  Ok(())
}

/// Ends the lease standing on `item`, as an item's move ends it.
pub(crate) async fn end_standing_lease(tx: &mut Tx, item: &mut Item, now: i64) -> Result<(), Refusal> {
  end_lease(tx, item, now, EventKind::LeaseDeleted).await
}

/// The item of the one standing lease the caller `name` holds, or none where it holds none or several
/// (docs/api.md, "Leases").
pub(crate) async fn held_lease_item(tx: &mut Tx, name: &str) -> Result<Option<i64>, Refusal> {
  let items: Vec<i64> =
    sqlx::query_scalar("SELECT item_id FROM leases WHERE ended_at IS NULL AND caller_name = $1 LIMIT 2")
      .bind(name)
      .fetch_all(&mut **tx)
      .await?;
  Ok(match items[..] {
    [item_id] => Some(item_id),
    _ => None,
  })
}

/// Gives back every lease past its span at `now`, each item open again, as the service's own change.
pub(crate) async fn expire_leases(tx: &mut Tx, now: i64) -> Result<usize, Refusal> {
  let ids: Vec<i64> =
    sqlx::query_scalar("SELECT item_id FROM leases WHERE ended_at IS NULL AND expires_at <= $1")
      .bind(now)
      .fetch_all(&mut **tx)
      .await?;
  for item_id in &ids {
    let mut item = item_at(tx, *item_id).await?;
    let previous = item.state;
    if item.state == ItemState::Leased {
      item.state = ItemState::Open;
    }
    item.changed_at = now;
    end_lease(tx, &mut item, now, EventKind::LeaseExpired).await?;
    save_item(tx, &item).await?;
    told_changed(tx, now, &item, previous, Some(OWN_NAME)).await?;
  }
  Ok(ids.len())
}

impl Store {
  /// Takes the item the ask names, or the next open one of its queue, held by `caller`; none where nothing may be
  /// taken.
  pub async fn take_lease(&self, ask: &LeaseAsk, caller: &Caller) -> Result<Option<Lease>, Refusal> {
    span(ask.ttl_ms)?;
    filled("holderName", &ask.holder_name)?;
    let frozen = frozen_checked(ask.frozen.as_ref())?;
    queue_name(&ask.queue_name)?;
    let now = self.now();
    let mut tx = self.begin().await?;
    let queue = find_queue(&mut tx, &ask.queue_name).await?;
    let open = queue.as_ref().is_some_and(|queue| gives_leases(queue, now));

    let item = match ask.item_id {
      Some(item_id) => {
        let Some(item) = super::items::find_item(&mut tx, item_id).await? else {
          return Err(Refusal::new(Code::UnknownItem, format!("There is no item {item_id}.")));
        };
        if item.queue_name != ask.queue_name {
          return Err(Refusal::bad_request(format!(
            "The item {item_id} is not in the queue {}.",
            ask.queue_name
          )));
        }
        if !open {
          return Err(Refusal::new(
            Code::QueueNotOpen,
            format!("The queue {} gives no lease now.", ask.queue_name),
          ));
        }
        if item.state != ItemState::Open {
          return Err(Refusal::conflict(format!("The item {item_id} is {}, not open.", item.state)));
        }
        if item.assignee_name.is_some() && item.assignee_name != ask.assignee_name {
          return Err(Refusal::conflict(format!("The item {item_id} is assigned to another.")));
        }
        item
      }
      None => {
        if !open {
          return Ok(None);
        }
        let next: Option<i64> = sqlx::query_scalar(
          "SELECT id FROM items WHERE queue_name = $1 AND state = 'open' AND \
           (assignee_name IS NULL OR assignee_name = $2) \
           ORDER BY priority DESC, position, id LIMIT 1",
        )
        .bind(&ask.queue_name)
        .bind(&ask.assignee_name)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(next) = next else {
          return Ok(None);
        };
        item_at(&mut tx, next).await?
      }
    };

    let expires_at = now + ask.ttl_ms;
    let lease_id: i64 = sqlx::query_scalar(
      "INSERT INTO leases (queue_name, item_id, holder_name, caller_name, ttl_ms, expires_at, created_at, frozen) \
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
    )
    .bind(&ask.queue_name)
    .bind(item.id)
    .bind(&ask.holder_name)
    .bind(&caller.name)
    .bind(ask.ttl_ms)
    .bind(expires_at)
    .bind(now)
    .bind(frozen)
    .fetch_one(&mut *tx)
    .await?;
    let mut item = item;
    let previous = item.state;
    item.state = ItemState::Leased;
    item.lease_id = Some(lease_id);
    item.changed_at = now;
    save_item(&mut tx, &item).await?;
    let lease = lease_row_at(&mut tx, lease_id).await?.with(item.clone());
    emit(&mut tx, now, EventKind::LeaseCreated.as_str(), &lease, &item_keys(&item)).await?;
    told_changed(&mut tx, now, &item, previous, caller.name.as_deref()).await?;
    self.commit(tx).await?;
    Ok(Some(lease))
  }

  /// The leases standing now.
  pub async fn leases(&self, query: &LeaseQuery) -> Result<Page<Lease>, Refusal> {
    let limit = page_limit(query.limit)?;
    let mut tx = self.begin().await?;
    let rows = sqlx::query(&format!(
      "SELECT {LEASE_COLUMNS} FROM leases WHERE ended_at IS NULL AND id > $1 \
       AND ($2 IS NULL OR holder_name = $2) AND ($3 IS NULL OR queue_name = $3) ORDER BY id LIMIT $4"
    ))
    .bind(query.after.unwrap_or(0))
    .bind(&query.holder_name)
    .bind(&query.queue_name)
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    let mut leases = Vec::with_capacity(rows.len());
    for row in &rows {
      let lease = lease_row(row)?;
      let item = item_at(&mut tx, lease.item_id).await?;
      leases.push(lease.with(item));
    }
    tx.commit().await?;
    Ok(paged(leases, limit))
  }

  pub async fn lease(&self, id: i64) -> Result<Lease, Refusal> {
    let mut tx = self.begin().await?;
    let lease = standing_lease(&mut tx, id).await?;
    let item = item_at(&mut tx, lease.item_id).await?;
    tx.commit().await?;
    Ok(lease.with(item))
  }

  /// Renews a standing lease for its span from now, and sets what it holds fixed where the renewal carries it.
  pub async fn renew_lease(&self, id: i64, renewal: LeaseRenewal) -> Result<Lease, Refusal> {
    span(renewal.ttl_ms)?;
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut lease = standing_lease(&mut tx, id).await?;
    lease.ttl_ms = renewal.ttl_ms;
    lease.expires_at = now + renewal.ttl_ms;
    if let Some(frozen) = renewal.frozen {
      lease.frozen = frozen;
    }
    let frozen = frozen_checked(lease.frozen.as_ref())?;
    sqlx::query("UPDATE leases SET ttl_ms = $1, expires_at = $2, frozen = $3 WHERE id = $4")
      .bind(lease.ttl_ms)
      .bind(lease.expires_at)
      .bind(frozen)
      .bind(id)
      .execute(&mut *tx)
      .await?;
    let item = item_at(&mut tx, lease.item_id).await?;
    self.commit(tx).await?;
    Ok(lease.with(item))
  }

  /// Gives a standing lease back, by `caller`; its item is open again.
  pub async fn release_lease(&self, id: i64, caller: &Caller) -> Result<(), Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let lease = standing_lease(&mut tx, id).await?;
    let mut item = item_at(&mut tx, lease.item_id).await?;
    let previous = item.state;
    end_standing_lease(&mut tx, &mut item, now).await?;
    if item.state == ItemState::Leased {
      item.state = ItemState::Open;
    }
    item.changed_at = now;
    save_item(&mut tx, &item).await?;
    told_changed(&mut tx, now, &item, previous, caller.name.as_deref()).await?;
    self.commit(tx).await?;
    Ok(())
  }
}
