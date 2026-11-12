//! Leases: an item held by one holder for a span (docs/api.md, "Leases").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::present;
use crate::items::Item;

/// A lease and the item it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Lease {
  pub id: i64,
  pub queue_name: String,
  pub item_id: i64,
  pub holder_name: String,
  pub ttl_ms: i64,
  /// Epoch milliseconds.
  pub expires_at: i64,
  /// Epoch milliseconds.
  pub created_at: i64,
  /// What of its item the lease holds fixed while it stands, or null for nothing.
  pub frozen: Option<Frozen>,
  /// The item as it stands.
  pub item: Item,
}

/// What of its item a lease holds fixed while it stands: a change giving any of it another value is refused.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Frozen {
  /// The item's fields held fixed.
  #[serde(default)]
  pub fields: Vec<ItemField>,
  /// The label keys whose values are held fixed, a key the item does not carry held as absent.
  #[serde(default)]
  pub labels: Vec<String>,
  /// The holder's own sentence for why, or null.
  #[serde(default)]
  pub reason: Option<String>,
}

/// An item field a lease may hold fixed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ItemField {
  Title,
  Brief,
  QueueName,
  Priority,
  Position,
  AssigneeName,
  RequesterName,
  Scope,
}

impl ItemField {
  /// The field's name as a body carries it.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::Title => "title",
      Self::Brief => "brief",
      Self::QueueName => "queueName",
      Self::Priority => "priority",
      Self::Position => "position",
      Self::AssigneeName => "assigneeName",
      Self::RequesterName => "requesterName",
      Self::Scope => "scope",
    }
  }

  /// Whether the field holds another value in `after` than in `before`.
  pub fn differs(self, before: &Item, after: &Item) -> bool {
    match self {
      Self::Title => before.title != after.title,
      Self::Brief => before.brief != after.brief,
      Self::QueueName => before.queue_name != after.queue_name,
      Self::Priority => before.priority != after.priority,
      Self::Position => before.position != after.position,
      Self::AssigneeName => before.assignee_name != after.assignee_name,
      Self::RequesterName => before.requester_name != after.requester_name,
      Self::Scope => before.scope != after.scope,
    }
  }
}

/// A lease asked for: the next open item of a queue, or the one item named.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseAsk {
  pub queue_name: String,
  pub holder_name: String,
  /// The span, 1000 to 3600000.
  pub ttl_ms: i64,
  /// Take items assigned to this name; left out, items assigned to no one.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub assignee_name: Option<String>,
  /// The one item to take, as a push named it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub item_id: Option<i64>,
  /// What of the item the lease holds fixed while it stands.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub frozen: Option<Frozen>,
}

/// How long to wait for an item where none is open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseWait {
  /// How long to wait for an open item where none waits, at most 60000.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub wait_ms: Option<i64>,
}

/// A lease renewed, and what it holds fixed set where it is carried.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseRenewal {
  /// The new span from now, 1000 to 3600000.
  pub ttl_ms: i64,
  /// What of its item the lease holds fixed from now, in place of what stood; null for nothing, left out to
  /// leave it as it stands.
  #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
  pub frozen: Option<Option<Frozen>>,
}

/// The filters of the lease list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseQuery {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub holder_name: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub queue_name: Option<String>,
  /// The key of the last lease read; the page continues past it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// How many leases at most, 50 where left out and at most 500.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
}
