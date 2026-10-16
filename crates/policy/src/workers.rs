//! What a job worker may be.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerRules {
  /// The pattern a worker's name matches.
  pub name_pattern: String,
  /// The longest a worker's label may be, in characters.
  pub label_most: usize,
  /// The longest a worker's preamble may be, in characters.
  pub preamble_most: usize,
  /// The longest a worker's dev server command may be, in characters.
  pub dev_command_most: usize,
  /// How many support workers one base may have.
  pub support_most: i64,
  /// The first support worker's number.
  pub first_support_slot: i64,
}

impl crate::Part for WorkerRules {
  const NAME: &'static str = "workers";
  const TEXT: &'static str = crate::policy_file!("workers");
}
