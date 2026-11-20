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
  ItemFrozen,
  Gone,
  UnknownItem,
  NotAChoice,
  NoChoices,
  NoRecommendation,
  QueueNotOpen,
  NotTheAddressee,
  LabelValueUnknown,
  ReasonRequired,
  ReasonWithoutHold,
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
      Self::ItemFrozen => "item-frozen",
      Self::Gone => "gone",
      Self::UnknownItem => "unknown-item",
      Self::NotAChoice => "not-a-choice",
      Self::NoChoices => "no-choices",
      Self::NoRecommendation => "no-recommendation",
      Self::QueueNotOpen => "queue-not-open",
      Self::NotTheAddressee => "not-the-addressee",
      Self::LabelValueUnknown => "label_value_unknown",
      Self::ReasonRequired => "reason_required",
      Self::ReasonWithoutHold => "reason_without_hold",
      Self::Failed => "failed",
    }
  }

  /// The status the code is answered at.
  pub fn status(self) -> u16 {
    match self {
      Self::BadRequest | Self::ReasonRequired | Self::ReasonWithoutHold => 400,
      Self::Unauthorized => 401,
      Self::NotTheAddressee => 403,
      Self::NoRoute | Self::NotFound => 404,
      Self::Conflict | Self::ItemFrozen => 409,
      Self::Gone => 410,
      Self::UnknownItem
      | Self::NotAChoice
      | Self::NoChoices
      | Self::NoRecommendation
      | Self::QueueNotOpen
      | Self::LabelValueUnknown => 422,
      Self::Failed => 500,
    }
  }
}
