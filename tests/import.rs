//! The old archive's move into the job store (src/import.rs, TODO #1923): the mapping of the old
//! words into the target's states and fields, the refusal of a store already holding rows, and the
//! count-and-sample compare, over a small export written into a temporary folder.

use std::fs;
use std::path::{Path, PathBuf};

use bmo_job::import::{self, Cell};
use serde_json::{Value, json};
use tempfile::TempDir;

/// One old todo, with every field the map reads and no more.
fn todo(id: i64, status: &str, done: i64, created: i64, extra: Value) -> Value {
  let mut row = json!({
    "id": id,
    "title": format!("todo {id}"),
    "instruction": format!("do {id}"),
    "requester": "qa-admin",
    "requester_id": 1,
    "chat_id": 90000001,
    "approved": 1,
    "done": done,
    "priority": 1,
    "status": status,
    "model": "auto",
    "note": null,
    "report": null,
    "created_at": created,
    "done_at": null,
    "held_at": null,
    "merged_into": null,
    "worker": "bmo-job",
    "cowork": 0,
    "agent": "bmo",
    "impact": null,
    "job_impact": null,
    "brief": null,
    "brief_korean": null
  });
  if let (Value::Object(base), Value::Object(over)) = (&mut row, extra) {
    for (key, value) in over {
      base.insert(key, value);
    }
  }
  row
}

/// A small export in the shape the item's export hand writes.
fn fixture() -> (TempDir, PathBuf) {
  let dir = tempfile::tempdir().expect("a temporary folder");
  let tables: Vec<(&str, Vec<Value>)> = vec![
    (
      "todos",
      vec![
        todo(
          1,
          "\u{C644}\u{B8CC}",
          1,
          100,
          json!({"done_at": 200, "report": "the work is done", "worker": "bmo-job"}),
        ),
        todo(
          2,
          "\u{CD08}\u{C548}: tester \u{ACB0}\u{C7AC} \u{B300}\u{AE30}",
          0,
          300,
          json!({"approved": 0}),
        ),
        todo(3, "\u{B300}\u{AE30}", 0, 400, json!({"held_at": 450})),
      ],
    ),
    (
      "todo_notes",
      vec![
        json!({"id": 1, "todo_id": 1, "author": "tester", "author_id": 1, "text": "a note", "added_at": 210, "kind": null, "delivered_at": null, "removed_at": null, "removed_by": null, "job_id": null}),
      ],
    ),
    (
      "todo_approvals",
      vec![
        json!({"id": 1, "todo_id": 1, "approved": 1, "via": "owner", "who_kind": "person", "who": "tester", "road": "chat_tool", "at": 150}),
      ],
    ),
    ("todo_sources", vec![json!({"todo_id": 2, "source_todo_id": 3})]),
    ("work_orders", vec![json!({"todo_id": 1, "body": "the order"})]),
    (
      "job_questions",
      vec![
        json!({"id": 1, "todo_id": 1, "chat_id": 90000001, "question": "which?", "options": "a\nb", "recommended": 0, "why": "because", "asked_at": 500, "due_at": 560, "chosen": 1, "answer": null, "paused_left": null, "settled_at": 550, "settled_by": "owner", "talks": 0}),
      ],
    ),
    (
      "question_talk",
      vec![json!({"id": 1, "question_id": 1, "by": "tester", "text": "a", "said_at": 540})],
    ),
  ];

  let mut manifest = Vec::new();
  for (source, rows) in &tables {
    let file = format!("{source}.ndjson");
    let mut body = String::new();
    for row in rows {
      body.push_str(&row.to_string());
      body.push('\n');
    }
    fs::write(dir.path().join(&file), body).expect("the export file");
    manifest.push(json!({
      "source": source,
      "file": file,
      "count": rows.len(),
      "columns": [],
      "key": ["id"]
    }));
  }
  fs::write(
    dir.path().join("manifest.json"),
    json!({
      "unit": "job",
      "snapshot": "1:1:",
      "readAt": "2026-10-21 17:00:00+09",
      "readOnly": true,
      "tables": manifest
    })
    .to_string(),
  )
  .expect("the manifest");

  let path = dir.path().to_path_buf();
  (dir, path)
}

fn sqlite(path: &Path) -> String {
  format!("sqlite://{}?mode=rwc", path.display())
}

#[test]
fn the_old_words_read_as_the_targets_states() {
  assert_eq!(import::item_state(&todo(1, "\u{C644}\u{B8CC}", 1, 100, json!({}))), "done");
  assert_eq!(
    import::item_state(&todo(
      2,
      "\u{CD08}\u{C548}: tester \u{ACB0}\u{C7AC} \u{B300}\u{AE30}",
      0,
      300,
      json!({})
    )),
    "draft"
  );
  assert_eq!(import::item_state(&todo(3, "\u{B300}\u{AE30}", 0, 400, json!({}))), "open");
  assert_eq!(import::item_state(&todo(4, "\u{C9C4}\u{D589} \u{C911}", 0, 500, json!({}))), "leased");
  assert_eq!(
    import::item_state(&todo(5, "\u{B300}\u{AE30}", 0, 600, json!({"held_at": 650}))),
    "held"
  );
  assert_eq!(
    import::item_state(&todo(6, "\u{C644}\u{B8CC}", 1, 700, json!({"merged_into": 1}))),
    "merged"
  );
}

#[test]
fn the_question_states_read_from_how_it_settled() {
  assert_eq!(
    import::question_state(&json!({"settled_at": 1, "chosen": 1, "answer": null})),
    "answered"
  );
  assert_eq!(
    import::question_state(&json!({"settled_at": 1, "chosen": null, "answer": null})),
    "withdrawn"
  );
  assert_eq!(import::question_state(&json!({"settled_at": null, "paused_left": 3})), "paused");
  assert_eq!(import::question_state(&json!({"settled_at": null, "paused_left": null})), "open");
}

#[test]
fn the_question_carries_its_recommendation() {
  let get = |row: &Value| {
    import::question(row)
      .into_iter()
      .find(|(column, _)| *column == "recommendation")
      .map(|(_, cell)| cell)
      .expect("a mapped column")
  };

  assert_eq!(
    get(&json!({"options": "a\nb", "recommended": 1, "why": "because"})),
    Cell::Text(json!({"answer": "b", "why": "because"}).to_string())
  );
  assert_eq!(
    get(&json!({"options": "a\nb", "recommended": 1, "why": "  "})),
    Cell::Null,
    "no reason, no recommendation"
  );
  assert_eq!(
    get(&json!({"options": "", "recommended": 0, "why": "because"})),
    Cell::Null,
    "no option, no recommendation"
  );
}

#[test]
fn the_item_carries_the_seconds_as_milliseconds_and_the_approval() {
  let votes = [json!({"who": "tester", "at": 150, "via": "owner"})];
  let row = import::item(&todo(1, "\u{C644}\u{B8CC}", 1, 100, json!({"done_at": 200})), &votes, None);
  let get = |column: &str| {
    row
      .iter()
      .find(|(held, _)| *held == column)
      .map(|(_, cell)| cell.clone())
      .expect("a mapped column")
  };

  assert_eq!(get("id"), Cell::Int(1));
  assert_eq!(get("state"), Cell::Text("done".to_owned()));
  assert_eq!(get("queue_name"), Cell::Text(import::QUEUE.to_owned()));
  assert_eq!(get("created_at"), Cell::Int(100_000));
  assert_eq!(get("changed_at"), Cell::Int(200_000));
  assert_eq!(get("ended_at"), Cell::Int(200_000));
  assert_eq!(get("scope"), Cell::Text("90000001".to_owned()));
  let Cell::Text(approvals) = get("approvals") else {
    panic!("approvals are JSON text");
  };
  let approvals: Value = serde_json::from_str(&approvals).expect("approvals JSON");
  assert_eq!(approvals[0]["actorName"], json!("tester"));
  assert_eq!(approvals[0]["at"], json!(150_000));
  let Cell::Text(labels) = get("labels") else {
    panic!("labels are JSON text");
  };
  let labels: Value = serde_json::from_str(&labels).expect("labels JSON");
  assert_eq!(labels["approved"], json!("1"));
}

#[tokio::test]
async fn the_import_writes_its_rows_refuses_a_second_run_and_the_compare_finds_a_changed_row() {
  let (_folder, export) = fixture();
  let store = tempfile::tempdir().expect("a temporary store");
  let database = sqlite(&store.path().join("job.db"));

  let report = import::run(&database, &export, false).await.expect("the import");
  assert_eq!(report.snapshot, "1:1:");
  assert!(report.sample.failures.is_empty(), "{:?}", report.sample.failures);
  let todos = report.tables.iter().find(|moved| moved.source == "todos").expect("todos");
  assert_eq!((todos.exported, todos.written), (3, 3));

  let again = import::run(&database, &export, false).await;
  assert!(again.is_err(), "a store holding rows is refused");
  assert!(again.unwrap_err().contains("already holds rows"));

  let passes = import::verify(&database, &export).await.expect("the compare");
  assert!(passes.failures.is_empty(), "{:?}", passes.failures);

  // A row changed behind the import's back is found by the sample.
  let pool = sqlx::SqlitePool::connect(&database).await.expect("the store");
  sqlx::query("UPDATE items SET title = 'changed' WHERE id = 1")
    .execute(&pool)
    .await
    .expect("the change");
  pool.close().await;
  let changed = import::verify(&database, &export).await.expect("the compare");
  assert!(
    changed.failures.iter().any(|failure| failure.contains("title")),
    "{:?}",
    changed.failures
  );
}

#[tokio::test]
async fn a_dry_run_writes_nothing() {
  let (_folder, export) = fixture();
  let store = tempfile::tempdir().expect("a temporary store");
  let database = sqlite(&store.path().join("job.db"));

  let report = import::run(&database, &export, true).await.expect("the dry run");
  assert!(report.dry_run);
  assert!(report.sample.failures.is_empty());

  let pool = sqlx::SqlitePool::connect(&database).await.expect("the store");
  let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM items")
    .fetch_one(&pool)
    .await
    .expect("the count");
  pool.close().await;
  assert_eq!(count, 0);
}
