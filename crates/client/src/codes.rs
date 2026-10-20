//! The error shape and every code the service answers in it (docs/api.md, "Access").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A refusal or failure: a code for a machine and one English sentence for a person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[schemars(rename = "Error")]
pub struct ErrorBody {
  pub code: String,
  pub error: String,
}

/// Every code the service answers, each with the status it is answered at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Code {
  BadRequest,
  Unauthorized,
  NoRoute,
  NotFound,
  Conflict,
  Gone,
  UnknownItem,
  NotAChoice,
  QueueNotOpen,
  Failed,
}

impl Code {
  /// The code as the body carries it.
  pub fn as_str(self) -> &'static str {
    match self {
      Self::BadRequest => "bad-request",
      Self::Unauthorized => "unauthorized",
      Self::NoRoute => "no-route",
      Self::NotFound => "not-found",
      Self::Conflict => "conflict",
      Self::Gone => "gone",
      Self::UnknownItem => "unknown-item",
      Self::NotAChoice => "not-a-choice",
      Self::QueueNotOpen => "queue-not-open",
      Self::Failed => "failed",
    }
  }

  /// The status the code is answered at.
  pub fn status(self) -> u16 {
    match self {
      Self::BadRequest => 400,
      Self::Unauthorized => 401,
      Self::NoRoute | Self::NotFound => 404,
      Self::Conflict => 409,
      Self::Gone => 410,
      Self::UnknownItem | Self::NotAChoice | Self::QueueNotOpen => 422,
      Self::Failed => 500,
    }
  }
}
