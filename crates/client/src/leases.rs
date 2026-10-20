//! Leases: an item held by one holder for a span (docs/api.md, "Leases").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
  /// The item as it stands.
  pub item: Item,
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
}

/// How long to wait for an item where none is open.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseWait {
  /// How long to wait for an open item where none waits, at most 60000.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub wait_ms: Option<i64>,
}

/// A lease renewed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LeaseRenewal {
  /// The new span from now, 1000 to 3600000.
  pub ttl_ms: i64,
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
