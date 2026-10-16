//! Talk with the agent: the refusal of a second word, and why a turn went unanswered.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum TalkRefusal {
  #[error("the first word is still being answered")]
  TalkBusy,
}

coded!(TalkRefusal);

impl Refused for TalkRefusal {
  fn status(&self) -> StatusCode {
    StatusCode::CONFLICT
  }
}

/// Why a turn went unanswered.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Unanswered {
  Resting,
  Limit,
  Outage,
  Silent,
  Other,
}
