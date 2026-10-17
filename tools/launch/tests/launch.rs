//! The launch tool as the tool host runs it (docs/tools.md): its manifest held to its entries, and its executable
//! handed one call on standard input against a stand-in of the service's `POST /task`, answering on standard
//! output what the service answered or why it refused.

use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use job_tool_call::manifest::differences;
use job_tool_call::{Answer, Handed};
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

type Taken = Arc<Mutex<Vec<Value>>>;

// A stand-in of the service: `POST /task` recorded and answered with a launch, or refused where the prompt says
// so.
async fn service() -> (String, Taken) {
  let taken = Taken::default();
  let router = axum::Router::new()
    .route(
      "/task",
      post(|State(taken): State<Taken>, Json(body): Json<Value>| async move {
        let refused = body["prompt"] == "refuse";

        taken.lock().unwrap().push(body);

        if refused {
          return (
            StatusCode::CONFLICT,
            Json(json!({ "error": "the checkout is at work already", "code": "checkout_held" })),
          );
        }

        (
          StatusCode::OK,
          Json(json!({ "code": "job_launched", "job": 41, "unit": "bmo-task-41" })),
        )
      }),
    )
    .with_state(taken.clone());
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = format!("http://{}/", listener.local_addr().unwrap());

  tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
  (address, taken)
}

// The settings file a tool reads the service's address out of.
fn settings(dir: &std::path::Path, address: &str) -> std::path::PathBuf {
  let place = dir.join("settings.toml");
  let text = format!(
    "[database]\nurl = \"sqlite::memory:\"\n\n[deployment]\nowner_name = \"Owner\"\nagent_name = \"Agent\"\nagent_key = \"agent\"\nsender_name = \"Agent\"\nreceiver_unit = \"receiver\"\nowner_chat = 100\n\n[server]\nlisten = \"127.0.0.1:0\"\naddress = \"{address}\"\nunit = \"job\"\n"
  );

  std::fs::write(&place, text).unwrap();
  place
}

// One call handed to the executable, and the answer it wrote.
async fn called(settings: &std::path::Path, arguments: Value, variables: Value) -> Answer {
  let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_launch-tool"))
    .env("BMO_JOB_SETTINGS", settings)
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let handed = json!({
    "protocol": 1, "kind": "call", "entry": "bmo/task", "arguments": arguments,
    "session": { "runVariables": variables },
  });
  let mut input = child.stdin.take().unwrap();

  input.write_all(handed.to_string().as_bytes()).await.unwrap();
  drop(input);

  let ran = child.wait_with_output().await.unwrap();

  assert!(ran.status.success());
  serde_json::from_slice(&ran.stdout).unwrap()
}

#[test]
fn the_manifest_declares_each_entry_the_executable_answers() {
  let differences =
    differences(std::path::Path::new(env!("CARGO_MANIFEST_DIR")), "launch", launch_tool::ENTRIES);

  assert_eq!(differences, Vec::<String>::new());
}

#[test]
fn a_call_reads_as_the_host_hands_it_and_an_argument_the_schema_does_not_name_is_refused() {
  let handed: Handed = serde_json::from_value(json!({
    "protocol": 1, "kind": "call", "entry": "bmo/task", "arguments": { "prompt": "x" }, "session": {}
  }))
  .unwrap();

  assert_eq!(handed.entry, "bmo/task");
  assert!(
    serde_json::from_value::<launch_tool::TaskInput>(json!({ "prompt": "x", "loud": true }))
      .unwrap_err()
      .to_string()
      .contains("loud")
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_is_launched_in_the_turn_s_room_as_its_session_and_a_refusal_is_answered_as_an_error() {
  let scratch = tempfile::tempdir().unwrap();
  let (address, taken) = service().await;
  let place = settings(scratch.path(), &address);
  let turn = json!({ "BMO_TURN_CHAT": "-100", "BMO_TURN_AGENT": "agent" });
  let answer =
    called(&place, json!({ "prompt": "Look at the logs.", "todo": 7, "message": 12 }), turn.clone()).await;

  assert!(!answer.is_error, "{}", answer.plain());
  assert_eq!(serde_json::from_str::<Value>(&answer.plain()).unwrap()["code"], "job_launched");
  assert_eq!(
    taken.lock().unwrap()[0],
    json!({
      "chat": -100, "prompt": "Look at the logs.", "model": null, "item": 7, "purpose": null, "worker": null,
      "cowork": false, "started_by": "chat:-100"
    })
  );

  let refused = called(&place, json!({ "prompt": "refuse", "chat": 5 }), turn).await;

  assert!(refused.is_error);
  assert_eq!(refused.plain(), "the checkout is at work already");
  assert_eq!(taken.lock().unwrap()[1]["chat"], 5);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_call_naming_no_room_in_a_turn_that_answers_none_is_refused_before_anything_is_asked() {
  let scratch = tempfile::tempdir().unwrap();
  let (address, taken) = service().await;
  let place = settings(scratch.path(), &address);
  let answer = called(&place, json!({ "prompt": "x" }), json!({})).await;

  assert!(answer.is_error);
  assert_eq!(answer.plain(), job_tool_call::turn::NO_ROOM);
  assert!(taken.lock().unwrap().is_empty());
}
