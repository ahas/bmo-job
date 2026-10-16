//! Knowledge for a piece of work: the pieces of the brain's libraries bmo-gate picks for the work's lines.

use serde::{Deserialize, Serialize};

/// What a piece of work asks bmo-gate to pick knowledge for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct KnowledgeAsked {
  /// The agent whose libraries the pieces are picked from, by its key.
  pub agent: String,
  /// The room the work belongs to, which decides what the agent may be offered.
  pub chat: i64,
  /// The work's own lines: a job's item and task, or a search's query.
  pub lines: Vec<String>,
}

/// One piece bmo-gate picked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Piece {
  /// Where the piece stands in the brain.
  pub path: String,
  pub text: String,
}
