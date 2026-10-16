//! A runner started on the owner's own words (docs/api.md, "Runners").

use std::future::Future;

use super::{Done, Unbuilt};

/// A runner asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerLaunch {
  /// The runner, by its name.
  pub agent: String,
  pub text: String,
  /// How deep it goes, where it takes a depth; its first where none is asked.
  pub depth: Option<String>,
}

/// Runners started.
pub trait Runners {
  fn launch_runner(&self, launch: RunnerLaunch) -> impl Future<Output = Done<()>> + Send;
}

impl Runners for Unbuilt {
  fn launch_runner(&self, _: RunnerLaunch) -> impl Future<Output = Done<()>> + Send {
    Self::refused("launch_runner")
  }
}
