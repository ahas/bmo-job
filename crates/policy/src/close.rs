//! What a job's close says, each line with the `{name}` marks its file names, and the names of address
//! its English reading sets aside.

use serde::Deserialize;

use crate::Names;
use crate::english::English;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageWords {
  pub close: String,
  pub skip_reason: String,
  pub count: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseWords {
  pub name: String,
  pub said: String,
  pub refused: String,
  pub node_branch: String,
  pub line_gone: String,
  pub line_came: String,
  pub turn_gone: String,
  pub check_again: String,
  pub node_dropped: String,
  pub landed: String,
  pub one: String,
  pub many: String,
  pub set_up: String,
  pub set_up_failed: String,
  pub failed_files: String,
  pub log: String,
  pub no_verdict: String,
  pub home_silent: String,
  pub unrecorded: String,
  pub flaky_unwritten: String,
  pub recorded: String,
  pub unbuilt: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeWords {
  pub no_commit: String,
  pub not_pushed: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GateWords {
  pub stopped: String,
  pub check: String,
  pub check_not_run: String,
  pub check_failed: String,
  pub skipped: String,
  pub suite: String,
  pub unfinished: String,
  pub suite_not_run: String,
  pub once_more: String,
  pub flaky: String,
  pub failed_files: String,
  pub suite_failed: String,
  pub deployed: String,
  pub waiting: String,
  pub handed: String,
  pub passed: String,
  pub was_skipped: String,
  pub not_landed: String,
  pub done: String,
  pub flaky_named: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StretchWords {
  pub command: String,
  pub front_first: String,
  pub held: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineWords {
  pub waiting: String,
  pub nobody: String,
  pub held_by: String,
  pub joiner: String,
  pub pid: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckWords {
  pub step: String,
  pub ended: String,
  pub not_run: String,
  pub not_had: String,
  pub failed: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CountWords {
  pub failed: String,
  pub unaccounted: String,
  pub test: String,
  pub joiner: String,
  pub unreadable: String,
  pub start: String,
  pub head: String,
}

/// What the close refuses a reason or a commit of the job's own holding Hangul with.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanguageWords {
  pub reason: String,
  pub commits: String,
  pub commit: String,
  pub joiner: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Close {
  pub usage: UsageWords,
  pub close: CloseWords,
  /// How a reason or a commit message is read for English, its set-aside built from the names.
  pub english: English,
  pub language: LanguageWords,
  pub tree: TreeWords,
  pub gate: GateWords,
  pub stretch: StretchWords,
  pub line: LineWords,
  pub check: CheckWords,
  pub count: CountWords,
}

impl crate::Part for Close {
  const NAME: &'static str = "close";
  const TEXT: &'static str = crate::policy_file!("close");

  fn build(&mut self, names: &Names) {
    self.english.set_aside = names.pattern(&self.english.set_aside, &[]);
  }
}
