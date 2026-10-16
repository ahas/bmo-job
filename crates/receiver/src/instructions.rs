//! Instructions: what an agent's session in a room is handed to act on, and how far one has gone.

use job_codes::Fact;
use serde::{Deserialize, Serialize};

/// What an instruction hands the session: a fact to tell the room, or an order for the agent itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Said {
  /// A fact, which bmo-receiver frames and its agent says in the room's language.
  Fact(Fact),
  /// The words of an order, in English for the agent; no person reads them.
  Order(String),
}

/// One instruction to queue.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instruction {
  /// The agent whose session takes it, the deployment's agent where none.
  pub agent: Option<String>,
  /// The room, the owner's private chat where none.
  pub chat: Option<i64>,
  pub said: Said,
  /// What it is about: queueing one replaces the pending instruction of the same kind in the room.
  pub kind: Option<String>,
  /// Its family, such as `job`, `overtime` or `run_failure`.
  pub sort: String,
  /// The line it is listed under, made from the fact where none.
  pub summary: Option<String>,
  /// When it lapses.
  pub expires_at: Option<i64>,
  /// The events its delivery carries.
  pub events: Vec<String>,
  /// Until when it rides the next turn rather than opening one.
  pub rides_until: Option<i64>,
  /// The road the session was read on, which it is delivered on.
  pub via: Option<String>,
  /// The run whose result it is.
  pub run: Option<i64>,
}

/// How far an instruction has gone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstructionState {
  /// Queued, not yet taken by a turn.
  Pending,
  /// Handed to a turn.
  Handed,
  /// Delivered to the room.
  Delivered,
}
