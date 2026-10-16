//! The calls the rows' verbs make on other units (docs/rows.md, "Where a verb stands"): an instruction
//! queued and a message read through bmo-receiver, a room's memories read and one filed through bmo-brain,
//! and a turn record kept through bmo-agency. A call that fails is refused with 502 under the unit's code.

use futures_util::future::BoxFuture;
use job_agency::{Agency, TurnRecord};
use job_base::Refusal;
use job_brain::{Brain, Memory, MemoryAdd, Turn};
use job_codes::{JobError, UnitRefusal};
use job_receiver::{Instruction, Message, Receiver};

/// Every call the rows' verbs make on other units. A test stands in for each.
pub trait Units: Sync {
  /// An instruction queued for an agent's session, answering its id.
  fn queue_instruction<'a>(&'a self, instruction: &'a Instruction) -> BoxFuture<'a, Result<i64, Refusal>>;
  /// A message said in a room, none where bmo-receiver keeps none of it.
  fn message(&self, chat: i64, message: i64) -> BoxFuture<'_, Result<Option<Message>, Refusal>>;
  /// The deployment's agent's memories in a room.
  fn memories(&self, chat: i64) -> BoxFuture<'_, Result<Vec<Memory>, Refusal>>;
  /// A memory filed for the deployment's agent.
  fn add_memory<'a>(&'a self, memory: &'a MemoryAdd) -> BoxFuture<'a, Result<(), Refusal>>;
  /// A turn record kept under its run's name.
  fn keep_turn_record<'a>(&'a self, record: &'a TurnRecord) -> BoxFuture<'a, Result<(), Refusal>>;
}

fn failed(unit: fn(String) -> UnitRefusal, failure: impl std::fmt::Display) -> Refusal {
  JobError::from(unit(failure.to_string())).into()
}

/// The calls made over the ports to bmo-agency, bmo-brain and bmo-receiver, the brain's for the deployment's
/// agent by its key.
pub struct Ports<'a, A, B, R> {
  pub agency: &'a A,
  pub brain: &'a B,
  pub receiver: &'a R,
  pub agent_key: &'a str,
}

impl<A: Agency, B: Brain, R: Receiver> Units for Ports<'_, A, B, R> {
  fn queue_instruction<'a>(&'a self, instruction: &'a Instruction) -> BoxFuture<'a, Result<i64, Refusal>> {
    Box::pin(async move {
      self
        .receiver
        .queue_instruction(instruction)
        .await
        .map_err(|why| failed(|failure| UnitRefusal::Receiver { failure }, why))
    })
  }

  fn message(&self, chat: i64, message: i64) -> BoxFuture<'_, Result<Option<Message>, Refusal>> {
    Box::pin(async move {
      self
        .receiver
        .message(chat, message)
        .await
        .map_err(|why| failed(|failure| UnitRefusal::Receiver { failure }, why))
    })
  }

  fn memories(&self, chat: i64) -> BoxFuture<'_, Result<Vec<Memory>, Refusal>> {
    Box::pin(async move {
      self
        .brain
        .memories(self.agent_key, chat)
        .await
        .map_err(|why| failed(|failure| UnitRefusal::Brain { failure }, why))
    })
  }

  fn add_memory<'a>(&'a self, memory: &'a MemoryAdd) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move {
      let turn = Turn {
        agent: self.agent_key.to_owned(),
        chat_id: None,
        message_id: None,
      };

      self
        .brain
        .add_memory(&turn, memory)
        .await
        .map(drop)
        .map_err(|why| failed(|failure| UnitRefusal::Brain { failure }, why))
    })
  }

  fn keep_turn_record<'a>(&'a self, record: &'a TurnRecord) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move {
      self
        .agency
        .keep_turn_record(record)
        .await
        .map_err(|why| failed(|failure| UnitRefusal::Agency { failure }, why))
    })
  }
}
