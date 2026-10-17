//! A work order stopping, set aside, resuming and moving on, as the agent is told it in the owner's chat.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum WorkOrderStop {
  FinalNotClosed {
    item: i64,
  },
  OrderStopped {
    item: i64,
    worker: Option<String>,
    phase: Option<String>,
    reason: StopReason,
  },
  StageRunaway {
    item: i64,
    worker: String,
    stage: Option<String>,
    unit: Option<String>,
    cause: RunawayCause,
    starts: u64,
    minutes: u64,
    action: RunawayAction,
  },
  OrderSetAside {
    item: i64,
    worker: Option<String>,
    failures: u32,
  },
  OrderResuming {
    item: i64,
    worker: Option<String>,
    stages: Vec<String>,
    failures: u32,
    retry: RetryStep,
  },
  StageRerouted {
    item: i64,
    worker: Option<String>,
    stage: String,
    from: String,
    to: String,
  },
}

coded!(WorkOrderStop);

/// Why a work order stopped.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StopReason {
  LeftRunning { line: String },
  Runaway { line: String },
  NoRoute,
  StageFailed,
}

/// What made a running stage a runaway.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RunawayCause {
  Spin,
  Clock,
}

/// What was done about a runaway stage.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RunawayAction {
  Told,
  Stopped,
}

/// How a stopped work order is retried at its turn.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RetryStep {
  Again,
  FreshSession,
  OtherModel,
}
