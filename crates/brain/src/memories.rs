//! Memories: the rows an agent keeps, each standing in front of it in its room until it is checked off.

use serde::{Deserialize, Serialize};

/// The turn a memory tool call is made in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Turn {
  /// The agent the turn is for, by its key.
  pub agent: String,
  /// The turn's own room; none for a call made in no turn.
  pub chat_id: Option<i64>,
  /// The message the turn answers; none for a call made in no turn.
  pub message_id: Option<i64>,
}

/// A memory to file, as the memory tool's arguments.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryAdd {
  /// The row's kind, such as `followup`.
  #[serde(rename = "type")]
  pub kind: String,
  /// What the agent is to do.
  pub action: String,
  /// The item the row is tied to.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub todo: Option<i64>,
  /// The room the row is filed in, where it is another than the turn's.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub chat: Option<i64>,
  /// The row's name; bmo-brain names it after its number where none is given.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub slug: Option<String>,
}

/// A memory as the service reads it: the fields it reads of the row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
  pub id: i64,
  pub slug: String,
  /// The row's kind, such as `followup`.
  #[serde(rename = "type")]
  pub kind: String,
  /// The room the row stands in; none for a row tied to no room.
  pub chat_id: Option<i64>,
  pub action: String,
  /// The item the row is tied to.
  pub todo_id: Option<i64>,
  pub active: bool,
  /// When it was filed, in Unix seconds.
  pub set_at: i64,
  /// When it was checked off, in Unix seconds.
  pub done_at: Option<i64>,
}
