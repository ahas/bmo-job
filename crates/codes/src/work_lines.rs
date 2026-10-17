//! A job's line in a listing: which step of a work order it is and what it is labelled by.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

/// The step of a work order a job runs.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum WorkStep {
  Plan,
  Investigate,
  Review,
  Synthesis,
  Build,
  Final,
}

/// Why a worker's turn went past a stopped work order.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PassReason {
  Closing,
  NoRoute,
  JustStopped,
  WaitingAfterStop,
}

/// What a job is labelled by.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum JobLabel {
  Item { id: i64, attempt: Option<u32> },
  Itemless,
}
