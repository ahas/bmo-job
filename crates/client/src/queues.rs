//! Queues: where items wait, paused or open, and the holds on them (docs/api.md, "Queues").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

wire_enum! {
  /// Whether a queue gives leases.
  QueueState {
    Open = "open",
    Paused = "paused",
  }
}

/// A hold on a queue: while it stands, the queue gives no lease.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Hold {
  pub reason: String,
  pub actor_name: String,
  /// Epoch milliseconds after which the hold no longer stands, or null for one that stands until released.
  #[serde(default)]
  pub until: Option<i64>,
}

/// A queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Queue {
  pub name: String,
  pub state: QueueState,
  pub holds: Vec<Hold>,
  pub open_count: i64,
  pub leased_count: i64,
  /// Epoch milliseconds.
  pub changed_at: i64,
}

/// The cursor of the queue list, which is in name order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueQuery {
  /// The name of the last queue read; the page continues past it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub after: Option<String>,
  /// How many queues at most, 50 where left out and at most 500.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
}

/// A change of a queue, as a merge patch: its state, and its holds replaced whole.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueueChange {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub state: Option<QueueState>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub holds: Option<Vec<Hold>>,
}
