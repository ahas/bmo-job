//! Stopping runs: refusals of a stop, what a stop answers, and how far each target got.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum RunStopRefusal {
  #[error("the target {target} is not a kind and an id")]
  StopTargetWrong { target: String },
  #[error("the reason is not in the deployment's language, which it is shown in as written")]
  ReasonLanguage,
  #[error("the stop names no target")]
  StopTargetMissing,
  #[error("no run of {target} is known")]
  StopTargetAbsent { target: String },
  #[error("the run of {target} has already ended or is not running")]
  StopTargetEnded { target: String },
  #[error("a co-worker is not stopped on its own; stopping the run that launched it ends it too")]
  StopCoworker { parent: Option<String> },
  #[error("an item is named by its number, and {id} is none")]
  StopItemNumber { id: String },
}

coded!(RunStopRefusal);

impl Refused for RunStopRefusal {
  fn status(&self) -> StatusCode {
    match self {
      Self::StopTargetAbsent { .. } | Self::StopTargetEnded { .. } => StatusCode::NOT_FOUND,
      Self::StopCoworker { .. } => StatusCode::CONFLICT,
      _ => StatusCode::BAD_REQUEST,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RunStopAnswer {
  NothingRunning,
  NothingNameable {
    count: u32,
  },
  StopConfirmed {
    target: String,
    runs: u32,
  },
  StopAsked {
    target: String,
    runs: u32,
  },
  AllAsked {
    asked: u32,
    confirmed: u32,
    targets: Vec<TargetStop>,
  },
}

coded!(RunStopAnswer);

/// One target of a stop and how far it got.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetStop {
  pub target: String,
  pub state: StopState,
}

/// How far a target's stop got.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum StopState {
  Ended,
  Stopping,
  Refused { why: String },
}
