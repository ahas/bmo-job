//! A job's end, as its room is told it.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum JobFact {
  JobEnded {
    reason: JobEnding,
    job_id: i64,
    checkout: String,
  },
}

coded!(JobFact);

/// How a job ended.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum JobEnding {
  Done,
  WorkStep,
  Failed,
  LiveTasks,
  Unclosed,
  Limit,
  Error,
  Died,
  Stopped,
  Unanswered,
  Blocked,
}
