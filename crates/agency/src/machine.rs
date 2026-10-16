//! The room on the machine the service runs on, and the notice that launches are held for want of it.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The room on the machine. Every reading the service does not decide on stands in `details` as it was
/// given.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Room {
  /// Why launches are held now; empty where there is room.
  #[serde(default)]
  pub held: Vec<String>,
  pub jobs: u64,
  pub browser_jobs: u64,
  pub browsers: u64,
  #[serde(default)]
  pub details: Map<String, Value>,
}

/// Launches held, and why. An empty `held` withdraws the notice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchHeld {
  pub at: i64,
  pub held: Vec<String>,
}
