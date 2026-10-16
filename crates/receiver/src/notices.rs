//! Voiced notices: a fact a room is told by the agent that speaks for it.

use job_codes::Fact;
use serde::{Deserialize, Serialize};

/// One notice to voice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Notice {
  pub fact: Fact,
  /// What the line is for, in English for the agent.
  pub intent: String,
  /// What follows it.
  pub follow_up: Option<String>,
  /// Whether the voice may leave it unsaid where the fact leaves the room nothing to hear.
  pub optional: bool,
  /// What it is about: voicing one replaces the pending one of the same kind in the room.
  pub kind: Option<String>,
  /// When it lapses.
  pub expires_at: Option<i64>,
  /// The events its delivery carries.
  pub events: Vec<String>,
  /// Until when it rides the next turn rather than opening one.
  pub rides_until: Option<i64>,
}
