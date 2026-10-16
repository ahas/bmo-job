//! Checks and the suite: the change check, the test count, a suite run, the reading of its log and a
//! close in its repository's close line.

use std::path::PathBuf;

use serde::Deserialize;

/// How a check, a count or a suite ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
  Passed,
  Failed,
  /// The turn did not come; the same call is made again.
  NotRun,
}

/// The change check over what changed in a checkout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Check {
  pub checkout: PathBuf,
  /// The commit the change is read from.
  pub since: Option<String>,
  /// The file the check's output is written to.
  pub log: PathBuf,
}

/// The count of the tests a range removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Count {
  pub checkout: PathBuf,
  /// The commit the range starts from.
  pub since: Option<String>,
  /// The file the count's output is written to.
  pub log: PathBuf,
}

/// A suite run in a checkout.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Suite {
  pub checkout: PathBuf,
  /// The commit the change the suite runs for is read from; every unit runs where it is `None`.
  pub since: Option<String>,
  /// Runs once more what failed or never started.
  pub again: bool,
  /// The run's guard, in whole seconds.
  pub guard: Option<u64>,
  /// Each file's guard, in whole seconds.
  pub file_guard: Option<u64>,
  /// The wait for the turn, in whole seconds.
  pub turn_wait: Option<u64>,
  /// Paths narrowing the files, units' names, and words for each file's runner.
  pub words: Vec<String>,
  /// The file the suite's output is written to.
  pub log: PathBuf,
}

/// What a suite's log reads as.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct SuiteLog {
  pub passed: u64,
  pub failed: u64,
  pub skipped: u64,
  pub verdict: String,
  pub failed_units: Vec<String>,
  pub unstarted_units: Vec<String>,
  pub failures: Vec<Failure>,
}

/// One failing test and the unit it stands in.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Failure {
  pub test: String,
  pub unit: String,
}

/// A command run in the close line of a checkout's repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Close {
  pub checkout: PathBuf,
  /// The command's words, the program first.
  pub command: Vec<String>,
}

/// How a close's turn ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseTurn {
  /// The command ran, ending with this exit code.
  Ran(i32),
  /// The close's job was gone before its turn came.
  NotRun,
}
