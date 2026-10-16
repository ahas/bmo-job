//! The service's codes (docs/api.md, "Codes"): every refusal it answers, every answer it gives and every
//! fact it hands a room, as enums of stable snake_case codes with structured fields, and the kinds those
//! fields take. None carries a sentence a person reads in their own language; a refusal carries the one
//! English sentence the API answers beside its code.

use std::fmt::Display;

use axum::http::StatusCode;
use job_base::Refusal;
use serde::{Deserialize, Serialize};

/// Implements [`Code`] for each family as the snake_case name of its variant.
macro_rules! coded {
  ($($family:ty),+ $(,)?) => {
    $(
      impl $crate::Code for $family {
        fn code(&self) -> &'static str {
          self.into()
        }
      }
    )+
  };
}

pub mod agents;
pub mod branch_split;
pub mod chat_health;
pub mod checkout_holds;
pub mod control_answers;
pub mod job_ask;
pub mod job_ended;
pub mod job_questions;
pub mod job_wait;
pub mod providers;
pub mod room_removal;
pub mod run_stop;
pub mod span_words;
pub mod talk;
pub mod todo_blocks;
pub mod todo_reading;
pub mod todo_verbs;
pub mod work_lines;
pub mod work_order_edits;
pub mod work_order_lines;
pub mod work_order_stops;

pub use agents::AgentRefusal;
pub use branch_split::BranchSplitRefusal;
pub use chat_health::FailureFact;
pub use checkout_holds::CheckoutStop;
pub use control_answers::ControlRefusal;
pub use job_ask::{JobAskAnswer, JobAskRefusal};
pub use job_ended::JobFact;
pub use job_questions::QuestionFact;
pub use job_wait::JobWaitRefusal;
pub use providers::ProviderRefusal;
pub use room_removal::RoomHoldings;
pub use run_stop::{RunStopAnswer, RunStopRefusal};
pub use talk::TalkRefusal;
pub use todo_verbs::{TodoAnswer, TodoFact, TodoRefusal};
pub use work_order_edits::WorkOrderRefusal;
pub use work_order_lines::WorkOrderAnswer;
pub use work_order_stops::WorkOrderStop;

/// A refusal, an answer or a fact, named by its code.
pub trait Code {
  fn code(&self) -> &'static str;
}

/// A refusal, which answers its own status beside its code and its English sentence.
pub trait Refused: Code + Display {
  fn status(&self) -> StatusCode;

  fn refusal(&self) -> Refusal {
    Refusal::new(self.status(), self.code(), self.to_string())
  }
}

/// Every refusal the service answers, each area's under its own variant.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum JobError {
  #[error(transparent)]
  Agent(#[from] AgentRefusal),
  #[error(transparent)]
  BranchSplit(#[from] BranchSplitRefusal),
  #[error(transparent)]
  Control(#[from] ControlRefusal),
  #[error(transparent)]
  JobAsk(#[from] JobAskRefusal),
  #[error(transparent)]
  JobWait(#[from] JobWaitRefusal),
  #[error(transparent)]
  Provider(#[from] ProviderRefusal),
  #[error(transparent)]
  RunStop(#[from] RunStopRefusal),
  #[error(transparent)]
  Talk(#[from] TalkRefusal),
  #[error(transparent)]
  Todo(#[from] TodoRefusal),
  #[error(transparent)]
  WorkOrder(#[from] WorkOrderRefusal),
}

impl JobError {
  fn inner(&self) -> &dyn Refused {
    match self {
      Self::Agent(refusal) => refusal,
      Self::BranchSplit(refusal) => refusal,
      Self::Control(refusal) => refusal,
      Self::JobAsk(refusal) => refusal,
      Self::JobWait(refusal) => refusal,
      Self::Provider(refusal) => refusal,
      Self::RunStop(refusal) => refusal,
      Self::Talk(refusal) => refusal,
      Self::Todo(refusal) => refusal,
      Self::WorkOrder(refusal) => refusal,
    }
  }
}

impl Code for JobError {
  fn code(&self) -> &'static str {
    self.inner().code()
  }
}

impl Refused for JobError {
  fn status(&self) -> StatusCode {
    self.inner().status()
  }
}

impl From<JobError> for Refusal {
  fn from(error: JobError) -> Self {
    error.refusal()
  }
}

/// Every fact the service hands bmo-receiver to tell a room, read back by its code alone.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Fact {
  Checkout(CheckoutStop),
  Failure(FailureFact),
  Job(JobFact),
  Question(QuestionFact),
  Todo(TodoFact),
  WorkOrder(WorkOrderStop),
}

impl Fact {
  fn inner(&self) -> &dyn Code {
    match self {
      Self::Checkout(fact) => fact,
      Self::Failure(fact) => fact,
      Self::Job(fact) => fact,
      Self::Question(fact) => fact,
      Self::Todo(fact) => fact,
      Self::WorkOrder(fact) => fact,
    }
  }
}

impl Code for Fact {
  fn code(&self) -> &'static str {
    self.inner().code()
  }
}

/// A list laid out for a refusal's sentence, its members joined by commas.
pub(crate) fn listed<T: Display>(members: &[T]) -> String {
  members.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
}
