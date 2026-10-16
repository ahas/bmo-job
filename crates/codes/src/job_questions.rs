//! A job's question to the owner and a pane's confirmation, as the room is told them.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum QuestionFact {
  QuestionAsked {
    item: i64,
    question_id: i64,
    question: String,
    options: Vec<String>,
    recommended: u32,
    why: String,
    wait_seconds: u64,
    talk: bool,
    removed_room: Option<String>,
  },
  QuestionTalkHeld {
    item: i64,
  },
  JobReplied {
    item: i64,
    said: String,
  },
  QuestionRevised {
    item: i64,
    options: Vec<String>,
    recommended: u32,
    wait_seconds: u64,
  },
  QuestionChosen {
    item: i64,
    option: u32,
    named: String,
  },
  QuestionTyped {
    item: i64,
    answer: String,
  },
  QuestionLapsed {
    item: i64,
    lapsed: Lapse,
    option: u32,
    named: String,
    wait_seconds: u64,
  },
  PaneQuestion {
    item: i64,
    mark: String,
    window: PaneWindow,
    options: Vec<PaneOption>,
  },
  PaneQuestionWithdrawn {
    item: i64,
  },
  PaneCommandAllowed {
    item: i64,
  },
}

coded!(QuestionFact);

/// What a question's wait ran out in.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Lapse {
  Waiting,
  Talking,
  Stopped,
}

/// The window a pane stopped at.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PaneWindow {
  Permission,
  Unknown,
}

/// An answer the owner may give a pane's confirmation.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PaneOption {
  Allow,
  Refuse,
  Close,
  Leave,
}
