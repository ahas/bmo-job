//! Translations: the readings the translator makes and the ones bmo-gate keeps.

use serde::{Deserialize, Serialize};

/// An original as the translator read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reading {
  /// The original in the language asked for, or as it stands where it needed no translation.
  pub text: String,
  /// Whether every part came back rendered; where one did not, it stands in its original and nothing was
  /// kept.
  pub whole: bool,
}
