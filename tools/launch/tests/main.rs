//! The launch tool's manifest and how it reads a job's title and a search's question.

use std::path::Path;

use launch_tool::{ENTRIES, SearchArgs, key_of, options_of, question_of, title_of};

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
fn a_search_depth_names_its_research_key() {
  assert_eq!(key_of(None).unwrap(), "search:basic");
  assert_eq!(key_of(Some("thorough")).unwrap(), "search:thorough");
  assert_eq!(key_of(Some("exhaustive")).unwrap(), "search:exhaustive");
  assert!(key_of(Some("deep")).is_err());
}

#[test]
fn a_jobs_title_is_the_first_line_of_its_prompt() {
  assert_eq!(title_of("\n  Repair the queue\nand its checks"), "Repair the queue");
}

#[test]
fn a_search_question_is_its_words_or_the_page_it_brings_in() {
  let question = |query: Option<&str>, import: Option<&str>| {
    question_of(&SearchArgs {
      query: query.map(str::to_owned),
      import: import.map(str::to_owned),
      ..Default::default()
    })
  };

  assert_eq!(question(Some("what is this"), None).unwrap(), "what is this");
  assert_eq!(
    question(None, Some("https://example.test/page")).unwrap(),
    "Bring the page at https://example.test/page in whole."
  );
  assert!(question(None, None).is_err());
}

#[test]
fn a_searches_options_carry_what_it_names() {
  let args = SearchArgs {
    shape: Some("a table".to_owned()),
    period: Some("the last year".to_owned()),
    ..Default::default()
  };

  assert_eq!(options_of(&args).as_deref(), Some("shape: a table; period: the last year"));
  assert_eq!(options_of(&SearchArgs::default()), None);
}
