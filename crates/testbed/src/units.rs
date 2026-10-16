//! The unit links into a checkout: what a move asks of them, and relinking them once it is made.

use serde::Deserialize;

/// What moving a checkout to a commit asks of the unit links into it.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct UnitPlan {
  /// Each link the commit would leave dangling and the file it is pointed at.
  pub relinks: Vec<Relink>,
  /// The units running in a folder the commit no longer has, read only where the move holds for them.
  pub running: Vec<String>,
  /// The links with no single file of their name in the commit.
  pub unmatched: Vec<String>,
}

impl UnitPlan {
  /// Whether the move is held: a unit runs in a folder the commit lacks, or a link has nowhere to point.
  pub fn held(&self) -> bool {
    !self.running.is_empty() || !self.unmatched.is_empty()
  }
}

/// One link and the path in the commit it is pointed at.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Relink {
  pub name: String,
  pub file: String,
}

/// The unit links a relink moved, and those it could not.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Relinked {
  pub relinked: Vec<String>,
  pub dangling: Vec<String>,
  pub unmatched: Vec<String>,
}
