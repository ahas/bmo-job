//! Turn records: one thing a run wrote down, kept under the run's name for the checks on that turn's answer.

use serde::{Deserialize, Serialize};

/// What a run wrote down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnRecordKind {
  /// A row filed.
  TodoAdd,
  /// Words appended to a row.
  TodoAppend,
}

/// One thing a run wrote down, under the name the run worked under.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnRecord {
  pub run: String,
  pub kind: TurnRecordKind,
  /// What was written: the row's title or the words appended.
  pub text: String,
  /// The row it landed in.
  pub record_id: Option<i64>,
  /// Whether it set something that wakes the agent.
  pub wakes: bool,
  pub at: i64,
}
