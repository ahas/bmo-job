//! A job's end, as its room is told it.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::work_lines::JobLabel;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum JobFact {
  JobEnded {
    reason: JobEnding,
    job_id: i64,
    checkout: String,
  },
  JobGone {
    job_id: i64,
    label: Option<JobLabel>,
    purpose: Option<String>,
    removed_room: Option<i64>,
  },
  WorkReported {
    item: Option<i64>,
    done: bool,
    summary: Option<String>,
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
