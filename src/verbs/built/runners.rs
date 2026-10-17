//! A runner started on the owner's own words (docs/api.md, "Runners"), by the queue's launcher.

use super::Built;
use crate::queue::runners;
use crate::verbs::{Done, RunnerLaunch, Runners};

impl Runners for Built {
  async fn launch_runner(&self, launch: RunnerLaunch) -> Done<()> {
    runners::launch(
      &self.policy.research,
      &self.ground.launcher,
      &self.ground.launcher_directory,
      self.ground.owner_chat,
      &launch.agent,
      launch.depth.as_deref(),
      &launch.text,
    )
    .await
  }
}
