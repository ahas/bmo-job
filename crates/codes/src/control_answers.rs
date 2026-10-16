//! A control request as such: refusals of its body and its action.

use axum::http::StatusCode;
use strum::{IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum ControlRefusal {
  #[error("the request's body is not JSON")]
  BodyNotJson,
  #[error("the action asked is none the route takes")]
  ActionWrong,
}

coded!(ControlRefusal);

impl Refused for ControlRefusal {
  fn status(&self) -> StatusCode {
    StatusCode::BAD_REQUEST
  }
}
