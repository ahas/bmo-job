//! The agent log: every line an agent's run wrote, read for what a job's run has done. bmo-agency keeps a
//! line's moment in epoch seconds; the port hands it on in epoch milliseconds, as the service keeps every
//! other moment.

use serde::{Deserialize, Serialize};

/// A line's moment: epoch seconds on the wire, epoch milliseconds in the service.
mod seconds {
  use serde::{Deserialize, Deserializer, Serializer};

  pub fn serialize<S: Serializer>(at: &i64, to: S) -> Result<S::Ok, S::Error> {
    to.serialize_i64(at.div_euclid(1000))
  }

  pub fn deserialize<'de, D: Deserializer<'de>>(from: D) -> Result<i64, D::Error> {
    Ok(i64::deserialize(from)?.saturating_mul(1000))
  }
}

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
  /// Epoch milliseconds.
  #[serde(with = "seconds")]
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
  /// The name the line is kept under.
  pub run: Option<String>,
  pub reference: Option<String>,
  pub name: String,
  pub source: Option<String>,
  /// Epoch milliseconds.
  #[serde(with = "seconds")]
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
  /// The name the line is kept under.
  pub run: Option<String>,
  pub reference: Option<String>,
  /// False where the result was cut.
  pub complete: bool,
  pub length: Option<u64>,
  /// Epoch milliseconds.
  #[serde(with = "seconds")]
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
