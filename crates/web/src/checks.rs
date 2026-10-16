//! Checks: whether a user holds a relation on a worker or a repository.

use serde::{Deserialize, Serialize};

/// The kind of object a check asks about, as bmo-web's rules name it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObjectKind {
  #[serde(rename = "bmo_worker")]
  Worker,
  #[serde(rename = "bmo_repository")]
  Repository,
}

/// A check of a user's grant on an object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectCheck {
  pub kind: ObjectKind,
  /// The worker or the repository asked about.
  pub object: String,
  /// A relation the kind declares: `run` on a worker, `change` on a repository.
  pub relation: String,
  /// The user, as bmo-web names it.
  pub user: String,
}

/// What a check answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checked {
  /// Whether the user holds the relation.
  pub allowed: bool,
  /// The grant that was asked for, `kind:object#relation`.
  pub grant: String,
}
