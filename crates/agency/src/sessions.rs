//! Agent sessions: one agent's conversation on a provider, kept by bmo-agency and carried on its session
//! contract, and the starts and ends of their turns its event stream tells.

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

/// The parts of a turn's context the service hands in beside the prompt: on a worker's turn, the job it is a turn
/// of and the item that job works.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnContext {
  pub job: Option<i64>,
  pub item: Option<i64>,
}

/// One turn sent into a session: its prompt and its context.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
  pub session: SessionId,
  pub prompt: String,
  #[serde(flatten)]
  pub context: TurnContext,
}

/// A turn put into a session began: the session and the run it goes out as.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnStarted {
  pub session: SessionId,
  pub run: String,
  pub at: i64,
}

/// What a turn came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnOutcome {
  Answered,
  /// Its program waits on a person.
  Blocked,
  /// It was ended from outside.
  Stopped,
  Failed,
}

/// The error an engine ended a turn with: its kind as the program names it, none where it named none.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineError {
  pub kind: Option<String>,
}

/// How a turn put into a session ended: its answer where it answered, why where it failed, and the engine's
/// error where its engine ended it with one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnEnded {
  pub session: SessionId,
  pub run: String,
  pub outcome: TurnOutcome,
  pub answer: Option<String>,
  pub reason: Option<String>,
  pub engine_error: Option<EngineError>,
}

/// One event of a session's turns, as the event stream tells it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TurnEvent {
  Started(TurnStarted),
  Ended(TurnEnded),
}

impl TurnEvent {
  /// The session the event is of.
  pub fn session(&self) -> &SessionId {
    match self {
      Self::Started(started) => &started.session,
      Self::Ended(ended) => &ended.session,
    }
  }
}
