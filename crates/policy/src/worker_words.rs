//! A word said to a worker: its clocks, the paragraphs it reaches the worker in, and the lines written
//! under it.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkerWords {
  pub answer_minutes: u64,
  pub tick_seconds: u64,
  pub person_wait_seconds: u64,
  pub head_chars: usize,
  /// With `{owner}`, `{worker}` and `{words}`.
  pub question_what: String,
  pub question_clock: String,
  /// With `{owner}`.
  pub block_label: String,
  pub paragraphs: Paragraphs,
  pub held: Held,
  pub ended: Ended,
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

/// The line written under a word for what held it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Held {
  pub person: String,
  pub untrusted: String,
  pub unsubmitted: String,
  pub stopped: String,
  pub disabled: String,
  pub lost: String,
  pub resting: String,
  pub job_waiting: String,
  pub in_job: String,
}

/// The line written under the last word a turn took where it wrote no answer.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ended {
  pub silent: String,
  pub cut_by_agent: String,
  pub cut_by_agent_why: String,
  pub cut_by_bot: String,
  pub pinned_failed: String,
}

impl crate::Part for WorkerWords {
  const NAME: &'static str = "worker-words";
  const TEXT: &'static str = crate::policy_file!("worker-words");
}
