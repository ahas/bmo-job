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

/// Why an item is held, kept from the change that held it until it moves out of `held`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemHold {
  /// The reason the change that held it carried.
  pub reason: String,
  /// The caller the request's token named; null for the shared token.
  pub caller_name: Option<String>,
  /// Epoch milliseconds.
  pub at: i64,
}

/// Why notices of an item are switched off, kept until a change switches them on again.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct NoticesOff {
  /// The reason the change that switched them off carried.
  pub reason: String,
  /// The caller the request's token named; null for the shared token.
  pub caller_name: Option<String>,
  /// Epoch milliseconds.
  pub at: i64,
}

/// Notices of an item switched off as a change gives it: the reason alone. Who switched them off is the caller
/// the request's token names, and when is the time of the change.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NoticesOffGiven {
  pub reason: String,
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
  /// How many times its work was sent back: the `rejection` entries of its review, read and never written.
  #[serde(default)]
  pub rejections: i64,
  pub merged_into: Option<i64>,
  /// The item this one was raised from, fixed when it is raised; null where there is none.
  pub raised_by: Option<i64>,
  pub lease_id: Option<i64>,
  /// Why it is held, while it is; null otherwise.
  #[serde(default)]
  pub hold: Option<ItemHold>,
  /// Why notices of it are switched off, while they are; null while they are on. What a notice is is the
  /// caller's to decide.
  #[serde(default)]
  pub notices_off: Option<NoticesOff>,
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
  /// The standing item this one is raised from. Left out, it is the item of the one lease the caller the token
  /// names holds, where that caller holds exactly one, and none otherwise.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub raised_by: Option<i64>,
}

/// An item as `item.created` and `item.changed` tell it: the item as a GET answers it, the state it left and who
/// changed it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemEvent {
  #[serde(flatten)]
  pub item: Item,
  /// The state the item stood in before the change; null where its state did not move, and on a raise.
  pub previous_state: Option<ItemState>,
  /// The caller the request's token named; null for the shared token, and `bmo-job` for the service's own
  /// sweep.
  pub changed_by: Option<String>,
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
  /// Why the item is held: required with a state that moves it to held, and taken with nothing else.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub reason: Option<String>,
  /// Switches notices of the item off with a reason, or on again with null.
  #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
  pub notices_off: Option<Option<NoticesOffGiven>>,
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
  /// The key of the item the items were raised from.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub raised_by: Option<i64>,
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

/// Several items' labels changed at once: the items by their numbers, and a merge of label keys, a key given a value
/// set to it and a key given null taken off, every other key standing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemsRelabel {
  pub ids: Vec<i64>,
  pub labels: std::collections::BTreeMap<String, Option<String>>,
}

/// What a change of several items' labels came to: the items changed, as they now stand, and each item refused,
/// with the code and sentence its own change would have been refused in.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemsRelabeled {
  pub changed: Vec<Item>,
  pub refused: Vec<ItemRefused>,
}

/// One item a change of several left as it stood, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ItemRefused {
  pub item_id: i64,
  pub code: String,
  pub error: String,
}
