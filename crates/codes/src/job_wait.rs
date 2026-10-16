//! A job's wait mark: its refusals and what it waits on.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum JobWaitRefusal {
  #[error("the wait mark does not say what the job waits on")]
  WaitNeedsWhat,
  #[error("the wait mark names no unit or process and runs no command")]
  WaitNeedsTarget,
}

coded!(JobWaitRefusal);

impl Refused for JobWaitRefusal {
  fn status(&self) -> StatusCode {
    StatusCode::BAD_REQUEST
  }
}

/// What a job waits on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum WaitTarget {
  Unit { name: String },
  Pid { pid: u32 },
}
