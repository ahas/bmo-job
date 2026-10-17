//! The job queue's declared limits.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueueLimits {
  /// How long the owner's stop of a question's clock holds it, in seconds.
  pub question_stop_seconds: i64,
  /// How many files one word to a worker carries.
  pub word_files: usize,
  /// The largest one file a word to a worker carries may be, in bytes.
  pub word_file_bytes: u64,
  /// The model a stopped order's last retry runs on where the archive setting names none.
  pub work_retry_model: String,
  /// The tier a builder task in a work order's stage runs at where its plan named none.
  pub stage_builder_tier: String,
}

impl crate::Part for QueueLimits {
  const NAME: &'static str = "queue";
  const TEXT: &'static str = crate::policy_file!("queue");
}
