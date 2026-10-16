//! A refused request answers its plain status and the body {error, code} (docs/api.md).

use axum::body::to_bytes;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use job_base::{Refusal, RefusalBody};
use serde_json::{Value, json};

async fn answered(refusal: Refusal) -> (StatusCode, Value) {
  let response = refusal.into_response();
  let status = response.status();
  let bytes = to_bytes(response.into_body(), usize::MAX).await.expect("the body reads");

  (status, serde_json::from_slice(&bytes).expect("the body is JSON"))
}

#[tokio::test]
async fn a_refusal_answers_its_status_and_a_body_of_error_and_code() {
  let (status, body) = answered(Refusal::absent("todo_absent", "There is no TODO #7.")).await;

  assert_eq!(status, StatusCode::NOT_FOUND);
  assert_eq!(body, json!({ "error": "There is no TODO #7.", "code": "todo_absent" }));
}

#[tokio::test]
async fn the_body_reads_back_as_a_caller_reads_it() {
  let (_, body) = answered(Refusal::conflict("queue_paused", "The queue is paused.")).await;
  let read: RefusalBody = serde_json::from_value(body).expect("the body reads as a refusal");

  assert_eq!(
    read,
    RefusalBody {
      error: "The queue is paused.".into(),
      code: "queue_paused".into(),
    }
  );
}

#[test]
fn each_kind_of_refusal_carries_its_plain_status() {
  let kinds = [
    (Refusal::bad("a", "a"), StatusCode::BAD_REQUEST),
    (Refusal::forbidden("a", "a"), StatusCode::FORBIDDEN),
    (Refusal::absent("a", "a"), StatusCode::NOT_FOUND),
    (Refusal::conflict("a", "a"), StatusCode::CONFLICT),
    (Refusal::new(StatusCode::BAD_GATEWAY, "a", "a"), StatusCode::BAD_GATEWAY),
  ];

  for (refusal, status) in kinds {
    assert_eq!(refusal.status, status);
  }
}

#[tokio::test]
async fn a_database_failure_is_refused_as_the_service_s_own() {
  let refusal = Refusal::from(sea_orm::DbErr::Custom("the disk is full".into()));

  assert_eq!(refusal.to_string(), "the database: Custom Error: the disk is full");

  let (status, body) = answered(refusal).await;

  assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
  assert_eq!(body["code"], "database");
}
