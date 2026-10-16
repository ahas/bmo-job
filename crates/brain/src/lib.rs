//! The service's port to bmo-brain (docs/api.md, "bmo-brain"): every call it makes there, declared by the
//! trait `Brain` and made over HTTP by `BrainClient`. The shapes the calls carry stand in one module to an
//! area.

pub mod client;
pub mod holds;
pub mod memories;

use std::future::Future;

pub use client::BrainClient;
pub use holds::{Hold, Holds, Waited};
pub use memories::{Memory, MemoryAdd, Turn};

/// Why a call on bmo-brain failed. Each names the call it was.
#[derive(Debug, thiserror::Error)]
pub enum BrainError {
  /// The address given is no `http:` address a path can be added to.
  #[error("{address} is not an http address bmo-brain can be reached at")]
  NotAnAddress { address: String },
  /// The call did not reach bmo-brain, or no answer came back.
  #[error("{call}: bmo-brain did not answer: {error}")]
  Unreachable { call: &'static str, error: String },
  /// bmo-brain refused the call, with its status, code and sentence as it gave them: a refusal it answered
  /// at 200 as well as a status that is no success.
  #[error("{call}: bmo-brain refused with {status} {code}: {error}")]
  Refused {
    call: &'static str,
    status: u16,
    code: String,
    error: String,
  },
  /// bmo-brain answered with a body that does not read as the call's answer or as a refusal.
  #[error("{call}: bmo-brain answered {status} with a body that does not read: {error}")]
  Malformed {
    call: &'static str,
    status: u16,
    error: String,
  },
}

/// What a call on bmo-brain comes back with.
pub type Answer<T> = Result<T, BrainError>;

/// Every call the service makes on bmo-brain.
pub trait Brain: Send + Sync {
  /// The brain held for `run`, the name of a job's run that writes into it, and the editing locks taken
  /// once the hold was set.
  fn hold(&self, run: &str) -> impl Future<Output = Answer<Hold>> + Send;

  /// The runs holding the brain and the editing locks taken now.
  fn holds(&self) -> impl Future<Output = Answer<Holds>> + Send;

  /// Waits, for nine minutes at most, until no brain editor holds an editing lock.
  fn wait(&self) -> impl Future<Output = Answer<Waited>> + Send;

  /// Every memory `agent` keeps in the room `chat`, newest first.
  fn memories(&self, agent: &str, chat: i64) -> impl Future<Output = Answer<Vec<Memory>>> + Send;

  /// A memory filed by the memory tool in `turn`, and the tool's reading of what it did, a line each.
  fn add_memory(&self, turn: &Turn, asked: &MemoryAdd) -> impl Future<Output = Answer<Vec<String>>> + Send;
}
