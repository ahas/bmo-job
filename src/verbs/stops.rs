//! Stopping runs (docs/api.md, "Stopping runs").

use std::fmt;
use std::future::Future;

use job_codes::RunStopAnswer;
use serde::{Deserialize, Serialize};

use super::{Done, Unbuilt};

/// What a stop names: a kind, and the id that kind is named by.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Target {
  pub kind: TargetKind,
  pub id: String,
}

/// What a stop reaches, by what it is named by.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TargetKind {
  /// A run in the agent's host, by its execution record.
  Execution,
  /// A run standing apart in a unit of its own.
  Unit,
  /// An item, by its number: the owner's stop of it.
  Item,
}

impl fmt::Display for Target {
  /// The target as an answer names it: its kind, then its id.
  fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    let kind = match self.kind {
      TargetKind::Execution => "execution",
      TargetKind::Unit => "unit",
      TargetKind::Item => "item",
    };

    write!(out, "{kind}:{}", self.id)
  }
}

/// One stop asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stop {
  pub target: Target,
  /// Why, which an item's stop requires and nothing else's does.
  pub why: Option<String>,
}

/// Runs stopped.
pub trait RunStops {
  /// The one run the target names asked to stop, and confirmed where it is seen gone.
  fn stop_run(&self, stop: Stop) -> impl Future<Output = Done<RunStopAnswer>> + Send;

  /// Everything running asked to stop, one that cannot be stopped keeping none of the rest running.
  fn stop_all(&self) -> impl Future<Output = Done<RunStopAnswer>> + Send;
}

impl RunStops for Unbuilt {
  fn stop_run(&self, _: Stop) -> impl Future<Output = Done<RunStopAnswer>> + Send {
    Self::refused("stop_run")
  }

  fn stop_all(&self) -> impl Future<Output = Done<RunStopAnswer>> + Send {
    Self::refused("stop_all")
  }
}
