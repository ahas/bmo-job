//! A job's end and the queue's standing told to the agent (docs/api.md, "A job's end").

use std::future::Future;

use job_codes::QueueTold;

use super::{Done, Unbuilt};

/// A job that ended, as its runner says so.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobEnded {
  /// How it ended, as the runner names it.
  pub outcome: String,
  pub job: Option<i64>,
}

/// A moment the queue's standing is told at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Moment {
  pub moment: String,
  /// Tell it again where it stood the same.
  pub repeat: bool,
  /// Let the fact ride the room's next turn for this long rather than open one.
  pub rides_ms: Option<i64>,
}

/// What a job's end and the queue's standing tell the agent.
pub trait Telling {
  /// The report the job left waiting delivered, then the fact of its end told.
  fn job_ended(&self, ended: JobEnded) -> impl Future<Output = Done<QueueTold>> + Send;

  /// The report a job wrote delivered.
  fn deliver_report(&self, job: i64) -> impl Future<Output = Done<()>> + Send;

  /// The queue's standing told at a moment.
  fn tell_queue(&self, moment: Moment) -> impl Future<Output = Done<QueueTold>> + Send;
}

impl Telling for Unbuilt {
  fn job_ended(&self, _: JobEnded) -> impl Future<Output = Done<QueueTold>> + Send {
    Self::refused("job_ended")
  }

  fn deliver_report(&self, _: i64) -> impl Future<Output = Done<()>> + Send {
    Self::refused("deliver_report")
  }

  fn tell_queue(&self, _: Moment) -> impl Future<Output = Done<QueueTold>> + Send {
    Self::refused("tell_queue")
  }
}
