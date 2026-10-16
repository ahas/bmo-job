//! The gate port's contract: every call is made on the method, path and body the API document gives it,
//! and its answer is read as the call's shape (docs/api.md, "bmo-gate").

mod support;

use std::collections::HashMap;

use job_gate::{Gate, GateClient, GateError, Reading, Sanitise, Sanitised};
use serde_json::json;
use support::gate::Stub;
use url::Url;

fn originals() -> Vec<String> {
  vec!["Ein Satz.".to_owned(), "Already English.".to_owned()]
}

// Translations.

#[tokio::test]
async fn kept_reads_the_translations_kept_for_the_originals_that_have_one() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /translations/kept/read",
    200,
    json!({ "kept": [{ "original": "Ein Satz.", "text": "A sentence." }] }),
  );

  let kept = stub.client().kept("en", &originals()).await.unwrap();

  assert_eq!(kept, HashMap::from([("Ein Satz.".to_owned(), "A sentence.".to_owned())]));
  stub.took(
    "POST /translations/kept/read",
    "/translations/kept/read",
    Some(json!({ "language": "en", "originals": ["Ein Satz.", "Already English."] })),
  );
}

#[tokio::test]
async fn translate_reads_one_reading_for_each_original_in_its_order() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /translations",
    200,
    json!({ "readings": [
      { "text": "A sentence.", "whole": true },
      { "text": "Already English.", "whole": false },
    ] }),
  );

  let readings = stub.client().translate("en", &originals()).await.unwrap();

  assert_eq!(
    readings,
    vec![
      Reading {
        text: "A sentence.".into(),
        whole: true,
      },
      Reading {
        text: "Already English.".into(),
        whole: false,
      },
    ]
  );
  stub.took(
    "POST /translations",
    "/translations",
    Some(json!({ "language": "en", "originals": ["Ein Satz.", "Already English."] })),
  );
}

#[tokio::test]
async fn readings_that_do_not_stand_one_for_each_original_are_malformed() {
  let stub = Stub::start().await;

  for readings in [json!([{ "text": "A sentence.", "whole": true }]), json!([])] {
    stub.answer("POST /translations", 200, json!({ "readings": readings }));

    let failed = stub.client().translate("en", &originals()).await.unwrap_err();

    assert!(
      matches!(
        failed,
        GateError::Malformed {
          call: "translate",
          status: 200,
          ..
        }
      ),
      "{failed}"
    );
  }
}

#[tokio::test]
async fn keep_puts_the_translation_under_its_original_and_language() {
  let stub = Stub::start().await;

  stub.answer_empty("PUT /translations/kept", 204);

  stub.client().keep("en", "Ein Satz.", "A sentence.").await.unwrap();

  stub.took(
    "PUT /translations/kept",
    "/translations/kept",
    Some(json!({ "language": "en", "original": "Ein Satz.", "text": "A sentence." })),
  );
}

// The sanitiser.

#[tokio::test]
async fn sanitise_posts_the_words_and_reads_what_to_hand_on() {
  let stub = Stub::start().await;

  stub.answer("POST /sanitised", 200, json!({ "text": "Defused.", "changed": true }));

  let asked = Sanitise {
    text: "Words.".into(),
    label: "a job's prompt".into(),
    chat: Some(-1001),
  };
  let sanitised = stub.client().sanitise(&asked).await.unwrap();

  assert_eq!(
    sanitised,
    Sanitised {
      text: "Defused.".into(),
      changed: true,
    }
  );
  stub.took(
    "POST /sanitised",
    "/sanitised",
    Some(json!({ "text": "Words.", "label": "a job's prompt", "chat": -1001 })),
  );
}

#[tokio::test]
async fn words_of_no_room_are_sanitised_with_no_chat() {
  let stub = Stub::start().await;

  stub.answer("POST /sanitised", 200, json!({ "text": "Words.", "changed": false }));

  let asked = Sanitise {
    text: "Words.".into(),
    label: "a job's prompt".into(),
    chat: None,
  };

  assert!(!stub.client().sanitise(&asked).await.unwrap().changed);
  assert_eq!(stub.calls()[0].body.as_ref().unwrap()["chat"], json!(null));
}

// Failures.

#[tokio::test]
async fn a_refusal_is_handed_on_with_its_status_code_and_sentence() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /sanitised",
    409,
    json!({ "error": "The sanitiser's run was stopped.", "code": "stopped" }),
  );
  stub.answer(
    "PUT /translations/kept",
    500,
    json!({ "error": "The database failed.", "code": "database" }),
  );

  let asked = Sanitise {
    text: "Words.".into(),
    label: "a job's prompt".into(),
    chat: None,
  };
  let stopped = stub.client().sanitise(&asked).await.unwrap_err();
  let unkept = stub.client().keep("en", "Ein Satz.", "A sentence.").await.unwrap_err();

  assert!(
    matches!(
      &stopped,
      GateError::Refused { call: "sanitise", status: 409, code, error }
        if code == "stopped" && error == "The sanitiser's run was stopped."
    ),
    "{stopped}"
  );
  assert!(
    matches!(
      &unkept,
      GateError::Refused { call: "keep", status: 500, code, .. } if code == "database"
    ),
    "{unkept}"
  );
}

#[tokio::test]
async fn an_answer_that_does_not_read_is_malformed() {
  let stub = Stub::start().await;

  stub.answer("POST /translations/kept/read", 200, json!({ "kept": { "Ein Satz.": "A sentence." } }));
  stub.answer("POST /translations", 502, json!("bad gateway"));

  let unread = stub.client().kept("en", &originals()).await.unwrap_err();
  let unsaid = stub.client().translate("en", &originals()).await.unwrap_err();

  assert!(
    matches!(
      unread,
      GateError::Malformed {
        call: "kept",
        status: 200,
        ..
      }
    ),
    "{unread}"
  );
  assert!(
    matches!(
      unsaid,
      GateError::Malformed {
        call: "translate",
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

  let failed = GateClient::new(address).unwrap().kept("en", &originals()).await.unwrap_err();

  assert!(matches!(failed, GateError::Unreachable { call: "kept", .. }), "{failed}");
}

#[test]
fn an_address_that_is_not_http_is_refused() {
  for address in ["https://gate.invalid/", "mailto:gate@gate.invalid", "file:///gate"] {
    let refused = GateClient::new(Url::parse(address).unwrap()).unwrap_err();

    assert!(matches!(refused, GateError::NotAnAddress { .. }), "{address}: {refused}");
  }
}
