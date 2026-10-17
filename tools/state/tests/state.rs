//! The state tool as the tool host runs it (docs/tools.md): its manifest held to its entries, and its executable
//! handed one call on standard input against a stand-in of the service's readings, answering the reading as the
//! lines of the reading schema, or why the service refused it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use axum::Json;
use axum::extract::{Path as Segment, State};
use axum::http::{StatusCode, Uri};
use axum::routing::get;
use job_tool_call::Answer;
use job_tool_call::manifest::differences;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

type Taken = Arc<Mutex<Vec<String>>>;

// A stand-in of the service's readings: a room's jobs, and item 3's work order, every other item refused.
async fn service() -> (String, Taken) {
  let taken = Taken::default();
  let router = axum::Router::new()
    .route(
      "/rooms/{chat}/jobs",
      get(|State(taken): State<Taken>, uri: Uri| async move {
        taken.lock().unwrap().push(uri.to_string());
        Json(json!([{ "unit": "bmo-task-2" }, { "unit": "bmo-task-1" }]))
      }),
    )
    .route(
      "/work-orders/{item}",
      get(|Segment(item): Segment<i64>| async move {
        if item != 3 {
          return (
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "there is no such item", "code": "item_absent" })),
          );
        }

        (
          StatusCode::OK,
          Json(json!({ "order": { "status": "draft" }, "lines": ["#3 draft", "planning"] })),
        )
      }),
    )
    .with_state(taken.clone());
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let address = format!("http://{}/", listener.local_addr().unwrap());

  tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
  (address, taken)
}

fn settings(dir: &Path, address: &str) -> PathBuf {
  let place = dir.join("settings.toml");
  let text = format!(
    "[database]\nurl = \"sqlite::memory:\"\n\n[deployment]\nowner_name = \"Owner\"\nagent_name = \"Agent\"\nagent_key = \"agent\"\nsender_name = \"Agent\"\nreceiver_unit = \"receiver\"\nowner_chat = 100\n\n[server]\nlisten = \"127.0.0.1:0\"\naddress = \"{address}\"\nunit = \"job\"\n"
  );

  std::fs::write(&place, text).unwrap();
  place
}

async fn called(settings: &Path, entry: &str, arguments: Value, variables: Value) -> Answer {
  let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_state-tool"))
    .env("BMO_JOB_SETTINGS", settings)
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let handed = json!({
    "protocol": 1, "kind": "call", "entry": entry, "arguments": arguments, "session": { "runVariables": variables },
  });
  let mut input = child.stdin.take().unwrap();

  input.write_all(handed.to_string().as_bytes()).await.unwrap();
  drop(input);

  let ran = child.wait_with_output().await.unwrap();

  assert!(ran.status.success());
  serde_json::from_slice(&ran.stdout).unwrap()
}

#[test]
fn the_manifest_declares_each_reading_the_executable_answers() {
  assert_eq!(
    differences(Path::new(env!("CARGO_MANIFEST_DIR")), "state", state_tool::ENTRIES),
    Vec::<String>::new()
  );
}

#[test]
fn an_argument_the_schema_does_not_name_is_refused() {
  let refused = serde_json::from_value::<state_tool::WorkOrderInput>(json!({ "item": 1, "loud": true }));

  assert!(refused.unwrap_err().to_string().contains("loud"));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_room_s_jobs_are_read_for_the_turn_s_room_and_a_call_naming_no_room_is_refused() {
  let scratch = tempfile::tempdir().unwrap();
  let (address, taken) = service().await;
  let place = settings(scratch.path(), &address);
  let answer = called(&place, "bmo/jobs", json!({ "limit": 2 }), json!({ "BMO_TURN_CHAT": "-100" })).await;

  assert!(!answer.is_error, "{}", answer.plain());
  assert_eq!(
    answer.structured,
    Some(json!({ "lines": [r#"{"unit":"bmo-task-2"}"#, r#"{"unit":"bmo-task-1"}"#] }))
  );
  assert_eq!(taken.lock().unwrap()[0], "/rooms/-100/jobs?limit=2");

  let refused = called(&place, "bmo/jobs", json!({}), json!({})).await;

  assert!(refused.is_error);
  assert_eq!(refused.plain(), job_tool_call::turn::NO_ROOM);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_s_work_order_is_read_as_its_lines_and_an_item_that_is_none_is_refused() {
  let scratch = tempfile::tempdir().unwrap();
  let (address, _) = service().await;
  let place = settings(scratch.path(), &address);
  let answer = called(&place, "bmo/work_order", json!({ "item": 3 }), json!({})).await;

  assert_eq!(answer.structured, Some(json!({ "lines": ["#3 draft", "planning"] })));
  assert_eq!(answer.plain(), "#3 draft\nplanning");

  let refused = called(&place, "bmo/work_order", json!({ "item": 9 }), json!({})).await;

  assert!(refused.is_error);
  assert_eq!(refused.plain(), "there is no such item");

  let wrong = called(&place, "bmo/work_order", json!({ "item": "three" }), json!({})).await;

  assert!(
    wrong.is_error && wrong.plain().starts_with("invalid arguments: bmo/work_order"),
    "{}",
    wrong.plain()
  );
}
