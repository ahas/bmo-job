//! A work order in its worker's line: the answer to a resume the queue holds, and what holds it.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum WorkOrderAnswer {
  ResumedHeld { item: i64, hold: QueueHold },
}

coded!(WorkOrderAnswer);

/// What holds the queue still.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum QueueHold {
  OwnerPause,
  DeployHold,
}
