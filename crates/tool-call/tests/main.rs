//! The call protocol and the shared access, held to the shapes the host hands and the tools read.

use std::path::PathBuf;

use serde_json::json;
use tool_call::{Answer, Handed, address, entry_name, turn_room, variable};

#[test]
fn a_full_name_answers_by_its_last_part() {
  assert_eq!(entry_name("worker/todo_list"), "todo_list");
  assert_eq!(entry_name("todo_list"), "todo_list");
}

#[test]
fn a_listen_setting_reads_as_an_address() {
  assert_eq!(address("127.0.0.1:13103"), "http://127.0.0.1:13103");
  assert_eq!(address("http://127.0.0.1:13103/"), "http://127.0.0.1:13103");
}

#[test]
fn a_record_ignores_the_fields_this_crate_does_not_read() {
  let handed: Handed = serde_json::from_value(json!({
    "protocol": 1,
    "kind": "call",
    "entry": "bmo/todo_list",
    "arguments": {},
    "session": {
      "holderName": "worker-a",
      "namespaces": ["bmo", "worker"],
      "capabilities": ["queue.read"],
      "grants": [{ "resource": "approve" }],
      "variables": { "BMO_TURN_CHAT": "42" },
      "labels": { "line": "chat" }
    },
    "callback": "/tmp/socket"
  }))
  .unwrap();

  assert_eq!(handed.session.holder_name, "worker-a");
  assert_eq!(turn_room(&handed.session), Some(42));
  assert_eq!(handed.callback, Some(PathBuf::from("/tmp/socket")));
}

#[test]
fn an_env_file_is_read_by_its_key_value_lines() {
  let dir = tempfile::tempdir().unwrap();
  let path = dir.path().join("service.env");
  std::fs::write(&path, "# a note\nADDRESS=http://127.0.0.1:1\nTOKEN=abc\n").unwrap();
  let file = tool_call::env_file(&path);

  assert_eq!(variable("ADDRESS", &file).as_deref(), Some("http://127.0.0.1:1"));
  assert_eq!(variable("TOKEN", &file).as_deref(), Some("abc"));
  assert_eq!(variable("NOTHING", &file), None);
}

#[test]
fn a_reading_answers_as_lines_and_as_the_object_the_manifest_names() {
  let answer = Answer::lines(vec!["#1 [open] one".to_owned()]);

  assert!(!answer.is_error);
  assert_eq!(answer.structured.as_ref().unwrap()["lines"][0], "#1 [open] one");
  assert_eq!(answer.content[0]["text"], "#1 [open] one");
}

#[test]
fn nothing_read_answers_a_sentence_rather_than_an_empty_text() {
  let answer = Answer::lines(Vec::new());

  assert_eq!(answer.content[0]["text"], "Nothing stands there now.");
}
