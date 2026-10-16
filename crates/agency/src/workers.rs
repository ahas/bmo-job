//! Workers' runtime: what bmo-agency holds of a worker as it runs.

use serde::{Deserialize, Serialize};

/// The provider, and the model where one is named, a worker runs on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pin {
  pub provider: String,
  /// None leaves the model to the route.
  pub model: Option<String>,
}

/// A worker's runtime. The pin is the one in force: a support worker with none of its own is answered
/// its base worker's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRuntime {
  pub worker: String,
  pub pin: Option<Pin>,
  pub keepalive_paused_at: Option<i64>,
  pub context_tokens: Option<u64>,
}
