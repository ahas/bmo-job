//! A refused request: a plain HTTP status, a stable snake_case code a machine decides on, and one
//! English sentence a person reads (docs/api.md, "A refused request").

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::{Deserialize, Serialize};

/// A refusal, said with its status, its code and a sentence naming why.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{error}")]
pub struct Refusal {
  pub status: StatusCode,
  pub code: &'static str,
  pub error: String,
}

/// The body a refusal is answered with, as a caller reads it back.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RefusalBody {
  pub error: String,
  pub code: String,
}

impl Refusal {
  pub fn new(status: StatusCode, code: &'static str, error: impl Into<String>) -> Self {
    Self {
      status,
      code,
      error: error.into(),
    }
  }

  /// A request wrong as asked.
  pub fn bad(code: &'static str, error: impl Into<String>) -> Self {
    Self::new(StatusCode::BAD_REQUEST, code, error)
  }

  /// A request the asker may not make.
  pub fn forbidden(code: &'static str, error: impl Into<String>) -> Self {
    Self::new(StatusCode::FORBIDDEN, code, error)
  }

  /// A request for something that does not exist.
  pub fn absent(code: &'static str, error: impl Into<String>) -> Self {
    Self::new(StatusCode::NOT_FOUND, code, error)
  }

  /// A request that conflicts with the present state.
  pub fn conflict(code: &'static str, error: impl Into<String>) -> Self {
    Self::new(StatusCode::CONFLICT, code, error)
  }

  pub fn body(&self) -> RefusalBody {
    RefusalBody {
      error: self.error.clone(),
      code: self.code.to_owned(),
    }
  }
}

impl From<sea_orm::DbErr> for Refusal {
  fn from(error: sea_orm::DbErr) -> Self {
    Self::new(StatusCode::INTERNAL_SERVER_ERROR, "database", format!("the database: {error}"))
  }
}

impl IntoResponse for Refusal {
  fn into_response(self) -> Response {
    (self.status, Json(self.body())).into_response()
  }
}
