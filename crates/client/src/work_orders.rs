//! Work orders and their tasks (docs/api.md, "Work orders and tasks").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::present;

wire_enum! {
  /// Where a work order stands.
  WorkOrderState {
    Draft = "draft",
    Saved = "saved",
    Running = "running",
    Done = "done",
  }
}

impl WorkOrderState {
  /// Whether a work order may move from this state to `to`.
  pub fn may_move_to(self, to: Self) -> bool {
    use WorkOrderState::*;
    matches!(
      (self, to),
      (Draft, Saved)
        | (Saved, Draft)
        | (Saved, Running)
        | (Running, Saved)
        | (Running, Done)
        | (Done, Running)
    )
  }
}

wire_enum! {
  /// Where a task stands.
  TaskState {
    Waiting = "waiting",
    Current = "current",
    Done = "done",
    Skipped = "skipped",
  }
}

/// An item's work order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct WorkOrder {
  pub item_id: i64,
  pub purpose: String,
  pub directive: String,
  /// The lines of the plan, in order; the step at place n is the task of stage n.
  pub steps: Vec<String>,
  pub state: WorkOrderState,
  /// Epoch milliseconds.
  pub changed_at: i64,
}

/// A work order saved whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkOrderBody {
  pub purpose: String,
  pub directive: String,
  pub steps: Vec<String>,
  /// The standing state where left out, or draft for a new work order.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub state: Option<WorkOrderState>,
}

/// A change of a work order, as a merge patch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkOrderChange {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub state: Option<WorkOrderState>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub purpose: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub directive: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub steps: Option<Vec<String>>,
}

/// A task of a work order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Task {
  pub id: i64,
  pub item_id: i64,
  pub stage: i64,
  pub title: String,
  pub state: TaskState,
  pub assignee_name: Option<String>,
  /// The other item's work the step waits on; null where it waits on none.
  pub after: Option<TaskAfter>,
  /// Epoch milliseconds.
  pub changed_at: i64,
}

/// The work a step waits on: it is not to be started before the item's commit stands on the main branch of the
/// repository, or the item has ended. The service keeps it and judges nothing by it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskAfter {
  pub item_id: i64,
  pub repository: String,
}

/// One step added behind a work order's plan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewStep {
  pub title: String,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<TaskAfter>,
  /// The title of the work already under way, the first step of the work order made where the item has none;
  /// not read where one stands.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub standing: Option<String>,
}

/// A move of a task, as a merge patch.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskChange {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub state: Option<TaskState>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub stage: Option<i64>,
  #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
  pub assignee_name: Option<Option<String>>,
  /// The work the step waits on; null clears it.
  #[serde(default, deserialize_with = "present", skip_serializing_if = "Option::is_none")]
  pub after: Option<Option<TaskAfter>>,
}

/// The filters of the task list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskQuery {
  /// The stage under way alone: the lowest stage holding a task waiting or current.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub current: Option<bool>,
  /// The key of the last task read; the page continues past its place.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// How many tasks at most, 50 where left out and at most 500.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
}
