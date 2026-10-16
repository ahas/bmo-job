//! The service's port to bmo-receiver (docs/api.md, "bmo-receiver"): every call it makes there, declared
//! by the trait `Receiver` and made over HTTP by `ReceiverClient`. The shapes the calls carry stand in
//! one module to an area.

pub mod client;
pub mod holdings;
pub mod instructions;
pub mod notices;
pub mod rooms;
pub mod talks;

use std::future::Future;

pub use client::ReceiverClient;
pub use holdings::{Effect, Holder, Holding};
pub use instructions::{Instruction, InstructionState, Said};
pub use notices::Notice;
pub use rooms::{Answerer, Answerers, Message, Owner, Room};
pub use talks::{DocumentTopic, ItemTopic, Talk, Topic, Turn};

/// Why a call on bmo-receiver failed. Each names the call it was.
#[derive(Debug, thiserror::Error)]
pub enum ReceiverError {
  /// The address given is no `http:` address a path can be added to.
  #[error("{address} is not an http address bmo-receiver can be reached at")]
  NotAnAddress { address: String },
  /// The call did not reach bmo-receiver, or no answer came back.
  #[error("{call}: bmo-receiver did not answer: {error}")]
  Unreachable { call: &'static str, error: String },
  /// bmo-receiver refused the call, with its status, code and sentence as it gave them. A talk that
  /// came back with nothing is refused so, with the code `resting`, `limit`, `outage` or any other.
  #[error("{call}: bmo-receiver refused with {status} {code}: {error}")]
  Refused {
    call: &'static str,
    status: u16,
    code: String,
    error: String,
  },
  /// bmo-receiver answered with a body that does not read as the call's answer or as a refusal.
  #[error("{call}: bmo-receiver answered {status} with a body that does not read: {error}")]
  Malformed {
    call: &'static str,
    status: u16,
    error: String,
  },
}

/// What a call on bmo-receiver comes back with.
pub type Answer<T> = Result<T, ReceiverError>;

/// Every call the service makes on bmo-receiver. A read of one thing by its id answers `None` where
/// bmo-receiver has no such thing.
pub trait Receiver: Send + Sync {
  /// Queues an instruction, answering its id.
  fn queue_instruction(&self, instruction: &Instruction) -> impl Future<Output = Answer<i64>> + Send;

  /// Reads how far an instruction has gone.
  fn instruction_state(&self, id: i64) -> impl Future<Output = Answer<Option<InstructionState>>> + Send;

  /// Withdraws an instruction not yet taken, answering false where it was handed already or is gone.
  fn withdraw_instruction(&self, id: i64) -> impl Future<Output = Answer<bool>> + Send;

  /// Voices a notice in a room, answering the id of the instruction it was queued as.
  fn voice_notice(&self, chat: i64, notice: &Notice) -> impl Future<Output = Answer<i64>> + Send;

  /// Reads the deployment's owner.
  fn owner(&self) -> impl Future<Output = Answer<Owner>> + Send;

  /// Reads what a room is to the deployment.
  fn room(&self, chat: i64) -> impl Future<Output = Answer<Room>> + Send;

  /// Reads the agents who answer in a room.
  fn answerers(&self, chat: i64) -> impl Future<Output = Answer<Answerers>> + Send;

  /// Reads a message said in a room.
  fn message(&self, chat: i64, message: i64) -> impl Future<Output = Answer<Option<Message>>> + Send;

  /// Takes one turn of a talk, answering what the agent said, `None` where it said nothing.
  fn talk(&self, talk: &Talk) -> impl Future<Output = Answer<Option<String>>> + Send;

  /// Declares the service, as `unit`, a holder of rows naming a room.
  fn declare_holder(&self, unit: &str, holder: &Holder) -> impl Future<Output = Answer<()>> + Send;
}
