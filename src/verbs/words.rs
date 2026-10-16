//! A word the owner said to one worker rather than to the agent (docs/api.md, "A worker's word").

use std::future::Future;

use super::{Done, Unbuilt};

/// The word, with the files that came with it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerWord {
  /// The worker, the built-in one where none is named.
  pub worker: Option<String>,
  pub text: String,
  pub files: Vec<WordFile>,
}

/// One file that came with a word: its name as it was sent, and its bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordFile {
  pub name: String,
  pub bytes: Vec<u8>,
}

/// Words said to workers.
pub trait WorkerWords {
  /// The files written under the worker's workbench, and the words offered to its session.
  fn say_to_worker(&self, word: WorkerWord) -> impl Future<Output = Done<()>> + Send;
}

impl WorkerWords for Unbuilt {
  fn say_to_worker(&self, _: WorkerWord) -> impl Future<Output = Done<()>> + Send {
    Self::refused("say_to_worker")
  }
}
