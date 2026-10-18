//! A job's question to the owner: refusals of the question and of a talk on it, and the answer a talk
//! gives the job.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{IntoStaticStr, VariantNames};

use crate::{Refused, listed};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum JobAskRefusal {
  #[error("a question names no room; it goes to the owner's private room alone")]
  QuestionRoom,
  #[error("the question names no item")]
  QuestionItem,
  #[error("the question does not say what is asked")]
  QuestionText,
  #[error("the question has fewer than two options")]
  QuestionOptions,
  #[error("an option is not one line with something on it")]
  QuestionOptionEmpty,
  #[error("the recommendation is not one of the {count} options by number")]
  QuestionRecommend { count: u32 },
  #[error("the question does not say why it is asked rather than decided")]
  QuestionWhy,
  #[error("the wait is not a number of milliseconds")]
  QuestionWait,
  #[error("the talk names no question by number")]
  QuestionTalkNumber,
  #[error("question {id} takes no talk")]
  QuestionTakesNoTalk { id: i64 },
  #[error("the talk has no words of its own")]
  QuestionTalkSay,
  #[error("question {id} is settled already and takes no more talk")]
  QuestionTalkSettled { id: i64 },
  #[error("the words use terms the owner has set aside: {}", listed(.terms))]
  QuestionBannedTerms { terms: Vec<String> },
  #[error("the words hand the owner pages hosted outside the deployment's own archive: {}", listed(.addresses))]
  QuestionOutsideHosting { addresses: Vec<String> },
  #[error("there is no question {id}")]
  QuestionAbsent { id: i64 },
  #[error("answer with one of the question's options by its number, or with words of your own, not both")]
  QuestionAnswerWrong,
}

coded!(JobAskRefusal);

impl Refused for JobAskRefusal {
  fn status(&self) -> StatusCode {
    match self {
      Self::QuestionTakesNoTalk { .. } | Self::QuestionTalkSettled { .. } => StatusCode::CONFLICT,
      Self::QuestionAbsent { .. } => StatusCode::NOT_FOUND,
      _ => StatusCode::BAD_REQUEST,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum JobAskAnswer {
  OwnerTalked { id: i64, heard: Vec<String> },
}

coded!(JobAskAnswer);
