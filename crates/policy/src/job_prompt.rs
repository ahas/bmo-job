//! What a job's prompt is made of beside the job's own text and its trailer: the lines of the block a
//! worker's job prompt opens with, the workbench's own names they cite, what a step of a bot-managed work
//! order is told, and the words of the deployment's language the lines name.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobPrompt {
  pub preamble: Preamble,
  pub workbench: Workbench,
  pub work_step: WorkStep,
  pub impact: Impact,
  pub words: Words,
}

/// The preamble's lines, each with its `{name}` marks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preamble {
  pub heading: String,
  pub workbench: String,
  pub item: String,
  pub support: String,
  pub support_commit: String,
  pub own_worktree: String,
  pub own_checkout: String,
  pub branch_commit: String,
  pub worktree_after: String,
  pub checkout_after: String,
  pub not_checkout: String,
  /// With `{dev_tab}`, the tab's name the words give.
  pub dev_support: String,
  pub dev_support_named: String,
  /// With `{dev_tab}`, the tab's name the words give.
  pub dev_own: String,
  pub dev_own_log: String,
  pub dev_use: String,
  pub dev_up: String,
}

/// The names under a worker's workbench the preamble cites.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Workbench {
  pub scratch: String,
  pub database: String,
  pub dev_log: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkStep {
  pub told: String,
}

/// The block an item's impact grounds stand in, each line with its `{name}` marks.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Impact {
  pub heading: String,
  pub contract: String,
  pub inside: String,
  pub new_home: String,
  pub single: String,
  pub cowork: String,
}

/// The words of the deployment's language the preamble names, English where the deployment gives none.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Words {
  /// The name of the tab a worker's dev server stands in.
  pub dev_tab: String,
}

impl crate::Part for JobPrompt {
  const NAME: &'static str = "job-prompt";
  const TEXT: &'static str = crate::policy_file!("job-prompt");
}
