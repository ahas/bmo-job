//! The web port's contract: every call is made on the method, path and body the API document gives it,
//! and its answer is read as the call's shape (docs/api.md, "bmo-web").

mod support;

use job_web::{Checked, ObjectCheck, ObjectKind, Web, WebClient, WebError};
use serde_json::json;
use support::web::Stub;
use url::Url;

// The account model.

#[tokio::test]
async fn model_in_force_reads_whether_the_account_model_is_in_force() {
  let stub = Stub::start().await;

  for in_force in [true, false] {
    stub.answer("GET /accounts/model", 200, json!({ "in_force": in_force }));

    assert_eq!(stub.client().model_in_force().await.unwrap(), in_force);
  }

  assert!(stub.calls().iter().all(|call| call.target == "/accounts/model" && call.body.is_none()));
}

// Users.

#[tokio::test]
async fn user_of_sender_reads_the_user_a_sender_is_linked_to() {
  let stub = Stub::start().await;

  stub.answer("GET /senders/{sender}/user", 200, json!({ "user": "17" }));

  assert_eq!(stub.client().user_of_sender(-501).await.unwrap(), Some("17".to_owned()));

  stub.took("GET /senders/{sender}/user", "/senders/-501/user", None);
}

#[tokio::test]
async fn a_sender_linked_to_no_user_reads_as_none() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /senders/{sender}/user",
    404,
    json!({ "error": "The sender is linked to no user.", "code": "unknown" }),
  );

  assert_eq!(stub.client().user_of_sender(501).await.unwrap(), None);
}

// Checks.

#[tokio::test]
async fn check_object_posts_the_check_and_reads_whether_it_opens() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /checks/object",
    200,
    json!({ "allowed": false, "grant": "bmo_repository:job#change" }),
  );

  let check = ObjectCheck {
    kind: ObjectKind::Repository,
    object: "job".into(),
    relation: "change".into(),
    user: "17".into(),
  };
  let checked = stub.client().check_object(&check).await.unwrap();

  assert_eq!(
    checked,
    Checked {
      allowed: false,
      grant: "bmo_repository:job#change".into(),
    }
  );
  stub.took(
    "POST /checks/object",
    "/checks/object",
    Some(json!({ "kind": "bmo_repository", "object": "job", "relation": "change", "user": "17" })),
  );
}

#[tokio::test]
async fn a_worker_is_checked_as_its_kind() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /checks/object",
    200,
    json!({ "allowed": true, "grant": "bmo_worker:builder#run" }),
  );

  let check = ObjectCheck {
    kind: ObjectKind::Worker,
    object: "builder".into(),
    relation: "run".into(),
    user: "17".into(),
  };

  assert!(stub.client().check_object(&check).await.unwrap().allowed);
  assert_eq!(stub.calls()[0].body.as_ref().unwrap()["kind"], "bmo_worker");
}

// Failures.

#[tokio::test]
async fn a_refusal_is_handed_on_with_its_status_code_and_sentence() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /checks/object",
    400,
    json!({ "error": "The kind declares no such relation.", "code": "undeclared" }),
  );
  stub.answer(
    "GET /accounts/model",
    503,
    json!({ "error": "The database is not reachable.", "code": "database" }),
  );

  let check = ObjectCheck {
    kind: ObjectKind::Worker,
    object: "builder".into(),
    relation: "change".into(),
    user: "17".into(),
  };
  let unchecked = stub.client().check_object(&check).await.unwrap_err();
  let unread = stub.client().model_in_force().await.unwrap_err();

  assert!(
    matches!(
      &unchecked,
      WebError::Refused { call: "check_object", status: 400, code, error }
        if code == "undeclared" && error == "The kind declares no such relation."
    ),
    "{unchecked}"
  );
  assert!(
    matches!(
      &unread,
      WebError::Refused { call: "model_in_force", status: 503, code, .. } if code == "database"
    ),
    "{unread}"
  );
}

#[tokio::test]
async fn a_read_of_a_sender_that_is_refused_otherwise_is_not_nothing_found() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /senders/{sender}/user",
    500,
    json!({ "error": "The database failed.", "code": "database" }),
  );

  let failed = stub.client().user_of_sender(501).await.unwrap_err();

  assert!(
    matches!(&failed, WebError::Refused { call: "user_of_sender", status: 500, code, .. } if code == "database"),
    "{failed}"
  );
}

#[tokio::test]
async fn an_answer_that_does_not_read_is_malformed() {
  let stub = Stub::start().await;

  stub.answer("GET /senders/{sender}/user", 200, json!({ "user": 17 }));
  stub.answer("GET /accounts/model", 502, json!("bad gateway"));

  let unread = stub.client().user_of_sender(501).await.unwrap_err();
  let unsaid = stub.client().model_in_force().await.unwrap_err();

  assert!(
    matches!(
      unread,
      WebError::Malformed {
        call: "user_of_sender",
        status: 200,
        ..
      }
    ),
    "{unread}"
  );
  assert!(
    matches!(
      unsaid,
      WebError::Malformed {
        call: "model_in_force",
        status: 502,
        ..
      }
    ),
    "{unsaid}"
  );
}

#[tokio::test]
async fn a_call_that_reaches_nothing_is_unreachable() {
  let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let address = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();

  drop(listener);

  let failed = WebClient::new(address).unwrap().model_in_force().await.unwrap_err();

  assert!(
    matches!(
      failed,
      WebError::Unreachable {
        call: "model_in_force",
        ..
      }
    ),
    "{failed}"
  );
}

#[test]
fn an_address_that_is_not_http_is_refused() {
  for address in ["https://web.invalid/", "mailto:web@web.invalid", "file:///web"] {
    let refused = WebClient::new(Url::parse(address).unwrap()).unwrap_err();

    assert!(matches!(refused, WebError::NotAnAddress { .. }), "{address}: {refused}");
  }
}
