//! Deploys: the commits waiting to go live, and what a repository has live.

use std::path::PathBuf;

use serde::Deserialize;

/// A commit landed in a checkout's repository, to be recorded as waiting to go live.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingDeploy {
  pub checkout: PathBuf,
  pub commit: String,
  pub item: Option<i64>,
  pub worker: Option<String>,
  pub job: Option<i64>,
  /// Why it goes live, in English.
  pub reason: String,
}

/// A commit waiting to go live, as bmo-testbed records it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Pending {
  /// The declared name of the repository the commit is of.
  pub repository: String,
  pub commit: String,
  pub item: Option<i64>,
  pub worker: Option<String>,
  pub job: Option<i64>,
  pub reason: String,
  /// True where bmo-testbed reads the commit as one to put live ahead of the rest.
  pub urgent: bool,
  pub created_at: i64,
}

/// Which pending deploys a list holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingOf {
  /// Every repository's on the machine.
  All,
  Job(i64),
  Item(i64),
}

/// What a repository has live.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Deployed {
  /// The commit its deploy tree runs.
  pub commit: String,
  /// The release directory that holds it.
  pub release: PathBuf,
}
