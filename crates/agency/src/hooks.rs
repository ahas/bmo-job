//! The hook points bmo-agency hands a decision up at, which a consumer registers its own address at.

use serde::{Deserialize, Serialize};

/// A hook point, and whether a consumer is registered at it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookPoint {
  pub point: String,
  pub registered: bool,
}
