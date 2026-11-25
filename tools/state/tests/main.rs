//! The state tool's manifest and the lines its readings answer.

use std::path::Path;

use serde_json::json;
use state_tool::{ENTRIES, message_line, run_line, task_line};

fn manifest() -> toml::Table {
  let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tool.toml")).unwrap();

  text.parse().unwrap()
}

#[test]
fn the_manifest_declares_every_entry_the_executable_answers() {
  let manifest = manifest();
  let entries = manifest["entry"].as_array().unwrap();

  for entry in entries {
    let name = entry["name"].as_str().unwrap();

    assert!(ENTRIES.contains(&name), "{name} is not answered");
  }

  assert_eq!(entries.len(), ENTRIES.len());
}

#[test]
fn every_schema_the_manifest_names_is_an_object() {
  let manifest = manifest();
  let dir = Path::new(env!("CARGO_MANIFEST_DIR"));

  for entry in manifest["entry"].as_array().unwrap() {
    for key in ["input", "output"] {
      let Some(file) = entry.get(key).and_then(|value| value.as_str()) else {
        continue;
      };
      let text = std::fs::read_to_string(dir.join(file)).unwrap_or_else(|why| panic!("{file}: {why}"));
      let schema: serde_json::Value = serde_json::from_str(&text).unwrap();

      assert!(schema.is_object(), "{file} is no schema object");
    }
  }
}

#[test]
fn a_job_reads_as_one_line() {
  let job = json!({
    "id": 7,
    "state": "running",
    "itemId": 42,
    "workerName": "bmo-job",
    "roleName": "worker",
    "roomId": 9
  });

  assert_eq!(run_line(&job, false), "#7 [running] item #42 · bmo-job · worker · room 9");
}

#[test]
fn a_job_read_whole_carries_its_report() {
  let job = json!({ "id": 7, "state": "ended", "itemId": 42, "outcome": "answered", "report": "done" });

  assert_eq!(run_line(&job, true), "#7 [ended] item #42 · answered\n  done");
}

#[test]
fn a_task_reads_as_one_line() {
  let task = json!({
    "id": 11,
    "itemId": 42,
    "stage": 2,
    "title": "Mend the reading",
    "state": "waiting",
    "assigneeName": "bmo-job",
    "after": null
  });

  assert_eq!(task_line(&task), "  2. [waiting] Mend the reading (bmo-job)");
}

/// A step that waits on another item's work says which item, and the repository its commit must stand on.
#[test]
fn a_task_that_waits_names_the_item_and_its_repository() {
  let task = json!({
    "id": 12,
    "itemId": 42,
    "stage": 3,
    "title": "Deploy the reading",
    "state": "waiting",
    "after": { "itemId": 41, "repository": "web" }
  });

  assert_eq!(task_line(&task), "  3. [waiting] Deploy the reading · waits on item #41 in web");
}

#[test]
fn a_message_reads_as_one_line() {
  let message =
    json!({ "id": 5, "senderName": "a person", "sentAt": 1700000000000_i64, "text": "hello\nthere" });

  assert_eq!(message_line(&message), "#5 a person 1700000000000: hello there");
}
