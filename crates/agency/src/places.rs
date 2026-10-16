//! The places work stands in: a worker's workbench and a pool's branch slots, as absolute paths on the
//! machine.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A worker's workbench and the item scratch directories under it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workbench {
  pub root: PathBuf,
  #[serde(default)]
  pub scratches: Vec<PathBuf>,
}

/// One branch slot of a pool.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Slot {
  pub slot: u32,
  pub dir: PathBuf,
}

/// A pool's branch slots under their root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BranchSlots {
  pub root: PathBuf,
  #[serde(default)]
  pub slots: Vec<Slot>,
}
