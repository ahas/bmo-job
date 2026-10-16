//! The service's port to bmo-testbed (docs/api.md, "bmo-testbed"): every call it makes there, declared
//! by the trait `Testbed` and made by `TestbedClient`, which runs bmo-testbed's binary. The shapes the
//! calls carry stand in one module to an area.

pub mod checks;
pub mod client;
pub mod deploys;
pub mod heavy;
pub mod units;

use std::future::Future;
use std::path::Path;

pub use checks::{Check, Close, CloseTurn, Count, Failure, Outcome, Suite, SuiteLog};
pub use client::TestbedClient;
pub use deploys::{Deployed, Pending, PendingDeploy, PendingOf};
pub use heavy::Classified;
pub use units::{Relink, Relinked, UnitPlan};

/// Why a call on bmo-testbed failed. Each names the call it was.
#[derive(Debug, thiserror::Error)]
pub enum TestbedError {
  /// The path given is not an absolute path a program can be run from.
  #[error("{program} is not an absolute path bmo-testbed's binary can be run from")]
  NotAProgram { program: String },
  /// The binary did not start, or its log file could not be opened.
  #[error("{call}: bmo-testbed did not start: {error}")]
  Unstarted { call: &'static str, error: String },
  /// bmo-testbed refused the call with exit code 2, with the last line it wrote as it stands.
  #[error("{call}: bmo-testbed refused: {said}")]
  Refused { call: &'static str, said: String },
  /// bmo-testbed ended with an exit code the call does not name, or by a signal where `code` is `None`.
  #[error("{call}: bmo-testbed ended with {code:?}: {said}")]
  Broke {
    call: &'static str,
    code: Option<i32>,
    said: String,
  },
  /// bmo-testbed answered with output that does not read as the call's answer.
  #[error("{call}: bmo-testbed answered with output that does not read: {error}")]
  Malformed { call: &'static str, error: String },
}

/// What a call on bmo-testbed comes back with.
pub type Answer<T> = Result<T, TestbedError>;

/// Every call the service makes on bmo-testbed.
pub trait Testbed: Send + Sync {
  /// Checks the change in a checkout, its output written to the check's log.
  fn check(&self, check: &Check) -> impl Future<Output = Answer<Outcome>> + Send;

  /// Counts the tests a range removed, its output written to the count's log.
  fn count(&self, count: &Count) -> impl Future<Output = Answer<Outcome>> + Send;

  /// Runs the suite in a checkout, its output written to the suite's log.
  fn suite(&self, suite: &Suite) -> impl Future<Output = Answer<Outcome>> + Send;

  /// Reads a suite's log.
  fn read_log(&self, log: &Path) -> impl Future<Output = Answer<SuiteLog>> + Send;

  /// Runs a command in the close line of a checkout's repository once its turn comes.
  fn close(&self, close: &Close) -> impl Future<Output = Answer<CloseTurn>> + Send;

  /// Reads whether a shell line starts a heavy command outside the testbed.
  fn classify(&self, line: &str) -> impl Future<Output = Answer<Classified>> + Send;

  /// Plans what moving a checkout to a commit asks of the unit links into it.
  fn plan_units(
    &self,
    checkout: &Path,
    commit: &str,
    hold: bool,
  ) -> impl Future<Output = Answer<UnitPlan>> + Send;

  /// Relinks every unit link a checkout's present commit leaves dangling.
  fn relink_units(&self, checkout: &Path) -> impl Future<Output = Answer<Relinked>> + Send;

  /// Records a commit landed in a checkout's repository as waiting to go live, answering the record
  /// that stands for it.
  fn record_pending(&self, pending: &PendingDeploy) -> impl Future<Output = Answer<Pending>> + Send;

  /// Lists the commits waiting to go live.
  fn pending(&self, of: PendingOf) -> impl Future<Output = Answer<Vec<Pending>>> + Send;

  /// Reads what a checkout's repository has live, `None` where nothing is.
  fn deployed(&self, checkout: &Path) -> impl Future<Output = Answer<Option<Deployed>>> + Send;
}
