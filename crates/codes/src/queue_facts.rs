//! The queue as the agent is told it in the owner's chat: workers standing with work waiting, a job's
//! finish and what it means for its item, a chain of repairs held at its ceiling, and an open job that
//! circles or stands still.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::job_ended::JobEnding;
use crate::job_wait::WaitTarget;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum QueueFact {
  QueueWaiting {
    moment: String,
    workers: Vec<WorkerWaiting>,
  },
  QueueStalled {
    worker: String,
    label: String,
    cwd: String,
    waiting: u32,
    minutes: u32,
  },
  JobFinished(Box<JobFinish>),
  RepairCeiling {
    item: i64,
    title: String,
    rounds: u32,
    limit: u32,
    held: Vec<i64>,
    last_job: Option<i64>,
    last_item: Option<i64>,
    outcome: String,
  },
  JobCircling {
    job_id: i64,
    item: Option<i64>,
    worker: String,
    unit: String,
    starts: u64,
    most: u64,
  },
  JobQuiet {
    job_id: i64,
    item: Option<i64>,
    worker: String,
    unit: String,
    minutes: u32,
    last_activity_at: i64,
    last_line: Option<LastLine>,
    wait: Option<WaitMark>,
  },
}

coded!(QueueFact);

/// One worker standing with a free checkout and work of its own waiting.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerWaiting {
  pub worker: String,
  pub label: String,
  pub cwd: String,
  pub waiting: u32,
  pub free_checkouts: u32,
}

/// How many of something, against the most there may be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tally {
  pub count: u32,
  pub limit: u32,
}

/// The attempts at an item's work, and whether this run counted among them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempts {
  pub count: u32,
  pub limit: u32,
  pub counted: bool,
}

/// The chain of repairs an item stands in: the item it began at, its rounds and the most it may have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairChain {
  pub of: i64,
  pub rounds: u32,
  pub limit: u32,
}

/// A commit waiting to go live.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingCommit {
  pub commit: String,
  pub urgent: bool,
}

/// A job's finish, as the agent is told it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobFinish {
  pub worker: Option<String>,
  pub label: String,
  pub job_id: Option<i64>,
  pub item: Option<i64>,
  /// The ending's word as its record keeps it.
  pub outcome: String,
  pub ending: Option<JobEnding>,
  pub succeeded: bool,
  pub attempts: Option<Attempts>,
  pub launch_failures: Option<Tally>,
  /// Why the ending set the item aside, where it did.
  pub held: Option<HoldCause>,
  pub repair: Option<RepairChain>,
  pub pending_deploy: Vec<PendingCommit>,
  pub queue_paused: bool,
  pub deploy_hold: bool,
  /// Why launches are held for want of room on the machine.
  pub machine_short: Vec<String>,
  pub report: Option<String>,
  /// The room that heard a line of the report.
  pub room_line_to: Option<i64>,
  pub removed_room: Option<i64>,
  /// Whether the worker's report went to nobody, and is the agent's to pass on.
  pub report_untold: bool,
}

/// Why an ending set its item aside.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum HoldCause {
  Attempts { count: u32, last: String },
  LaunchFailures { count: u32, last: String },
  RepairCeiling { rounds: u32, item: i64, last: String },
}

/// The last line a job wrote to the agent log.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LastLine {
  pub kind: String,
  pub name: Option<String>,
}

/// A job's own mark on a long wait, as it stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WaitMark {
  pub what: String,
  pub target: WaitTarget,
  pub until: i64,
  pub ended_at: Option<i64>,
  pub standing: WaitStanding,
}

/// Where a job's wait mark stands.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum WaitStanding {
  Waiting,
  Overdue,
  Ended,
}
