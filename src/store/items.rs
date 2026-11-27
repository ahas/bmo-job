//! Items: raised, read in queue order, and changed by the moves docs/api.md lists (docs/api.md, "Items").

use bmo_job_client::{
  Approval, ApprovalKind, Code, EventKind, Item, ItemChange, ItemEvent, ItemHold, ItemQuery, ItemState,
  ItemsRelabel, ItemsRelabeled, NewItem, NewReport, NoticesOff, Page, ReportKind,
};
use serde::Deserialize;
use sqlx::sqlite::SqliteRow;
use sqlx::{QueryBuilder, Row, Sqlite};

use super::events::{EventKeys, emit};
use super::label_values::refuse_unknown_label_values;
use super::leases::{end_standing_lease, held_lease_item, refuse_frozen_change};
use super::queues::ensure_queue;
use super::reports::write_report;
use super::{
  OWN_NAME, Store, Tx, filled, from_json, listed, page_limit, paged, queue_name, stored, to_json,
};
use crate::access::Caller;
use crate::refusal::Refusal;

const ITEM_COLUMNS: &str = "id, title, brief, state, queue_name, position, priority, assignee_name, requester_name, \
  scope, labels, approvals, merged_into, raised_by, lease_id, created_at, changed_at, ended_at, \
  hold_reason, hold_caller_name, hold_at, notices_off_reason, notices_off_caller_name, notices_off_at, \
  (SELECT COUNT(*) FROM reviews WHERE reviews.item_id = items.id AND reviews.kind = 'rejection') AS rejections";

/// An approval as the store holds it. One held with no kind, such as one imported, reads as an approval of no
/// caller (docs/api.md, "Approvals").
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct HeldApproval {
  #[serde(default = "held_kind")]
  kind: ApprovalKind,
  #[serde(default)]
  caller_name: Option<String>,
  at: i64,
}

fn held_kind() -> ApprovalKind {
  ApprovalKind::Approval
}

fn approvals_from(text: &str) -> Result<Vec<Approval>, Refusal> {
  let held: Vec<HeldApproval> = from_json(text)?;
  Ok(
    held
      .into_iter()
      .map(|held| Approval {
        kind: held.kind,
        caller_name: held.caller_name,
        at: held.at,
      })
      .collect(),
  )
}

/// An item's hold as its columns keep it: a reason and a time where it is held.
fn hold_from(row: &SqliteRow) -> Result<Option<ItemHold>, Refusal> {
  let reason: Option<String> = row.try_get("hold_reason")?;
  let caller_name: Option<String> = row.try_get("hold_caller_name")?;
  let at: Option<i64> = row.try_get("hold_at")?;
  Ok(reason.zip(at).map(|(reason, at)| ItemHold {
    reason,
    caller_name,
    at,
  }))
}

/// An item's notices switched off as its columns keep it: a reason and a time where they are off.
fn notices_off_from(row: &SqliteRow) -> Result<Option<NoticesOff>, Refusal> {
  let reason: Option<String> = row.try_get("notices_off_reason")?;
  let caller_name: Option<String> = row.try_get("notices_off_caller_name")?;
  let at: Option<i64> = row.try_get("notices_off_at")?;
  Ok(reason.zip(at).map(|(reason, at)| NoticesOff {
    reason,
    caller_name,
    at,
  }))
}

fn item_from(row: &SqliteRow) -> Result<Item, Refusal> {
  Ok(Item {
    id: row.try_get("id")?,
    title: row.try_get("title")?,
    brief: row.try_get("brief")?,
    state: stored(row.try_get("state")?, ItemState::parse)?,
    queue_name: row.try_get("queue_name")?,
    position: row.try_get("position")?,
    priority: row.try_get("priority")?,
    assignee_name: row.try_get("assignee_name")?,
    requester_name: row.try_get("requester_name")?,
    scope: row.try_get("scope")?,
    labels: from_json(row.try_get::<&str, _>("labels")?)?,
    approvals: approvals_from(row.try_get::<&str, _>("approvals")?)?,
    rejections: row.try_get("rejections")?,
    merged_into: row.try_get("merged_into")?,
    raised_by: row.try_get("raised_by")?,
    lease_id: row.try_get("lease_id")?,
    hold: hold_from(row)?,
    notices_off: notices_off_from(row)?,
    created_at: row.try_get("created_at")?,
    changed_at: row.try_get("changed_at")?,
    ended_at: row.try_get("ended_at")?,
  })
}

/// The item at `id`, or none.
pub(crate) async fn find_item(tx: &mut Tx, id: i64) -> Result<Option<Item>, Refusal> {
  let row = sqlx::query(&format!("SELECT {ITEM_COLUMNS} FROM items WHERE id = $1"))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?;
  row.as_ref().map(item_from).transpose()
}

/// The item at `id`, or a refusal saying there is none.
pub(crate) async fn item_at(tx: &mut Tx, id: i64) -> Result<Item, Refusal> {
  find_item(tx, id)
    .await?
    .ok_or_else(|| Refusal::not_found(format!("There is no item {id}.")))
}

/// Writes every field of `item` back.
pub(crate) async fn save_item(tx: &mut Tx, item: &Item) -> Result<(), Refusal> {
  sqlx::query(
    "UPDATE items SET title = $1, brief = $2, state = $3, queue_name = $4, position = $5, priority = $6, \
     assignee_name = $7, requester_name = $8, scope = $9, labels = $10, approvals = $11, \
     merged_into = $12, lease_id = $13, changed_at = $14, ended_at = $15, hold_reason = $16, \
     hold_caller_name = $17, hold_at = $18, notices_off_reason = $19, notices_off_caller_name = $20, \
     notices_off_at = $21 WHERE id = $22",
  )
  .bind(&item.title)
  .bind(&item.brief)
  .bind(item.state.as_str())
  .bind(&item.queue_name)
  .bind(item.position)
  .bind(item.priority)
  .bind(&item.assignee_name)
  .bind(&item.requester_name)
  .bind(&item.scope)
  .bind(to_json(&item.labels))
  .bind(to_json(&item.approvals))
  .bind(item.merged_into)
  .bind(item.lease_id)
  .bind(item.changed_at)
  .bind(item.ended_at)
  .bind(item.hold.as_ref().map(|hold| &hold.reason))
  .bind(item.hold.as_ref().and_then(|hold| hold.caller_name.as_ref()))
  .bind(item.hold.as_ref().map(|hold| hold.at))
  .bind(item.notices_off.as_ref().map(|off| &off.reason))
  .bind(item.notices_off.as_ref().and_then(|off| off.caller_name.as_ref()))
  .bind(item.notices_off.as_ref().map(|off| off.at))
  .bind(item.id)
  .execute(&mut **tx)
  .await?;
  Ok(())
}

/// What an event about the item, or a member of it, is narrowed by.
pub(crate) fn item_keys(item: &Item) -> EventKeys {
  EventKeys {
    queue_name: Some(item.queue_name.clone()),
    scope: item.scope.clone(),
    item_id: Some(item.id),
  }
}

/// Tells that the item changed from the state `previous`, by `changed_by` (docs/api.md, "Events").
pub(crate) async fn told_changed(
  tx: &mut Tx,
  at: i64,
  item: &Item,
  previous: ItemState,
  changed_by: Option<&str>,
) -> Result<(), Refusal> {
  let told = ItemEvent {
    item: item.clone(),
    previous_state: (previous != item.state).then_some(previous),
    changed_by: changed_by.map(str::to_owned),
  };
  emit(tx, at, EventKind::ItemChanged.as_str(), &told, &item_keys(item)).await?;
  Ok(())
}

/// The item a raise names as `raisedBy`: the one it carries, which must stand, or else the item of the one lease
/// its caller holds (docs/api.md, "Raised from an item").
async fn raised_from(tx: &mut Tx, given: Option<i64>, caller: &Caller) -> Result<Option<i64>, Refusal> {
  match (given, &caller.name) {
    (Some(id), _) => match find_item(tx, id).await? {
      Some(_) => Ok(Some(id)),
      None => Err(Refusal::new(
        Code::UnknownItem,
        format!("There is no item {id} to raise this one from."),
      )),
    },
    (None, Some(name)) => held_lease_item(tx, name).await,
    (None, None) => Ok(None),
  }
}

fn reason_required(sentence: &str) -> Refusal {
  Refusal::new(Code::ReasonRequired, sentence)
}

/// A reason a hold or a switch-off carries, which may not be blank.
fn reason_given(reason: &str) -> Result<(), Refusal> {
  if reason.trim().is_empty() {
    return Err(reason_required("The reason is blank; say why in it."));
  }
  Ok(())
}

/// Whether a state may move to another by a change (docs/api.md, "States and moves"); `leased` is reached
/// only by a lease.
fn may_move(from: ItemState, to: ItemState) -> bool {
  use ItemState::*;
  match to {
    Open => matches!(from, Draft | Held),
    Held => matches!(from, Open | Leased),
    Done => from == Leased,
    Dropped | Merged => !from.is_final(),
    Draft | Leased => false,
  }
}

impl Store {
  /// Raises an item for `caller` as a draft, or open where it carries `unattended`, a label the settings name
  /// unattended, with a note saying it opened by itself (docs/api.md, "Unattended labels").
  pub async fn create_item(
    &self,
    new: NewItem,
    unattended: Option<&str>,
    caller: &Caller,
  ) -> Result<Item, Refusal> {
    filled("title", &new.title)?;
    queue_name(&new.queue_name)?;
    let now = self.now();
    let mut tx = self.begin().await?;
    let raised_by = raised_from(&mut tx, new.raised_by, caller).await?;
    refuse_unknown_label_values(&mut tx, None, &new.labels).await?;
    ensure_queue(&mut tx, &new.queue_name, now).await?;
    let position: i64 =
      sqlx::query_scalar("SELECT COALESCE(MAX(position), 0) + 1 FROM items WHERE queue_name = $1")
        .bind(&new.queue_name)
        .fetch_one(&mut *tx)
        .await?;
    let id: i64 = sqlx::query_scalar(
      "INSERT INTO items (title, brief, state, queue_name, position, priority, assignee_name, requester_name, scope, \
       labels, approvals, raised_by, created_at, changed_at) \
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, '[]', $11, $12, $12) RETURNING id",
    )
    .bind(&new.title)
    .bind(&new.brief)
    .bind(if unattended.is_some() { ItemState::Open } else { ItemState::Draft }.as_str())
    .bind(&new.queue_name)
    .bind(position)
    .bind(new.priority.unwrap_or(0))
    .bind(&new.assignee_name)
    .bind(&new.requester_name)
    .bind(&new.scope)
    .bind(to_json(&new.labels))
    .bind(raised_by)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    let item = item_at(&mut tx, id).await?;
    let told = ItemEvent {
      item: item.clone(),
      previous_state: None,
      changed_by: caller.name.clone(),
    };
    emit(&mut tx, now, EventKind::ItemCreated.as_str(), &told, &item_keys(&item)).await?;
    if let Some(label) = unattended {
      super::notes::opened_unattended(&mut tx, now, &item, label).await?;
    }
    self.commit(tx).await?;
    Ok(item)
  }

  pub async fn item(&self, id: i64) -> Result<Item, Refusal> {
    let mut tx = self.begin().await?;
    let item = item_at(&mut tx, id).await?;
    tx.commit().await?;
    Ok(item)
  }

  /// Items in queue order, filtered as the query asks.
  pub async fn items(&self, query: &ItemQuery) -> Result<Page<Item>, Refusal> {
    let limit = page_limit(query.limit)?;
    let states = listed("state", query.state.as_deref(), ItemState::parse)?;
    let ids = listed("id", query.ids.as_deref(), |id| id.parse::<i64>().ok())?;
    let mut tx = self.begin().await?;

    let mut select: QueryBuilder<Sqlite> =
      QueryBuilder::new(format!("SELECT {ITEM_COLUMNS} FROM items WHERE 1 = 1"));
    if !states.is_empty() {
      select.push(" AND state IN (");
      let mut list = select.separated(", ");
      for state in &states {
        list.push_bind(state.as_str());
      }
      select.push(")");
    }
    if !ids.is_empty() {
      select.push(" AND id IN (");
      let mut list = select.separated(", ");
      for id in &ids {
        list.push_bind(*id);
      }
      select.push(")");
    }
    if let Some(queue) = &query.queue_name {
      select.push(" AND queue_name = ").push_bind(queue.clone());
    }
    if let Some(assignee) = &query.assignee_name {
      select.push(" AND assignee_name = ").push_bind(assignee.clone());
    }
    if let Some(raised_by) = query.raised_by {
      select.push(" AND raised_by = ").push_bind(raised_by);
    }
    if let Some(scope) = &query.scope {
      select.push(" AND scope = ").push_bind(scope.clone());
    }
    if let Some(label) = &query.label {
      let (key, value) = match label.split_once('=') {
        Some((key, value)) => (key.to_owned(), Some(value.to_owned())),
        None => (label.clone(), None),
      };
      select
        .push(" AND EXISTS (SELECT 1 FROM json_each(items.labels) WHERE json_each.key = ")
        .push_bind(key);
      if let Some(value) = value {
        select.push(" AND json_each.value = ").push_bind(value);
      }
      select.push(")");
    }
    if let Some(words) = &query.q {
      for word in words.split_whitespace() {
        let word = word.to_lowercase();
        select
          .push(" AND (instr(lower(title), ")
          .push_bind(word.clone())
          .push(") > 0 OR instr(lower(brief), ")
          .push_bind(word)
          .push(") > 0)");
      }
    }
    if let Some(after) = query.after {
      let Some(last) = find_item(&mut tx, after).await? else {
        return Err(Refusal::bad_request(format!("The cursor names no item: {after}.")));
      };
      select
        .push(" AND (priority < ")
        .push_bind(last.priority)
        .push(" OR (priority = ")
        .push_bind(last.priority)
        .push(" AND (position > ")
        .push_bind(last.position)
        .push(" OR (position = ")
        .push_bind(last.position)
        .push(" AND id > ")
        .push_bind(last.id)
        .push("))))");
    }
    select.push(" ORDER BY priority DESC, position, id LIMIT ").push_bind(limit + 1);
    let rows = select.build().fetch_all(&mut *tx).await?;
    tx.commit().await?;
    let items = rows.iter().map(item_from).collect::<Result<Vec<_>, _>>()?;
    Ok(paged(items, limit))
  }

  /// The items not done, dropped, merged or held, oldest first.
  pub async fn open_items(&self) -> Result<Vec<Item>, Refusal> {
    let mut tx = self.begin().await?;
    let rows = sqlx::query(&format!(
      "SELECT {ITEM_COLUMNS} FROM items WHERE state NOT IN ($1, $2, $3, $4) ORDER BY id"
    ))
    .bind(ItemState::Done.as_str())
    .bind(ItemState::Dropped.as_str())
    .bind(ItemState::Merged.as_str())
    .bind(ItemState::Held.as_str())
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter().map(item_from).collect()
  }

  /// How many items under `scope` have not ended: not done, dropped or merged.
  pub async fn items_in_scope(&self, scope: &str) -> Result<i64, Refusal> {
    let mut tx = self.begin().await?;
    let count =
      sqlx::query_scalar("SELECT count(*) FROM items WHERE scope = $1 AND state NOT IN ($2, $3, $4)")
        .bind(scope)
        .bind(ItemState::Done.as_str())
        .bind(ItemState::Dropped.as_str())
        .bind(ItemState::Merged.as_str())
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(count)
  }

  /// Changes an item as a merge patch, its state by the moves allowed; an approval it carries is `caller`'s.
  /// Several items' labels changed at once (docs/api.md, Items, `PATCH /items`): each item as a change of its labels
  /// alone would change it, its keys merged with those given, and one refused does not hold back the rest. Refused
  /// whole where no item or more than 200 are named, or an item is named twice.
  pub async fn relabel_items(
    &self,
    relabel: ItemsRelabel,
    caller: &Caller,
  ) -> Result<ItemsRelabeled, Refusal> {
    let mut seen = relabel.ids.clone();

    seen.sort_unstable();
    seen.dedup();

    if relabel.ids.is_empty() || relabel.ids.len() > 200 || seen.len() != relabel.ids.len() {
      return Err(Refusal::bad_request(
        "ids names 1 to 200 items, each once; job_items lists them by a filter",
      ));
    }

    if relabel.labels.is_empty() {
      return Err(Refusal::bad_request(
        "labels names at least one key, set to a value or to null to take it off",
      ));
    }

    let mut answered = ItemsRelabeled::default();

    for id in relabel.ids {
      let merged = match self.item(id).await {
        Ok(item) => {
          let mut labels = item.labels.clone();

          for (key, value) in &relabel.labels {
            match value {
              Some(value) => labels.insert(key.clone(), value.clone()),
              None => labels.remove(key),
            };
          }

          labels
        }
        Err(refusal) => {
          answered.refused.push(refused(id, refusal));
          continue;
        }
      };
      let change = ItemChange {
        labels: Some(merged),
        ..ItemChange::default()
      };

      match self.change_item(id, change, caller).await {
        Ok(item) => answered.changed.push(item),
        Err(refusal) => answered.refused.push(refused(id, refusal)),
      }
    }

    Ok(answered)
  }

  pub async fn change_item(&self, id: i64, change: ItemChange, caller: &Caller) -> Result<Item, Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut item = item_at(&mut tx, id).await?;
    let before = item.clone();
    let previous = item.state;

    if let Some(title) = change.title {
      filled("title", &title)?;
      item.title = title;
    }
    if let Some(brief) = change.brief {
      item.brief = brief;
    }
    if let Some(queue) = change.queue_name
      && queue != item.queue_name
    {
      queue_name(&queue)?;
      if item.state == ItemState::Leased {
        return Err(Refusal::conflict("A leased item stays in its queue until its lease ends."));
      }
      ensure_queue(&mut tx, &queue, now).await?;
      item.queue_name = queue;
    }
    if let Some(priority) = change.priority {
      item.priority = priority;
    }
    if let Some(position) = change.position {
      item.position = position;
    }
    if let Some(assignee) = change.assignee_name {
      item.assignee_name = assignee;
    }
    if let Some(requester) = change.requester_name {
      item.requester_name = requester;
    }
    if let Some(scope) = change.scope {
      item.scope = scope;
    }
    if let Some(labels) = change.labels {
      item.labels = labels;
    }
    refuse_frozen_change(&mut tx, &before, &item).await?;
    refuse_unknown_label_values(&mut tx, Some(&before.labels), &item.labels).await?;
    match change.notices_off {
      Some(Some(off)) => {
        reason_given(&off.reason)?;
        item.notices_off = Some(NoticesOff {
          reason: off.reason,
          caller_name: caller.name.clone(),
          at: now,
        });
      }
      Some(None) => item.notices_off = None,
      None => {}
    }

    let approved = change.approval.is_some() && caller.approves;
    if let Some(approval) = change.approval {
      item.approvals.push(Approval {
        kind: caller.approval_kind(),
        caller_name: caller.name.clone(),
        at: approval.at.unwrap_or(now),
      });
    }

    let target = match change.state {
      Some(state) => Some(state),
      None if approved && item.state == ItemState::Draft => Some(ItemState::Open),
      None => None,
    };
    if change.merged_into.is_some() && target != Some(ItemState::Merged) {
      return Err(Refusal::bad_request("A mergedInto goes with the state merged."));
    }
    let holding = target == Some(ItemState::Held) && item.state != ItemState::Held;
    let hold_reason = match (holding, change.reason) {
      (true, Some(reason)) => {
        reason_given(&reason)?;
        Some(reason)
      }
      (true, None) => return Err(reason_required("A hold carries its reason, as reason.")),
      (false, Some(_)) => {
        return Err(Refusal::new(
          Code::ReasonWithoutHold,
          "A reason goes with a change that moves the item to held.",
        ));
      }
      (false, None) => None,
    };
    if let Some(target) = target
      && target != item.state
    {
      if !may_move(item.state, target) {
        return Err(Refusal::conflict(format!("An item {} cannot move to {}.", item.state, target)));
      }
      if target == ItemState::Done {
        let finals: i64 =
          sqlx::query_scalar(r#"SELECT COUNT(*) FROM "reports" WHERE "item_id" = $1 AND "kind" = 'final'"#)
            .bind(item.id)
            .fetch_one(&mut *tx)
            .await?;
        if finals == 0 {
          return Err(Refusal::conflict(
            "An item closes with a final report; write one before it is moved to done.",
          ));
        }
      }
      if item.state == ItemState::Draft
        && target == ItemState::Open
        && !item.approvals.iter().any(|approval| approval.kind == ApprovalKind::Approval)
      {
        return Err(Refusal::conflict("A draft opens with an approver's approval."));
      }
      if target == ItemState::Merged {
        let Some(into) = change.merged_into else {
          return Err(Refusal::bad_request("A merge names the item it goes into, as mergedInto."));
        };
        if into == item.id {
          return Err(Refusal::conflict("An item cannot be merged into itself."));
        }
        let Some(other) = find_item(&mut tx, into).await? else {
          return Err(Refusal::new(Code::UnknownItem, format!("There is no item {into} to merge into.")));
        };
        if matches!(other.state, ItemState::Dropped | ItemState::Merged) {
          return Err(Refusal::conflict(format!("The item {into} is {} and takes no merge.", other.state)));
        }
        item.merged_into = Some(into);
      }
      if matches!(target, ItemState::Held | ItemState::Done | ItemState::Dropped | ItemState::Merged) {
        end_standing_lease(&mut tx, &mut item, now).await?;
      }
      if target.is_final() {
        item.ended_at = Some(now);
      }
      item.hold = None;
      item.state = target;
    }
    if let Some(reason) = hold_reason {
      let held = NewReport {
        kind: ReportKind::Held,
        body: reason.clone(),
        actor_name: caller.name.clone().unwrap_or_else(|| OWN_NAME.to_owned()),
        labels: Default::default(),
      };
      write_report(&mut tx, now, &item, held).await?;
      item.hold = Some(ItemHold {
        reason,
        caller_name: caller.name.clone(),
        at: now,
      });
    }

    item.changed_at = now;
    save_item(&mut tx, &item).await?;
    told_changed(&mut tx, now, &item, previous, caller.name.as_deref()).await?;
    self.commit(tx).await?;
    Ok(item)
  }
}

// One item a change of several left as it stood, in the code and sentence its own change was refused in.
fn refused(item_id: i64, refusal: Refusal) -> bmo_job_client::ItemRefused {
  bmo_job_client::ItemRefused {
    item_id,
    code: refusal.code.as_str().to_owned(),
    error: refusal.sentence,
  }
}
