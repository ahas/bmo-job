//! The run ledger: an agent's unit started, how deep it was, and how it ended.

use serde::{Deserialize, Serialize};

/// What a run is opened with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunOpening {
  pub agent: String,
  pub chat_id: Option<i64>,
  pub unit: String,
  pub depth: u32,
  pub cap_ms: Option<i64>,
  pub started_at: i64,
  pub started_by: Option<String>,
  pub search_id: Option<String>,
}

/// What a run is closed with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunClosing {
  pub finished_at: i64,
  pub outcome: String,
  pub context_tokens: Option<u64>,
}

/// One run of the ledger.
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
