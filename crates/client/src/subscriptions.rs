//! Subscriptions: how a caller is told of changes, by webhook or as a stream (docs/api.md, "Subscriptions").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::{Labels, present};

wire_enum! {
  /// Whether a subscription is handed its events.
  SubscriptionState {
    Active = "active",
    Paused = "paused",
    Failing = "failing",
  }
}

/// A subscription.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
#[schemars(
  description = "What a subscriber is told of: events of the kinds listed, narrowed to the queues, scope and \
                 items named, an empty list or a null scope matching all. With a url each event is posted there \
                 as the subscription-event webhook; without one it is read on GET /events with subscriptionId. A \
                 subscription with ttlMs runs out at expiresAt unless a change renews it. A webhook that keeps \
                 failing moves it to failing, where it is still tried every five minutes and moved back to \
                 active by the first post taken; setting state back to active delivers again at once from the \
                 event after deliveredEventId."
)]
pub struct Subscription {
  pub id: i64,
  pub holder_name: String,
  pub kinds: Vec<String>,
  pub queue_names: Vec<String>,
  pub scope: Option<String>,
  pub item_ids: Vec<i64>,
  pub url: Option<String>,
  pub state: SubscriptionState,
  pub ttl_ms: Option<i64>,
  /// Epoch milliseconds.
  pub expires_at: Option<i64>,
  pub failure_count: i64,
  pub delivered_event_id: Option<i64>,
  /// The caller's own metadata.
  pub labels: Labels,
  /// Epoch milliseconds.
  pub created_at: i64,
  /// Epoch milliseconds.
  pub changed_at: i64,
}

/// A subscription made.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewSubscription {
  pub holder_name: String,
  /// Event kinds to hear of; empty for every kind.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub kinds: Vec<String>,
  /// Queues to hear of; empty for every queue.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub queue_names: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub scope: Option<String>,
  /// Items to hear of; empty for every item.
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub item_ids: Vec<i64>,
  /// An absolute http or https address to post each event to; left out, the subscription is read as a
  /// stream.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,
  /// The key the webhook's body is signed with.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub secret: Option<String>,
  /// The span, 1000 to 86400000; left out, the subscription stands until it is deleted.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub ttl_ms: Option<i64>,
  /// The caller's own metadata.
  #[serde(default, skip_serializing_if = "Labels::is_empty")]
  pub labels: Labels,
}

/// A change of a subscription, as a merge patch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionChange {
  /// Renews the subscription for this span from now; left out, for the span it stands with.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub ttl_ms: Option<i64>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub kinds: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub queue_names: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub item_ids: Option<Vec<i64>>,
  #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
  pub scope: Option<Option<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub state: Option<SubscriptionState>,
}

/// The filters of the subscription list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SubscriptionQuery {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub holder_name: Option<String>,
  /// One state, or several comma-separated.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub state: Option<String>,
  /// The key of the last subscription read; the page continues past it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// How many subscriptions at most, 50 where left out and at most 500.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
}
