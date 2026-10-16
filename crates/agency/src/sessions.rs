//! Agent sessions: one agent's conversation on a provider, kept by bmo-agency.

use std::fmt;

use serde::{Deserialize, Serialize};

/// A session's id, as bmo-agency gave it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(pub String);

impl fmt::Display for SessionId {
  fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
    formatter.write_str(&self.0)
  }
}

/// Whom a session is started for.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Scope {
  /// The agent itself.
  Agent,
  /// One room.
  Room { room: String },
  /// A worker.
  Worker,
}

/// What a session is started with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Start {
  pub agent: String,
  pub scope: Scope,
}

/// A session as bmo-agency holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
  pub id: SessionId,
  pub agent: String,
  pub scope: Scope,
}

/// Whether a session took a turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
  Taken,
  /// The session cannot take a turn now.
  Blocked,
}

/// The tokens a session carries and the most it may.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Context {
  pub carried: u64,
  pub window: u64,
}

/// What a session runs on now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionState {
  pub session: SessionId,
  pub provider: String,
  pub model: String,
  pub place: String,
  /// How many times the session has started over.
  pub generation: u64,
  pub context: Option<Context>,
}

/// What a session has written in its latest turn so far.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Output {
  pub session: SessionId,
  pub generation: u64,
  pub text: String,
}
