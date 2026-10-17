//! A control request as such: refusals of its body, its fields, its action and its route, and the forms
//! a field takes.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum ControlRefusal {
  #[error("the request's body is not JSON")]
  BodyNotJson,
  #[error("the action asked is none the route takes")]
  ActionWrong,
  #[error("{name} is missing or is not {}", takes.described())]
  FieldWrong { name: String, takes: FieldForm },
  #[error("no route answers this method and path")]
  RouteAbsent,
  #[error("the running build does not carry the verb {verb}")]
  NotBuilt { verb: String },
  #[error("this request takes no files")]
  TakesNoFiles,
  #[error("at most {most} files come with this request")]
  TooManyFiles { most: u32 },
  #[error("{name} is empty, so it was not sent")]
  FileEmpty { name: String },
  #[error("{name} is {size} bytes, and a file comes with this request up to {most} bytes")]
  FileTooLarge { name: String, size: u64, most: u64 },
}

coded!(ControlRefusal);

impl Refused for ControlRefusal {
  fn status(&self) -> StatusCode {
    match self {
      Self::RouteAbsent => StatusCode::NOT_FOUND,
      Self::NotBuilt { .. } => StatusCode::NOT_IMPLEMENTED,
      Self::FileTooLarge { .. } => StatusCode::PAYLOAD_TOO_LARGE,
      _ => StatusCode::BAD_REQUEST,
    }
  }
}

/// The form a field of a request takes.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum FieldForm {
  WholeNumber,
  Boolean,
  Text,
  Words,
  Numbers,
  Object,
  Direction,
  StopTarget,
}

impl FieldForm {
  /// The form as the refusal's sentence names it.
  pub fn described(self) -> &'static str {
    match self {
      Self::WholeNumber => "a whole number",
      Self::Boolean => "true or false",
      Self::Text => "a string",
      Self::Words => "a string with something in it",
      Self::Numbers => "a list of whole numbers",
      Self::Object => "an object",
      Self::Direction => "up or down",
      Self::StopTarget => "a stop target of a kind and an id",
    }
  }
}
