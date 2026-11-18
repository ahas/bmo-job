//! The queue tool's manifest and its two readings' shapes.

mod titles;

use std::path::Path;

use serde_json::json;
use todo_tool::titles::AsWritten;
use todo_tool::{ENTRIES, ListArgs, Reader, item_line, states};

fn manifest() -> toml::Table {
  let text = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tool.toml")).unwrap();

  text.parse().unwrap()
}

#[test]
fn the_manifest_declares_every_entry_the_executable_answers() {
  let manifest = manifest();
  let entries = manifest["entry"].as_array().unwrap();

  for entry in entries {
    let namespace = entry["namespace"].as_str().unwrap();
    let name = entry["name"].as_str().unwrap();

    assert!(ENTRIES.contains(&name), "{namespace}/{name} is not answered");
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
fn the_default_reading_asks_for_what_is_open() {
  assert_eq!(states(&ListArgs::default()).as_deref(), Some("draft,open,leased"));
}

#[test]
fn each_filter_asks_for_its_own_state() {
  let list = |all, pending, held, merged| {
    states(&ListArgs {
      all,
      pending,
      held,
      merged,
      chat: None,
    })
  };

  assert_eq!(list(None, Some(true), None, None).as_deref(), Some("draft"));
  assert_eq!(list(None, None, Some(true), None).as_deref(), Some("held"));
  assert_eq!(list(None, None, None, Some(true)).as_deref(), Some("merged"));
  assert_eq!(list(Some(true), Some(true), Some(true), Some(true)), None);
}

#[test]
fn an_item_reads_as_one_line() {
  let item = json!({
    "id": 12,
    "state": "open",
    "title": "Repair the queue",
    "assigneeName": "bmo-job"
  });

  assert_eq!(item_line(&item, &AsWritten), "#12 [open] Repair the queue · bmo-job");
}

#[test]
fn a_merged_item_names_the_row_it_went_into() {
  let item = json!({ "id": 3, "state": "merged", "title": "Duplicate", "mergedInto": 2 });

  assert_eq!(item_line(&item, &AsWritten), "#3 [merged] Duplicate · merged into #2");
}

#[test]
fn the_worker_namespace_alone_reads_titles_in_the_worker_s_language() {
  assert_eq!(Reader::of("worker/todo_show"), Reader::Worker);
  assert_eq!(Reader::of("worker/todo_list"), Reader::Worker);
  assert_eq!(
    Reader::of("bmo/todo_show"),
    Reader::Chat,
    "the host hands the full name, its last part alike"
  );
  assert_eq!(Reader::of("bmo/todo_list"), Reader::Chat);
}
