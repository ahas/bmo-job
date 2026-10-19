//! What a worker is told the words the owner said to it in (docs/api.md, "Hooks bmo-agency asks"):
//! bmo-agency offers the words and asks the service at its `worker-words` point for the text they go in.

use job_policy::Policy;
use job_policy::worker_words::Paragraphs;
use queue_read::blocks::render_block;
use queue_read::dates::iso;
use serde::Deserialize;

/// How the words reach the worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Way {
  /// In a turn of their own, with no job of the worker's behind it.
  Task,
  /// Into the turn a job of the worker's is working.
  Aside,
  /// Into the turn answering earlier words.
  More,
}

impl Way {
  /// The paragraphs the blocks stand between, opening and closing, for this way.
  fn paragraphs(self, said: &Paragraphs) -> (&str, &str) {
    match self {
      Self::Task => (&said.task_open, &said.task_close),
      Self::Aside => (&said.aside_open, &said.aside_close),
      Self::More => (&said.more_open, &said.more_close),
    }
  }
}

/// One word as bmo-agency hands it over: what was said and when, in epoch milliseconds.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaidWord {
  pub text: String,
  pub said_at: i64,
}

/// What bmo-agency asks the words worded for.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Told {
  pub worker: String,
  pub way: Way,
  pub words: Vec<SaidWord>,
}

/// The whole text the worker is told: the way's opening paragraph, each word in the block an item's own
/// words stand in, labelled as the owner's and dated, and the way's closing paragraph.
pub fn told(policy: &Policy, told: &Told) -> String {
  let words = &policy.worker_words;
  let label = policy.fill(&words.block_label, &[]);
  let (open, close) = told.way.paragraphs(&words.paragraphs);
  let blocks = told
    .words
    .iter()
    .map(|word| render_block(&label, &iso(word.said_at), &word.text))
    .collect::<Vec<_>>()
    .join("\n\n");

  [policy.fill(open, &[]), blocks, policy.fill(close, &[])].join("\n\n")
}
