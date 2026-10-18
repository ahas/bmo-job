//! The agent log: every line an agent's run wrote, read for what a job's run has done.

use serde::{Deserialize, Serialize};

/// A note written into a run's log beside the lines the run wrote itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogNote {
  pub agent: String,
  pub run: String,
  pub chat: Option<i64>,
  pub job: Option<i64>,
  pub text: String,
  pub source: Option<String>,
  pub state: Option<String>,
}

/// A job's last line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
  pub id: i64,
  pub at: i64,
  pub kind: String,
  pub name: Option<String>,
}

/// One top-level tool call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCall {
  pub id: i64,
  pub agent: String,
  pub run: Option<i64>,
  pub reference: Option<String>,
  pub name: String,
  pub source: Option<String>,
  pub at: i64,
  /// The command it ran, where it ran one.
  pub command: Option<String>,
  /// Its arguments, cut to 2000 characters.
  pub args: Option<String>,
}

/// One tool result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolResult {
  pub id: i64,
  pub agent: String,
  pub run: Option<i64>,
  pub reference: Option<String>,
  /// False where the result was cut.
  pub complete: bool,
  pub length: Option<u64>,
  pub at: i64,
}

/// What a job's run wrote: its tool calls and results, and how many requests and compactions it made.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobLines {
  #[serde(default)]
  pub calls: Vec<ToolCall>,
  #[serde(default)]
  pub results: Vec<ToolResult>,
  pub requests: u64,
  pub compactions: u64,
}
