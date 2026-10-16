//! Launching a job: the refusal of a launch asked wrong, what a launch came to, and the notes it carries.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum TaskRefusal {
  #[error("a job takes the room it reports to and what to do")]
  TaskNeedsRoomAndWork,
}

coded!(TaskRefusal);

impl Refused for TaskRefusal {
  fn status(&self) -> StatusCode {
    StatusCode::BAD_REQUEST
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TaskAnswer {
  JobLaunched {
    job: i64,
    unit: String,
    worker: String,
    cwd: String,
    chat: i64,
    notes: Vec<LaunchNote>,
  },
  OrderStarted {
    item: i64,
    phase: String,
    notes: Vec<LaunchNote>,
  },
  OrderAlreadyActive {
    item: i64,
    notes: Vec<LaunchNote>,
  },
}

coded!(TaskAnswer);

/// What a launch says beside what it came to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum LaunchNote {
  CoworkOffCheckout { worker: String, cwd: String },
}
