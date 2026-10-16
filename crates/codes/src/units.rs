//! A call on another unit that failed: refused under the unit's name, the sentence naming the call and how
//! it failed.

use axum::http::StatusCode;
use strum::{IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum UnitRefusal {
  #[error("{failure}")]
  Agency { failure: String },
  #[error("{failure}")]
  Brain { failure: String },
  #[error("{failure}")]
  Gate { failure: String },
  #[error("{failure}")]
  Graph { failure: String },
  #[error("{failure}")]
  Receiver { failure: String },
  #[error("{failure}")]
  Testbed { failure: String },
  #[error("{failure}")]
  Web { failure: String },
}

coded!(UnitRefusal);

impl Refused for UnitRefusal {
  fn status(&self) -> StatusCode {
    StatusCode::BAD_GATEWAY
  }
}
