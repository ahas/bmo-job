//! An item as it is read: the marks on its line, its hold, its linked rows, its questions and its
//! model.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

/// A mark on an item's line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum LineMark {
  MergedInto { id: i64 },
  MergingInto { id: i64 },
  Done,
  PendingApproval { requester: String },
  Cowork,
  OwnerNote,
  Report,
  Unfinished,
  UnfinishedResolved,
}

/// What holds an item.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HeldWord {
  Stop,
  Stopping,
  Pause,
  Pausing,
  Failure,
  Plain,
  Pending,
}

/// Where a linked row stands.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RowState {
  Done,
  Open,
  Merged,
  Held,
  Opened,
}

/// Where a job's question stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum QuestionState {
  Waiting,
  Typed { answer: String },
  Chosen,
  Defaulted,
  AnsweredInPane,
}

/// What is marked about the model an item's runs went out on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ModelMark {
  Fallback,
  Changed,
  Requested { model: String },
}
