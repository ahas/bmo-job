//! A call of the service's tools (docs/tools.md, "A call" and "The manifest"): what the host hands read and
//! answered, the turn a session carries, and the check that finds every way a manifest differs from its entries.

use futures_util::future::BoxFuture;
use job_tool_call::manifest::differences;
use job_tool_call::{Answer, Entry, Turn, answer};
use serde_json::{Value, json};

// An entry that says the room it would act in and the argument it was handed.
struct Echo;

impl Entry for Echo {
  fn namespace(&self) -> &'static str {
    "worker"
  }

  fn name(&self) -> &'static str {
    "echo"
  }

  fn call<'a>(&'a self, turn: &'a Turn, arguments: Value) -> BoxFuture<'a, Answer> {
    Box::pin(async move { Answer::text(format!("{:?} {}", turn.room(None), arguments["said"])) })
  }
}

const ENTRIES: &[&dyn Entry] = &[&Echo];

fn handed(asked: Value) -> String {
  let mut said =
    json!({ "protocol": 1, "session": { "runVariables": { "BMO_TURN_CHAT": "-100", "OTHER": 3 } } });

  said.as_object_mut().unwrap().extend(asked.as_object().unwrap().clone());
  said.to_string()
}

#[tokio::test]
async fn a_call_is_answered_by_the_entry_it_names_under_the_turn_its_session_carries() {
  let answered = answer(
    ENTRIES,
    &handed(json!({ "kind": "call", "entry": "worker/echo", "arguments": { "said": "hi" } })),
  )
  .await
  .unwrap();

  assert_eq!(answered, Answer::text(r#"Some(-100) "hi""#));
  assert_eq!(
    serde_json::to_value(&answered).unwrap(),
    json!({ "content": [{ "type": "text", "text": r#"Some(-100) "hi""# }], "isError": false })
  );
}

#[tokio::test]
async fn an_entry_it_holds_none_of_another_kind_of_request_or_another_protocol_is_a_failed_call() {
  let refused = |asked: Value| async move { answer(ENTRIES, &handed(asked)).await.unwrap() };

  assert!(
    refused(json!({ "kind": "call", "entry": "worker/shout" }))
      .await
      .plain()
      .contains("no entry worker/shout")
  );
  assert!(refused(json!({ "kind": "resources_list" })).await.is_error);
  assert!(refused(json!({ "kind": "call", "entry": "worker/echo", "protocol": 2 })).await.is_error);
  assert!(answer(ENTRIES, "{").await.unwrap_err().contains("does not read"));
}

#[test]
fn a_turn_reads_the_variables_a_verb_is_started_with_and_a_named_room_before_its_own() {
  let turn = Turn::of(&json!({
    "runVariables": { "BMO_TURN_CHAT": "-100", "BMO_TURN_AGENT": "agent", "BMO_RUN_NAME": " ", "BMO_WORK_ORDER_STEP": "1" }
  }));

  assert_eq!(turn.room(Some(5)), Some(5));
  assert_eq!(turn.room(None), Some(-100));
  assert_eq!(turn.agent(), Some("agent"));
  assert_eq!(
    turn.verb_variables(),
    [("BMO_TURN_AGENT", "agent".to_owned()), ("BMO_WORK_ORDER_STEP", "1".to_owned())]
  );
  assert_eq!(Turn::of(&json!({})).room(None), None);
}

#[test]
fn every_way_a_manifest_differs_from_its_entries_is_named() {
  let dir = tempfile::tempdir().unwrap();
  let manifest = r#"
[tool]
name = "echoes"
protocol = 1
language = "rust"
build = "true"
check = "true"
run = ["echo-tool"]
timeout = 5

[[entry]]
namespace = "worker"
name = "shout"
description = "Shouts."
input = "shout.json"
capabilities = []
grant = "launch"
"#;

  std::fs::write(dir.path().join("tool.toml"), manifest).unwrap();
  std::fs::write(
    dir.path().join("shout.json"),
    json!({ "type": "object", "properties": { "text": {} }, "required": ["text", "loud"] }).to_string(),
  )
  .unwrap();

  let found = differences(dir.path(), "echo", ENTRIES);

  assert_eq!(
    found,
    [
      "the manifest names the tool echoes, not echo",
      "worker/echo is answered and not declared",
      "worker/shout is declared and not answered",
      "worker/shout: shout.json requires loud, which it names no property of",
    ]
  );

  std::fs::write(
    dir.path().join("shout.json"),
    json!({ "type": "object", "properties": { "text": {} } }).to_string(),
  )
  .unwrap();

  assert!(
    differences(dir.path(), "echoes", &[])
      .contains(&"worker/shout acts on a grant and takes no message".to_owned())
  );
}
