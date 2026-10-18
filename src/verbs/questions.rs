//! A job's question answered or talked over by the owner, and its clock stopped and started again
//! (docs/api.md, "Questions").

use std::future::Future;

use job_codes::QuestionAnswer;

use super::{Done, Unbuilt};

/// What the owner settles a question with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Settling {
  /// One of its options, by its number from 0.
  Chosen(u32),
  /// Words of his own.
  Words(String),
}

/// The owner's side of a job's questions.
pub trait Questions {
  /// The question settled, where nothing settled it first.
  fn answer_question(
    &self,
    id: i64,
    settling: Settling,
  ) -> impl Future<Output = Done<QuestionAnswer>> + Send;

  /// The question's clock stopped, or started again.
  fn pause_question(&self, id: i64, paused: bool) -> impl Future<Output = Done<QuestionAnswer>> + Send;

  /// A line of the owner's talk written on an open question that takes talk, its clock stopped for it.
  fn talk_question(&self, id: i64, say: String) -> impl Future<Output = Done<QuestionAnswer>> + Send;
}

impl Questions for Unbuilt {
  fn answer_question(&self, _: i64, _: Settling) -> impl Future<Output = Done<QuestionAnswer>> + Send {
    Self::refused("answer_question")
  }

  fn pause_question(&self, _: i64, _: bool) -> impl Future<Output = Done<QuestionAnswer>> + Send {
    Self::refused("pause_question")
  }

  fn talk_question(&self, _: i64, _: String) -> impl Future<Output = Done<QuestionAnswer>> + Send {
    Self::refused("talk_question")
  }
}
