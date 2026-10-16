//! Talks: the owner's conversation with the deployment's agent about an item or a document.

use serde::{Deserialize, Serialize};

/// One turn of a talk asked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Talk {
  pub topic: Topic,
  /// The turns said so far.
  pub turns: Vec<Turn>,
  /// What the owner says now.
  pub said: String,
}

/// What a talk is about.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Topic {
  Item(ItemTopic),
  Document(DocumentTopic),
}

/// An item talked about, as it stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemTopic {
  pub id: i64,
  pub title: String,
  pub done: bool,
  pub merged_into: Option<i64>,
  pub held: bool,
  /// Why it is held, where a reason is known: `stop`, `pause`, `failure` or any other hold.
  pub hold: Option<String>,
  /// Whether a run of it still ends while it is held.
  pub stopping: bool,
  pub approved: bool,
  pub status: String,
}

/// A document talked about, and the passage the owner selected in it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentTopic {
  pub document: String,
  pub label: String,
  pub writer: String,
  pub file: String,
  pub version: String,
  pub heading: Option<String>,
  pub passage: Option<String>,
  pub translated: bool,
}

/// One turn said before.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
  /// Whether the owner said it.
  pub owner: bool,
  pub text: String,
}
