//! When a stage of a work order that has not reported is a runaway, and the words its reason is said in.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runaway {
  /// How many times one run may be re-invoked before it is circling.
  pub max_starts: i64,
  /// When the clock alone ends a run in a copy that is thrown away.
  pub stop_after_minutes: i64,
  pub labels: Labels,
  pub lines: Lines,
}

/// The label a reason opens with, by what fired.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Labels {
  pub spin: String,
  pub stalled: String,
}

/// The reason's own words, with `{starts}`, `{max}`, `{minutes}` and `{owner}`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Lines {
  pub spin: String,
  pub spin_waited: String,
  pub clock: String,
  pub stopped: String,
  pub told: String,
}

impl crate::Part for Runaway {
  const NAME: &'static str = "runaway";
  const TEXT: &'static str = crate::policy_file!("runaway");
}
