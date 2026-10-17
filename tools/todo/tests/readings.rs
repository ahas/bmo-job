//! The TODO tool's readings as the tool host runs them (docs/tools.md): its manifest held to its entries, and its
//! executable handed one call on standard input against a stand-in of the service's readings, answering a
//! worker's lines as its text and a person's reading as the lines of the reading schema.

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

// A stand-in of the service's readings of the queue; item 9 is none.
async fn service() -> (String, Taken) {
  let taken = Taken::default();
  let absent = || {
    (
      StatusCode::NOT_FOUND,
      Json(json!({ "error": "no item answers to 9", "code": "item_absent" })),
    )
  };
  let router =
    axum::Router::new()
      .route(
        "/todos/worker-lines",
        get(|State(taken): State<Taken>, uri: Uri| async move {
          taken.lock().unwrap().push(uri.to_string());
          Json(json!(["#3 Fix the reader", "#4 Read the notes"]))
        }),
      )
      .route(
        "/todos/{item}/worker-lines",
        get(move |Segment(item): Segment<String>| async move {
          if item == "9" {
            return absent();
          }

          (StatusCode::OK, Json(json!([format!("#{item}"), "What to do."])))
        }),
      )
      .route(
        "/todos",
        get(|State(taken): State<Taken>, uri: Uri| async move {
          taken.lock().unwrap().push(uri.to_string());
          Json(json!([{ "id": 3, "title": "Fix the reader" }]))
        }),
      )
      .route(
        "/todos/{item}",
        get(move |Segment(item): Segment<String>| async move {
          (StatusCode::OK, Json(json!({ "item": item })))
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

async fn called(settings: &Path, entry: &str, arguments: Value) -> Answer {
  let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_todo-tool"))
    .env("BMO_JOB_SETTINGS", settings)
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  let handed =
    json!({ "protocol": 1, "kind": "call", "entry": entry, "arguments": arguments, "session": {} });
  let mut input = child.stdin.take().unwrap();

  input.write_all(handed.to_string().as_bytes()).await.unwrap();
  drop(input);

  let ran = child.wait_with_output().await.unwrap();

  assert!(ran.status.success());
  serde_json::from_slice(&ran.stdout).unwrap()
}

#[test]
fn the_manifest_declares_each_entry_the_executable_answers() {
  assert_eq!(
    differences(Path::new(env!("CARGO_MANIFEST_DIR")), "todo", todo_tool::ENTRIES),
    Vec::<String>::new()
  );
}

#[test]
fn a_listing_asks_for_the_flags_set_and_the_room() {
  let asked: todo_tool::ListInput =
    serde_json::from_value(json!({ "pending": true, "held": false, "chat": -100 })).unwrap();

  assert_eq!(asked.query(), [("pending", "true".to_owned()), ("chat", "-100".to_owned())]);
  assert!(serde_json::from_value::<todo_tool::ListInput>(json!({ "loud": true })).is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_reads_the_queue_and_an_item_as_lines_and_an_item_that_is_none_is_refused() {
  let scratch = tempfile::tempdir().unwrap();
  let (address, taken) = service().await;
  let place = settings(scratch.path(), &address);
  let listed = called(&place, "worker/todo_list", json!({ "pending": true })).await;

  assert_eq!(listed.plain(), "#3 Fix the reader\n#4 Read the notes");
  assert_eq!(taken.lock().unwrap()[0], "/todos/worker-lines?pending=true");

  let shown = called(&place, "worker/todo_show", json!({ "item": "Fix the reader" })).await;

  assert_eq!(shown.plain(), "#Fix the reader\nWhat to do.");

  let refused = called(&place, "worker/todo_show", json!({ "item": "9" })).await;

  assert!(refused.is_error);
  assert_eq!(refused.plain(), "no item answers to 9");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_person_s_reading_is_the_service_s_json_as_the_lines_of_the_reading_schema() {
  let scratch = tempfile::tempdir().unwrap();
  let (address, taken) = service().await;
  let place = settings(scratch.path(), &address);
  let listed = called(&place, "bmo/todo_list", json!({ "all": true })).await;

  assert_eq!(listed.structured, Some(json!({ "lines": [r#"{"id":3,"title":"Fix the reader"}"#] })));
  assert_eq!(taken.lock().unwrap()[0], "/todos?all=true");

  let shown = called(&place, "bmo/todo_show", json!({ "item": "3" })).await;

  assert_eq!(shown.structured, Some(json!({ "lines": [r#"{"item":"3"}"#] })));

  let empty = called(&place, "bmo/todo_show", json!({ "item": " " })).await;

  assert!(empty.is_error);
}
