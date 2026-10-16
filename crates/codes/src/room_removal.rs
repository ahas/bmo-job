//! A room about to be removed: what the service still holds of it.

use serde::{Deserialize, Serialize};
use strum::{IntoStaticStr, VariantNames};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RoomHoldings {
  RoomOpenItems { count: u32 },
  RoomWaitingQuestions { count: u32 },
}

coded!(RoomHoldings);
