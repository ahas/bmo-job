//! Brain holds: a job's run holding the brain's editors off, and the editing locks those editors take.

use serde::{Deserialize, Serialize};

/// What holding the brain for a run came to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hold {
  /// Whether the run's hold stands.
  pub held: bool,
  /// The editing locks a brain editor holds now, looked at after the hold was set.
  pub taken: Vec<String>,
}

/// The holds and the locks as they stand now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holds {
  /// The runs holding the brain.
  pub holders: Vec<String>,
  /// The editing locks a brain editor holds now.
  pub taken: Vec<String>,
}

/// How a wait for the brain's editors ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Waited {
  /// Whether every editing lock came free.
  pub free: bool,
  /// The outcome for a person, a line each.
  pub lines: Vec<String>,
}
