//! What a worker is told the words said to it in: the label each word's block carries and the paragraphs
//! the blocks stand between.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerWords {
  /// With `{owner}`.
  pub block_label: String,
  pub paragraphs: Paragraphs,
}

/// The paragraphs before and after the words' blocks, for each way the words reach the worker, with
/// `{owner}`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Paragraphs {
  pub aside_open: String,
  pub aside_close: String,
  pub task_open: String,
  pub task_close: String,
  pub more_open: String,
  pub more_close: String,
}

impl crate::Part for WorkerWords {
  const NAME: &'static str = "worker-words";
  const TEXT: &'static str = crate::policy_file!("worker-words");
}
