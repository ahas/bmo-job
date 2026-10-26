//! Items: one piece of work each, its place in a queue, its approvals and its state (docs/api.md, "Items").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::{Labels, Unread, present};

wire_enum! {
  /// Where an item stands in its life.
  ItemState {
    Draft = "draft",
    Open = "open",
    Leased = "leased",
    Held = "held",
    Done = "done",
    Dropped = "dropped",
    Merged = "merged",
  }
}

impl ItemState {
  /// Whether the item has ended: done, dropped or merged, from which it never moves.
  pub fn is_final(self) -> bool {
    matches!(self, Self::Done | Self::Dropped | Self::Merged)
  }
}

wire_enum! {
  /// What an approval counts as, fixed when it is given by whether its caller is an approver.
  ApprovalKind {
    /// Given by a caller the deployment marks as an approver; it opens a draft.
    Approval = "approval",
    /// Given by any other caller, or with the shared token; it is recorded and moves nothing.
    Recommendation = "recommendation",
  }
}

/// One approval given to an item: what it counts as, the caller the request's token named, and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Approval {
  /// `approval` where the caller is an approver, which opens a draft; `recommendation` otherwise.
  pub kind: ApprovalKind,
  /// The caller the request's token named; null for the shared token or an approval recorded with no caller.
  pub caller_name: Option<String>,
  /// Epoch milliseconds.
  pub at: i64,
}

/// An approval as a change gives it: when, which is the time of the change where left out. Who gives it is the
/// caller the request's token names, never a field of the body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ApprovalGiven {
  /// Epoch milliseconds.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub at: Option<i64>,
  /// A name a body may carry beside the time, read and dropped: it names nobody.
  #[serde(default, rename = "actorName", skip_serializing)]
  #[schemars(skip)]
  typed_name: Unread,
}

impl ApprovalGiven {
  /// An approval given at `at`, or at the time of the change where it is none.
  pub fn at(at: Option<i64>) -> Self {
    Self {
      at,
      typed_name: Unread,
    }
  }
}

/// An item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(
  description = "One piece of work. States and moves: draft to open (an approval of the kind approval), open to leased (a lease), \
                 leased to open (the lease given back or run out), open or leased to held and held to open, \
                 leased to done, and any state but done, dropped and merged to dropped or merged."
)]
pub struct Item {
  pub id: i64,
  pub title: String,
  pub brief: String,
  pub state: ItemState,
  pub queue_name: String,
  pub position: i64,
  pub priority: i64,
  pub assignee_name: Option<String>,
  pub requester_name: Option<String>,
  pub scope: Option<String>,
  /// The caller's own metadata.
  pub labels: Labels,
  pub approvals: Vec<Approval>,
  pub cowork: bool,
  pub merged_into: Option<i64>,
  pub lease_id: Option<i64>,
  /// Epoch milliseconds.
  pub created_at: i64,
  /// Epoch milliseconds.
  pub changed_at: i64,
  /// Epoch milliseconds.
  pub ended_at: Option<i64>,
}

/// An item raised.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewItem {
  pub title: String,
  pub brief: String,
  pub queue_name: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub priority: Option<i64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub assignee_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub requester_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub scope: Option<String>,
  /// The caller's own metadata.
  #[serde(default, skip_serializing_if = "Labels::is_empty")]
  pub labels: Labels,
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub cowork: bool,
}

/// A change of an item, as a merge patch: a field left out stays, a nullable field set to null is cleared.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemChange {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub state: Option<ItemState>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub approval: Option<ApprovalGiven>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub merged_into: Option<i64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub title: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub brief: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub queue_name: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub priority: Option<i64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub position: Option<i64>,
  #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
  pub assignee_name: Option<Option<String>>,
  #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
  pub requester_name: Option<Option<String>>,
  #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
  pub scope: Option<Option<String>>,
  /// The caller's own metadata, replaced whole.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub labels: Option<Labels>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub cowork: Option<bool>,
}

/// The filters of the item list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemQuery {
  /// One state, or several comma-separated.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub state: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub queue_name: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub assignee_name: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub scope: Option<String>,
  /// A label's key, or its key and value as `key=value`.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub label: Option<String>,
  /// Item keys, comma-separated.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub ids: Option<String>,
  /// Words found in the title or the brief, in any case.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub q: Option<String>,
  /// The key of the last item read; the page continues past its place in queue order.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// How many items at most, 50 where left out and at most 500.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
}
