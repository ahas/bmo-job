//! What came of telling the agent how the queue stands.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

/// Whether the queue's standing reached the agent, and why not where it did not.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum QueueTold {
  Told,
  NothingWaiting,
  AllBusy,
  Paused,
  Unreadable,
  MachineShort,
  Unchanged,
}
