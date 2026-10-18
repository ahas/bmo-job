//! Executions: the runs of agents' processes bmo-agency launched and records.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// One execution. The fields the service decides on are typed; every other field bmo-agency records of
/// it stands in `details` as it was given.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Execution {
  pub id: String,
  pub parent: Option<String>,
  pub root: Option<String>,
  pub line: Option<String>,
  /// True where it runs in a pane it shares.
  #[serde(default)]
  pub shared_pane: bool,
  pub started_at: Option<i64>,
  pub ended_at: Option<i64>,
  /// The outcome it recorded as it ended.
  pub outcome: Option<Map<String, Value>>,
  /// True where a cancel asked of it, or of an execution that covers it, still stands.
  #[serde(default)]
  pub cancelled: bool,
  #[serde(default)]
  pub details: Map<String, Value>,
}
