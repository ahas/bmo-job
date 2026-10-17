//! The TODO tool's filings as the tool host runs them (docs/tools.md): each call run as the unit CLI's `todo`
//! verb with the turn's agent, run and work-order step, against a stand-in of `bmo-job` on the search path that
//! writes down the words and the variables it was started with, answers one code, and refuses item 99. One test
//! holds every filing, so the stand-in is written before any process of this binary starts.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use job_tool_call::Answer;
use serde_json::{Value, json};
use tokio::io::AsyncWriteExt;

const STAND_IN: &str = r#"#!/bin/sh
here=$(dirname "$0")
printf '%s\n' "$@" > "$here/words"
printf 'agent=%s run=%s step=%s\n' "$BMO_TURN_AGENT" "$BMO_RUN_NAME" "$BMO_WORK_ORDER_STEP" > "$here/variables"
case " $* " in *" 99 "*) echo "no item answers to 99" >&2; exit 1;; esac
echo '{"code":"item_filed","id":12}'
"#;

fn settings(dir: &Path) -> std::path::PathBuf {
  let place = dir.join("settings.toml");
  let text = "[database]\nurl = \"sqlite::memory:\"\n\n[deployment]\nowner_name = \"Owner\"\nagent_name = \"Agent\"\nagent_key = \"agent\"\nsender_name = \"Agent\"\nreceiver_unit = \"receiver\"\nowner_chat = 100\n\n[server]\nlisten = \"127.0.0.1:0\"\naddress = \"http://127.0.0.1:1/\"\nunit = \"job\"\n";

  std::fs::write(&place, text).unwrap();
  place
}

async fn called(settings: &Path, bin: &Path, entry: &str, arguments: Value, variables: Value) -> Answer {
  let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap_or_default());
  let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_todo-tool"))
    .env("BMO_JOB_SETTINGS", settings)
    .env("PATH", path)
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

fn heard(bin: &Path, file: &str) -> String {
  std::fs::read_to_string(bin.join(file)).unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_row_is_filed_words_added_and_an_approval_given_as_the_unit_cli_s_todo_under_the_turn() {
  let scratch = tempfile::tempdir().unwrap();
  let bin = scratch.path().join("bin");
  let program = bin.join("bmo-job");

  std::fs::create_dir_all(&bin).unwrap();
  std::fs::write(&program, STAND_IN).unwrap();
  std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();

  let place = settings(scratch.path());
  let turn = json!({ "BMO_TURN_CHAT": "-100", "BMO_TURN_AGENT": "agent", "BMO_RUN_NAME": "run-7" });
  let filed = called(
    &place,
    &bin,
    "bmo/todo_add",
    json!({
      "title": "Fix the reader", "what": "The reader drops a line.", "do": "Keep the line.", "instruction": [4, 5],
      "cowork": true, "impact": { "nodes": [{ "id": "reader" }] }, "message": 5
    }),
    turn.clone(),
  )
  .await;

  assert!(!filed.is_error, "{}", filed.plain());
  assert_eq!(filed.plain(), r#"{"code":"item_filed","id":12}"#);
  assert_eq!(
    heard(&bin, "words").lines().collect::<Vec<_>>(),
    [
      "todo",
      "--add",
      "--chat",
      "-100",
      "--title",
      "Fix the reader",
      "--what",
      "The reader drops a line.",
      "--do",
      "Keep the line.",
      "--instruction",
      "4",
      "--instruction",
      "5",
      "--model",
      "auto",
      "--cowork",
      "--impact",
      r#"{"nodes":[{"id":"reader"}]}"#
    ]
  );
  assert_eq!(heard(&bin, "variables"), "agent=agent run=run-7 step=\n");

  let appended = called(
    &place,
    &bin,
    "bmo/todo_append",
    json!({ "item": "3", "text": "And the next one." }),
    turn.clone(),
  )
  .await;

  assert!(!appended.is_error);
  assert_eq!(
    heard(&bin, "words").lines().collect::<Vec<_>>(),
    ["todo", "--append", "3", "--text", "And the next one."]
  );

  let unapproved = called(
    &place,
    &bin,
    "bmo/todo_approve",
    json!({ "item": "3", "unapprove": true, "note": "" }),
    turn.clone(),
  )
  .await;

  assert!(!unapproved.is_error);
  assert_eq!(
    heard(&bin, "words").lines().collect::<Vec<_>>(),
    ["todo", "--unapprove", "3", "--note", ""]
  );

  // What the verb refuses is answered as an error with its sentence.
  let refused = called(&place, &bin, "bmo/todo_approve", json!({ "item": "99" }), turn).await;

  assert!(refused.is_error);
  assert_eq!(refused.plain(), "no item answers to 99");

  // A filing naming no room, in a turn that answers none, never reaches the verb.
  std::fs::remove_file(bin.join("words")).unwrap();

  let roomless = called(&place, &bin, "bmo/todo_add", json!({ "title": "x" }), json!({})).await;

  assert!(roomless.is_error);
  assert_eq!(roomless.plain(), job_tool_call::turn::NO_ROOM);
  assert!(!bin.join("words").exists());
}
