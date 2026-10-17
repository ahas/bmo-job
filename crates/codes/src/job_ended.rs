//! A job's end, as its room is told it.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::job_roads::WorkOutput;
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
  JobStopped {
    job_id: i64,
    item: Option<i64>,
    worker: String,
    outcome: JobEnding,
    requested: String,
    actual: Option<Selection>,
  },
  WorkOutputRefused {
    job_id: i64,
    worker: String,
    output: WorkOutput,
    reason: String,
    correction: u32,
  },
}

/// A provider and the model on it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
  pub provider: String,
  pub model: String,
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
