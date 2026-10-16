//! A job launched on a worker (docs/api.md, "Jobs").

use std::future::Future;

use job_codes::TaskAnswer;

use super::{Done, Unbuilt};

/// A launch as it was asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskLaunch {
  /// The room the job reports to.
  pub chat: i64,
  pub prompt: String,
  pub model: Option<String>,
  pub item: Option<i64>,
  pub purpose: Option<String>,
  pub worker: Option<String>,
  pub cowork: bool,
  pub started_by: Option<String>,
}

/// Jobs launched.
pub trait Tasks {
  fn launch_task(&self, launch: TaskLaunch) -> impl Future<Output = Done<TaskAnswer>> + Send;
}

impl Tasks for Unbuilt {
  fn launch_task(&self, _: TaskLaunch) -> impl Future<Output = Done<TaskAnswer>> + Send {
    Self::refused("launch_task")
  }
}
