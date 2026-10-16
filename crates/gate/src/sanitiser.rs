//! The sanitiser: words on their way into a model's context, read for security-intrusion material.

use serde::{Deserialize, Serialize};

/// Words asked of the sanitiser.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sanitise {
  pub text: String,
  /// What the words are, for the sanitiser's instructions.
  pub label: String,
  /// The room whose run the words belong to, where there is one.
  pub chat: Option<i64>,
}

/// What the sanitiser answered.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sanitised {
  /// The words as they are to be handed on.
  pub text: String,
  /// Whether anything was defused.
  pub changed: bool,
}
