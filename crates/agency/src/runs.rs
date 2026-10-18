//! The run ledger: an agent's unit started, how deep it was, and how it ended. Its times are epoch seconds,
//! as the ledger keeps them.

use serde::{Deserialize, Serialize};

/// What a run is opened with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOpening {
  pub agent: String,
  pub chat_id: Option<i64>,
  pub unit: String,
  pub depth: u32,
  pub cap_ms: Option<i64>,
  /// When the run started, in epoch seconds.
  pub started_at: i64,
  pub started_by: Option<String>,
  /// The number of the search the run carries, as bmo-artifact numbers its searches.
  pub search_id: Option<i64>,
}

/// What a run is closed with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunClosing {
  /// When the run finished, in epoch seconds.
  pub finished_at: i64,
  pub outcome: String,
  pub context_tokens: Option<u64>,
}

/// One run of the ledger, its times in epoch seconds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
  pub id: i64,
  pub agent: String,
  pub chat_id: Option<i64>,
  pub unit: String,
  pub depth: u32,
  pub started_at: i64,
  pub finished_at: Option<i64>,
  pub outcome: Option<String>,
  pub context_tokens: Option<u64>,
}
