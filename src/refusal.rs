//! A refusal or failure as the service answers it: a status, a code and one English sentence.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use bmo_job_client::{Code, ErrorBody};

/// Why a request was not done, in the common error shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
  pub code: Code,
  pub sentence: String,
}

impl Refusal {
  pub fn new(code: Code, sentence: impl Into<String>) -> Self {
    Self {
      code,
      sentence: sentence.into(),
    }
  }

  pub fn bad_request(sentence: impl Into<String>) -> Self {
    Self::new(Code::BadRequest, sentence)
  }

  pub fn not_found(sentence: impl Into<String>) -> Self {
    Self::new(Code::NotFound, sentence)
  }

  pub fn conflict(sentence: impl Into<String>) -> Self {
    Self::new(Code::Conflict, sentence)
  }

  pub fn gone(sentence: impl Into<String>) -> Self {
    Self::new(Code::Gone, sentence)
  }
}

impl From<sqlx::Error> for Refusal {
  fn from(error: sqlx::Error) -> Self {
    Self::new(Code::Failed, format!("The database failed: {error}."))
  }
}

impl From<serde_json::Error> for Refusal {
  fn from(error: serde_json::Error) -> Self {
    Self::new(Code::Failed, format!("A stored value could not be read: {error}."))
  }
}

impl IntoResponse for Refusal {
  fn into_response(self) -> Response {
    let status = StatusCode::from_u16(self.code.status()).expect("every code's status is a status");
    let body = ErrorBody {
      code: self.code.as_str().to_owned(),
      error: self.sentence,
    };
    (status, Json(body)).into_response()
  }
}
