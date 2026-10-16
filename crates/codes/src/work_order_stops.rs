//! Why a work order stopped where its room is told.

use serde::{Deserialize, Serialize};
use strum::{IntoStaticStr, VariantNames};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum WorkOrderStop {
  FinalNotClosed { item: i64 },
}

coded!(WorkOrderStop);
