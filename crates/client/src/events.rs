//! Events: the service's one stream, and every kind it tells (docs/api.md, "Events").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

wire_enum! {
  /// Every kind of event the service tells of its own members.
  EventKind {
    ItemCreated = "item.created",
    ItemChanged = "item.changed",
    ItemDeleted = "item.deleted",
    NoteCreated = "note.created",
    NoteDeleted = "note.deleted",
    ReportCreated = "report.created",
    WorkOrderChanged = "work-order.changed",
    TaskChanged = "task.changed",
    QueueChanged = "queue.changed",
    LeaseCreated = "lease.created",
    LeaseExpired = "lease.expired",
    LeaseDeleted = "lease.deleted",
    QuestionCreated = "question.created",
    QuestionChanged = "question.changed",
    QuestionDue = "question.due",
    QuestionDeleted = "question.deleted",
    ReplyCreated = "reply.created",
    SubscriptionCreated = "subscription.created",
    SubscriptionChanged = "subscription.changed",
    SubscriptionDeleted = "subscription.deleted",
  }
}

/// One event: its number, its kind, when it happened, and the member it tells of.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Event {
  /// The service's event number, which only grows.
  pub id: i64,
  /// `<resource>.<change>`.
  pub kind: String,
  /// Epoch milliseconds.
  pub at: i64,
  /// The member as a GET answers it, or its key alone for a deleted kind. An item's `item.created` and
  /// `item.changed` carry beside its fields `previousState`, the state it left or null, and `changedBy`, the
  /// caller the token named or null.
  pub data: Map<String, Value>,
}

impl Event {
  /// The key of the item the event belongs to, where its member names one.
  pub fn item_id(&self) -> Option<i64> {
    if self.kind.starts_with("item.") {
      return self.data.get("id").and_then(Value::as_i64);
    }
    self.data.get("itemId").and_then(Value::as_i64)
  }

  /// The queue the event belongs to, where its member names one: its `queueName`, or a queue's own name.
  pub fn queue_name(&self) -> Option<&str> {
    let named = self.data.get("queueName").and_then(Value::as_str);
    named.or_else(|| {
      self
        .kind
        .starts_with("queue.")
        .then(|| self.data.get("name").and_then(Value::as_str))
        .flatten()
    })
  }
}

/// An event a process of the service's own publishes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewEvent {
  /// `<resource>.<change>`.
  pub kind: String,
  /// The member it tells of; its `queueName`, `scope` and `itemId` narrow it.
  pub data: Map<String, Value>,
}

/// The filters and cursor of the event stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EventQuery {
  /// Event kinds, comma-separated.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub kinds: Option<String>,
  /// The number of the last event read; the stream continues past it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// How many events at most in a page, 50 where left out and at most 500.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub scope: Option<String>,
  /// The events a subscription without a url matches, the stream form of push.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub subscription_id: Option<i64>,
}
