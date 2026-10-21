//! The old archive's move into this store (docs/schema.md, and the migration plan's table map for
//! TODO #1923): one export directory read, this store written, nothing else touched.
//!
//! The export is the one the item's export hand wrote under
//! `workbench/worker/items/1923/exports/job/`: `manifest.json` with each source table's exact count,
//! and one NDJSON file a table. The import reads only that directory, refuses a store already holding
//! rows of its tables, and runs whole inside one transaction, so a dry run and a failed run leave the
//! store as it stood.
//!
//! The map:
//!
//! | Old table | Where it goes |
//! |---|---|
//! | `todos` | `items`, one for one |
//! | `todo_notes` | `notes`, one for one |
//! | `todo_approvals` | the item's `approvals` list, folded into the row |
//! | `todo_sources` | the source item's `merged_into`, naming the item it was raised into |
//! | `work_orders` | `work_orders`, keyed by the item |
//! | `job_questions` | `questions`, one for one |
//! | `question_talk` | `replies`, one for one |
//! | `todos.report` | `reports`, one per item that carries one |
//!
//! What the source has and the target does not:
//!
//! - An old todo's `status` is a Korean word, read into the target's state: `초안` is `draft`, `대기`
//!   is `open`, `진행 중` is `leased`, and `완료` is `done`, or `merged` where the item was folded
//!   into another. A todo held while unfinished is `held`.
//! - The old board had no queue; every item lands in one queue named `todo`, made by the import.
//! - Everything the target has no column for is kept as a label on the item: the model, the note, the
//!   agent, the approval flag and who raised it, the hold and its reason, the resume moment, the
//!   repair's item, the impact and the brief's Korean spelling.
//! - A question's `recommended`, `why` and job are not kept: the target's questions carry no labels.
//!
//! Every time is epoch seconds in the old archive and epoch milliseconds here, so each is multiplied
//! by a thousand as it is written.

use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;

use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, Sqlite, SqliteConnection, SqlitePool, Transaction};

use crate::clock::Clock;
use crate::store::Store;

/// The queue every moved item lands in. The old board kept one.
pub const QUEUE: &str = "todo";

/// One value written to the store, already in the target's shape.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
  Null,
  Int(i64),
  Text(String),
}

/// One mapped row: the target's columns and the cell each takes.
pub type Mapped = Vec<(&'static str, Cell)>;
/// The rows of one table, in the order they are written.
pub type MappedRows = Vec<Mapped>;

/// One table of the report: what the export held and what was written.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
  pub source: String,
  pub exported: u64,
  pub written: u64,
  pub note: String,
}

/// What an import run did.
#[derive(Clone, Debug, PartialEq)]
pub struct Report {
  pub dry_run: bool,
  pub snapshot: String,
  pub read_at: String,
  pub tables: Vec<Moved>,
  pub sample: Sample,
}

/// The count-and-sample compare's result.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
  pub checked: usize,
  pub failures: Vec<String>,
}

/// The export directory's manifest.
#[derive(Clone, Debug)]
pub struct Manifest {
  pub snapshot: String,
  pub read_at: String,
  pub tables: HashMap<String, (String, u64)>,
}

fn text(value: Option<&Value>) -> Option<String> {
  match value {
    Some(Value::String(said)) => Some(said.clone()),
    Some(Value::Null) | None => None,
    Some(other) => Some(other.to_string()),
  }
}

fn int(value: Option<&Value>) -> Option<i64> {
  match value {
    Some(Value::Number(number)) => number.as_i64().or_else(|| number.as_f64().map(|n| n as i64)),
    Some(Value::String(said)) => said.parse().ok(),
    _ => None,
  }
}

fn bool_int(value: Option<&Value>) -> i64 {
  match value {
    Some(Value::Bool(true)) => 1,
    Some(Value::Number(number)) => number.as_i64().unwrap_or(0),
    Some(Value::String(said)) if said == "1" || said.eq_ignore_ascii_case("true") => 1,
    _ => 0,
  }
}

/// Seconds to milliseconds; a null stays a null.
fn ms(value: Option<&Value>) -> Option<i64> {
  int(value).map(|seconds| seconds * 1000)
}

fn is_set(value: Option<&Value>) -> bool {
  matches!(value, Some(value) if !value.is_null())
}

fn put(map: &mut serde_json::Map<String, Value>, key: &str, value: Option<&Value>) {
  if let Some(value) = value
    && !value.is_null()
  {
    map.insert(key.to_owned(), value.clone());
  }
}

/// A label's value is a string like every label's: a number or a flag is kept as its text.
fn put_label(map: &mut serde_json::Map<String, Value>, key: &str, value: Option<&Value>) {
  if let Some(value) = value
    && !value.is_null()
  {
    let said = match value {
      Value::String(said) => said.clone(),
      other => other.to_string(),
    };
    map.insert(key.to_owned(), Value::String(said));
  }
}

/// The item's state: the old status's Korean word, and whether it was held or folded.
pub fn item_state(todo: &Value) -> &'static str {
  let status = text(todo.get("status")).unwrap_or_default();
  let merged = is_set(todo.get("merged_into"));
  if bool_int(todo.get("done")) == 1 {
    return if merged { "merged" } else { "done" };
  }
  if status.starts_with("초안") {
    return "draft";
  }
  if status == "진행 중" {
    return "leased";
  }
  if is_set(todo.get("held_at")) {
    return "held";
  }

  "open"
}

/// The labels the target has no column for.
pub fn item_labels(todo: &Value) -> String {
  let mut labels = serde_json::Map::new();
  for key in [
    "model",
    "note",
    "agent",
    "approved",
    "requester_id",
    "held_kind",
    "held_reason",
    "resume_after",
    "resume_after_deploy",
    "repairs_todo_id",
    "impact",
    "job_impact",
    "brief_korean",
    "merged_at",
    "unfinished",
    "unfinished_resolved_at",
    "unfinished_resolution",
    "approved_via",
  ] {
    put_label(&mut labels, key, todo.get(key));
  }
  Value::Object(labels).to_string()
}

/// The approvals a todo was given, as the target's list: each actor and when.
pub fn approvals(rows: &[Value]) -> String {
  let list: Vec<Value> = rows
    .iter()
    .map(|row| {
      let mut one = serde_json::Map::new();
      one.insert("actorName".to_owned(), row.get("who").cloned().unwrap_or(Value::Null));
      if let Some(at) = ms(row.get("at")) {
        one.insert("at".to_owned(), Value::from(at));
      }
      for key in ["via", "who_kind", "road", "approved"] {
        put(&mut one, key, row.get(key));
      }
      Value::Object(one)
    })
    .collect();

  Value::Array(list).to_string()
}

/// The item row the old todo becomes, in the target's columns.
pub fn item(todo: &Value, approved: &[Value], merged_into: Option<i64>) -> Vec<(&'static str, Cell)> {
  let id = int(todo.get("id")).unwrap_or(0);
  let state = item_state(todo);
  let merged = merged_into.or_else(|| int(todo.get("merged_into")));
  let changed = ["done_at", "unfinished_resolved_at", "held_at", "created_at"]
    .iter()
    .find_map(|key| ms(todo.get(*key)))
    .unwrap_or(0);
  let ended = matches!(state, "done" | "dropped" | "merged")
    .then(|| ms(todo.get("done_at")))
    .flatten();

  vec![
    ("id", Cell::Int(id)),
    ("title", Cell::Text(text(todo.get("title")).unwrap_or_default())),
    (
      "brief",
      Cell::Text(text(todo.get("brief")).or_else(|| text(todo.get("instruction"))).unwrap_or_default()),
    ),
    ("state", Cell::Text(state.to_owned())),
    ("queue_name", Cell::Text(QUEUE.to_owned())),
    ("position", Cell::Int(id)),
    ("priority", Cell::Int(int(todo.get("priority")).unwrap_or(0))),
    ("assignee_name", opt_text(todo.get("worker"))),
    ("requester_name", opt_text(todo.get("requester"))),
    (
      "scope",
      match int(todo.get("chat_id")) {
        Some(chat) => Cell::Text(chat.to_string()),
        None => Cell::Null,
      },
    ),
    ("labels", Cell::Text(item_labels(todo))),
    ("approvals", Cell::Text(approvals(approved))),
    ("cowork", Cell::Int(if bool_int(todo.get("cowork")) == 1 { 1 } else { 0 })),
    (
      "merged_into",
      match merged {
        Some(into) => Cell::Int(into),
        None => Cell::Null,
      },
    ),
    ("lease_id", Cell::Null),
    ("created_at", Cell::Int(ms(todo.get("created_at")).unwrap_or(0))),
    ("changed_at", Cell::Int(changed)),
    (
      "ended_at",
      match ended {
        Some(at) => Cell::Int(at),
        None => Cell::Null,
      },
    ),
  ]
}

fn opt_text(value: Option<&Value>) -> Cell {
  match text(value) {
    Some(said) => Cell::Text(said),
    None => Cell::Null,
  }
}

/// The note row an old todo note becomes.
pub fn note(row: &Value) -> Vec<(&'static str, Cell)> {
  let mut labels = serde_json::Map::new();
  for key in ["author_id", "kind", "job_id", "delivered_at", "removed_at", "removed_by"] {
    put_label(&mut labels, key, row.get(key));
  }
  vec![
    ("id", Cell::Int(int(row.get("id")).unwrap_or(0))),
    ("item_id", Cell::Int(int(row.get("todo_id")).unwrap_or(0))),
    ("text", Cell::Text(text(row.get("text")).unwrap_or_default())),
    (
      "actor_name",
      Cell::Text(
        text(row.get("author"))
          .or_else(|| text(row.get("agent")))
          .unwrap_or_else(|| "worker".to_owned()),
      ),
    ),
    ("labels", Cell::Text(Value::Object(labels).to_string())),
    ("created_at", Cell::Int(ms(row.get("added_at")).unwrap_or(0))),
  ]
}

/// The report row an item's old `report` becomes, where it carries one.
pub fn report(todo: &Value) -> Option<Vec<(&'static str, Cell)>> {
  let body = text(todo.get("report"))?;
  if body.trim().is_empty() {
    return None;
  }
  let id = int(todo.get("id")).unwrap_or(0);
  let at = ["done_at", "created_at"].iter().find_map(|key| ms(todo.get(*key))).unwrap_or(0);

  Some(vec![
    ("id", Cell::Int(id)),
    ("item_id", Cell::Int(id)),
    ("kind", Cell::Text("final".to_owned())),
    ("body", Cell::Text(body)),
    (
      "actor_name",
      Cell::Text(text(todo.get("worker")).unwrap_or_else(|| "worker".to_owned())),
    ),
    ("labels", Cell::Text("{}".to_owned())),
    ("created_at", Cell::Int(at)),
  ])
}

/// The work order row the old body becomes.
pub fn work_order(row: &Value, title: &str, changed_at: i64) -> Vec<(&'static str, Cell)> {
  vec![
    ("item_id", Cell::Int(int(row.get("todo_id")).unwrap_or(0))),
    ("purpose", Cell::Text(title.to_owned())),
    ("directive", Cell::Text(text(row.get("body")).unwrap_or_default())),
    ("steps", Cell::Text("[]".to_owned())),
    ("state", Cell::Text("done".to_owned())),
    ("changed_at", Cell::Int(changed_at)),
  ]
}

/// The question's state: answered or withdrawn once settled, paused while its clock is kept.
pub fn question_state(row: &Value) -> &'static str {
  if is_set(row.get("settled_at")) {
    let chosen = is_set(row.get("chosen"));
    let answered = text(row.get("answer")).is_some_and(|said| !said.trim().is_empty());
    return if chosen || answered { "answered" } else { "withdrawn" };
  }
  if is_set(row.get("paused_left")) {
    return "paused";
  }

  "open"
}

/// The choices an old question offered: its options, one a line.
pub fn choices(row: &Value) -> String {
  let options = text(row.get("options")).unwrap_or_default();
  let list: Vec<Value> = options
    .lines()
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .map(|line| Value::String(line.to_owned()))
    .collect();

  Value::Array(list).to_string()
}

/// The question row an old question becomes.
pub fn question(row: &Value) -> Vec<(&'static str, Cell)> {
  let paused = int(row.get("paused_left")).map(|left| left * 1000);

  vec![
    ("id", Cell::Int(int(row.get("id")).unwrap_or(0))),
    (
      "item_id",
      match int(row.get("todo_id")) {
        Some(item) => Cell::Int(item),
        None => Cell::Null,
      },
    ),
    ("asker_name", Cell::Text("bmo".to_owned())),
    (
      "scope",
      match int(row.get("chat_id")) {
        Some(chat) => Cell::Text(chat.to_string()),
        None => Cell::Null,
      },
    ),
    ("text", Cell::Text(text(row.get("question")).unwrap_or_default())),
    ("choices", Cell::Text(choices(row))),
    ("state", Cell::Text(question_state(row).to_owned())),
    (
      "due_at",
      match ms(row.get("due_at")) {
        Some(at) => Cell::Int(at),
        None => Cell::Null,
      },
    ),
    (
      "remaining_ms",
      match paused {
        Some(left) => Cell::Int(left),
        None => Cell::Null,
      },
    ),
    ("due_told", Cell::Int(0)),
    ("created_at", Cell::Int(ms(row.get("asked_at")).unwrap_or(0))),
  ]
}

/// The reply row an old question talk becomes.
pub fn reply(row: &Value) -> Vec<(&'static str, Cell)> {
  vec![
    ("id", Cell::Int(int(row.get("id")).unwrap_or(0))),
    ("question_id", Cell::Int(int(row.get("question_id")).unwrap_or(0))),
    ("choice", Cell::Null),
    ("text", Cell::Text(text(row.get("text")).unwrap_or_default())),
    ("actor_name", Cell::Text(text(row.get("by")).unwrap_or_default())),
    ("settles", Cell::Int(0)),
    ("created_at", Cell::Int(ms(row.get("said_at")).unwrap_or(0))),
  ]
}

/// The export directory's manifest.
pub fn read_manifest(dir: &Path) -> Result<Manifest, String> {
  let path = dir.join("manifest.json");
  let said = std::fs::read_to_string(&path)
    .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
  let document: Value =
    serde_json::from_str(&said).map_err(|error| format!("{} is not JSON: {error}", path.display()))?;
  let mut tables = HashMap::new();
  let list = document.get("tables").and_then(Value::as_array).ok_or("the manifest names no tables")?;
  for table in list {
    let source = table
      .get("source")
      .and_then(Value::as_str)
      .ok_or("a manifest table names no source")?
      .to_owned();
    let file = table
      .get("file")
      .and_then(Value::as_str)
      .ok_or("a manifest table names no file")?
      .to_owned();
    let count = table.get("count").and_then(Value::as_u64).ok_or("a manifest table names no count")?;
    tables.insert(source, (file, count));
  }

  Ok(Manifest {
    snapshot: document.get("snapshot").and_then(Value::as_str).unwrap_or_default().to_owned(),
    read_at: document.get("readAt").and_then(Value::as_str).unwrap_or_default().to_owned(),
    tables,
  })
}

/// One table's rows, each checked against the manifest's count as it is read.
pub fn rows(dir: &Path, manifest: &Manifest, source: &str) -> Result<Vec<Value>, String> {
  let (file, expected) = manifest
    .tables
    .get(source)
    .ok_or_else(|| format!("the manifest has no table {source}"))?;
  let path = dir.join(file);
  let said = std::fs::read_to_string(&path)
    .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
  let mut out = Vec::new();
  for (number, line) in said.lines().enumerate() {
    if line.trim().is_empty() {
      continue;
    }
    let row: Value = serde_json::from_str(line)
      .map_err(|error| format!("{} line {} is not JSON: {error}", path.display(), number + 1))?;
    out.push(row);
  }
  if out.len() as u64 != *expected {
    return Err(format!(
      "{}: the manifest says {expected} rows and the file holds {}",
      path.display(),
      out.len()
    ));
  }

  Ok(out)
}

/// The tables this import writes, for the refusal.
pub const WRITTEN: [&str; 7] =
  ["items", "notes", "reports", "work_orders", "queues", "questions", "replies"];

/// Refuses a store that already holds rows of any table this import writes.
pub async fn refused_if_holding(pool: &SqlitePool) -> Result<(), String> {
  let mut holding = Vec::new();
  for table in WRITTEN {
    let count: i64 = sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}"))
      .fetch_one(pool)
      .await
      .map_err(|error| format!("{table} could not be counted: {error}"))?;
    if count > 0 {
      holding.push(format!("{table} holds {count}"));
    }
  }
  if holding.is_empty() {
    Ok(())
  } else {
    Err(format!(
      "the store already holds rows of its tables, so nothing was written: {}",
      holding.join(", ")
    ))
  }
}

fn placeholders(count: usize) -> String {
  (1..=count).map(|n| format!("${n}")).collect::<Vec<_>>().join(", ")
}

async fn insert(
  tx: &mut Transaction<'_, Sqlite>,
  table: &str,
  row: &[(&'static str, Cell)],
) -> Result<(), String> {
  let columns = row.iter().map(|(column, _)| *column).collect::<Vec<_>>().join(", ");
  let sql = format!("INSERT INTO {table} ({columns}) VALUES ({})", placeholders(row.len()));
  let mut query = sqlx::query(&sql);
  for (_, cell) in row {
    query = match cell {
      Cell::Null => query.bind(None::<i64>),
      Cell::Int(number) => query.bind(*number),
      Cell::Text(said) => query.bind(said.clone()),
    };
  }
  query
    .execute(&mut **tx)
    .await
    .map_err(|error| format!("{table} could not be written: {error}"))?;

  Ok(())
}

fn digest(said: &str) -> String {
  Sha256::digest(said.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The rows the compare checks: every row of a table holding twenty or fewer, else the first, the
/// last and up to twenty by the smallest SHA-256 of the key's text.
fn sample<'a>(rows: &'a [Mapped], key: &str) -> Vec<&'a Mapped> {
  if rows.len() <= 20 {
    return rows.iter().collect();
  }
  let mut ordered: Vec<(String, &Vec<(&'static str, Cell)>)> = rows
    .iter()
    .map(|row| {
      let said = row
        .iter()
        .find(|(column, _)| *column == key)
        .map(|(_, cell)| format!("{cell:?}"))
        .unwrap_or_default();
      (digest(&said), row)
    })
    .collect();
  ordered.sort_by(|left, right| left.0.cmp(&right.0));
  let mut picked: Vec<&Vec<(&'static str, Cell)>> = vec![&rows[0], &rows[rows.len() - 1]];
  for (_, row) in ordered.into_iter().take(20) {
    if !picked.iter().any(|held| std::ptr::eq(*held, row)) {
      picked.push(row);
    }
  }

  picked
}

fn target_table(source: &str) -> &str {
  match source {
    "todos" => "items",
    "todo_notes" => "notes",
    "work_orders" => "work_orders",
    "job_questions" => "questions",
    "question_talk" => "replies",
    "todos.report" => "reports",
    other => other,
  }
}

fn key_of(source: &str) -> &str {
  if source == "work_orders" { "item_id" } else { "id" }
}

async fn compare(pool: &SqlitePool, prepared: &Prepared) -> Sample {
  let mut failures = Vec::new();
  let mut checked = 0;
  for (source, rows) in &prepared.tables {
    // A report is folded out of its item, so the export has no table of that name to count against.
    if source == "todos.report" {
      continue;
    }
    let expected = prepared.manifest.tables.get(source).map(|(_, count)| *count).unwrap_or(0);
    let table = target_table(source);
    let count: Result<i64, _> =
      sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table}")).fetch_one(pool).await;
    match count {
      Ok(held) if held as u64 == expected => {}
      Ok(held) => {
        failures.push(format!("{source}: the export holds {expected} rows and {table} holds {held}"))
      }
      Err(error) => failures.push(format!("{source}: {table} could not be counted: {error}")),
    }
    let where_key = key_of(source);
    for row in sample(rows, where_key) {
      checked += 1;
      let Some(key) = row.iter().find(|(column, _)| *column == where_key).map(|(_, cell)| cell.clone())
      else {
        failures.push(format!("{source}: the mapped row carries no {where_key}"));
        continue;
      };
      let sql = format!("SELECT * FROM {table} WHERE {where_key} = $1");
      let mut query = sqlx::query(&sql);
      query = match &key {
        Cell::Null => query.bind(None::<i64>),
        Cell::Int(number) => query.bind(*number),
        Cell::Text(said) => query.bind(said.clone()),
      };
      match query.fetch_optional(pool).await {
        Ok(Some(held)) => {
          for (column, cell) in row {
            let actual = match cell {
              Cell::Null => held.try_get::<Option<i64>, _>(*column).map(|_| Cell::Null),
              Cell::Int(_) => held
                .try_get::<i64, _>(*column)
                .map(Cell::Int)
                .or_else(|_| held.try_get::<String, _>(*column).map(Cell::Text)),
              Cell::Text(_) => held
                .try_get::<Option<String>, _>(*column)
                .map(|said| said.map_or(Cell::Null, Cell::Text)),
            };
            match actual {
              Ok(actual) if &actual == cell => {}
              Ok(actual) => failures.push(format!(
                "{source} {where_key}={key:?}: {column} was written {cell:?} but is {actual:?}"
              )),
              Err(error) => {
                failures.push(format!("{source} {where_key}={key:?}: {column} could not be read: {error}"))
              }
            }
          }
        }
        Ok(None) => failures.push(format!("{source} {where_key}={key:?}: the row is not in {table}")),
        Err(error) => {
          failures.push(format!("{source} {where_key}={key:?}: {table} could not be read: {error}"))
        }
      }
    }
  }

  Sample { checked, failures }
}

/// The export read and mapped, before anything is written: each target table's rows in the order
/// they are written, and how many rows were folded into an item.
pub struct Prepared {
  pub manifest: Manifest,
  pub tables: Vec<(String, MappedRows)>,
  pub approvals: u64,
  pub merges: u64,
}

/// Reads the export and maps every row to its target's shape.
pub fn prepare(export: &Path) -> Result<Prepared, String> {
  let manifest = read_manifest(export)?;
  let todos = rows(export, &manifest, "todos")?;
  let notes = rows(export, &manifest, "todo_notes")?;
  let votes = rows(export, &manifest, "todo_approvals")?;
  let sources = rows(export, &manifest, "todo_sources")?;
  let orders = rows(export, &manifest, "work_orders")?;
  let questions = rows(export, &manifest, "job_questions")?;
  let talks = rows(export, &manifest, "question_talk")?;

  let mut approved: HashMap<i64, Vec<Value>> = HashMap::new();
  for vote in &votes {
    let todo = int(vote.get("todo_id")).unwrap_or(0);
    approved.entry(todo).or_default().push(vote.clone());
  }
  let mut merged: HashMap<i64, i64> = HashMap::new();
  for source in &sources {
    if let (Some(todo), Some(was)) = (int(source.get("todo_id")), int(source.get("source_todo_id"))) {
      merged.insert(was, todo);
    }
  }
  let titles: HashMap<i64, String> = todos
    .iter()
    .map(|todo| (int(todo.get("id")).unwrap_or(0), text(todo.get("title")).unwrap_or_default()))
    .collect();

  let mut item_rows = Vec::new();
  let mut report_rows = Vec::new();
  for todo in &todos {
    let id = int(todo.get("id")).unwrap_or(0);
    item_rows.push(item(
      todo,
      approved.get(&id).map(Vec::as_slice).unwrap_or(&[]),
      merged.get(&id).copied(),
    ));
    if let Some(report) = report(todo) {
      report_rows.push(report);
    }
  }
  let note_rows: Vec<_> = notes.iter().map(note).collect();
  let order_rows: Vec<_> = orders
    .iter()
    .map(|row| {
      let item_id = int(row.get("todo_id")).unwrap_or(0);
      let changed = todos
        .iter()
        .find(|todo| int(todo.get("id")) == Some(item_id))
        .and_then(|todo| ms(todo.get("created_at")))
        .unwrap_or(0);
      work_order(row, titles.get(&item_id).map(String::as_str).unwrap_or(""), changed)
    })
    .collect();
  let question_rows: Vec<_> = questions.iter().map(question).collect();
  let reply_rows: Vec<_> = talks.iter().map(reply).collect();

  Ok(Prepared {
    manifest,
    tables: vec![
      ("todos".to_owned(), item_rows),
      ("todos.report".to_owned(), report_rows),
      ("todo_notes".to_owned(), note_rows),
      ("work_orders".to_owned(), order_rows),
      ("job_questions".to_owned(), question_rows),
      ("question_talk".to_owned(), reply_rows),
    ],
    approvals: votes.len() as u64,
    merges: merged.len() as u64,
  })
}

/// Runs the import: reads the export, refuses a store holding rows, writes every row, and compares
/// what it wrote. A dry run rolls the transaction back and reports what would have been written.
pub async fn run(database: &str, export: &Path, dry_run: bool) -> Result<Report, String> {
  let prepared = prepare(export)?;
  let options = SqliteConnectOptions::from_str(database)
    .map_err(|error| format!("{database}: {error}"))?
    .create_if_missing(true)
    .journal_mode(SqliteJournalMode::Wal);
  let pool = SqlitePoolOptions::new()
    .max_connections(1)
    .connect_with(options)
    .await
    .map_err(|error| format!("{database} could not be opened: {error}"))?;
  // The repository's own schema, made where the store has none.
  Store::open(database, Clock::System).await.map_err(|refusal| refusal.sentence)?;
  refused_if_holding(&pool).await?;

  let mut tx = pool
    .begin()
    .await
    .map_err(|error| format!("the transaction could not be opened: {error}"))?;
  for (source, rows) in &prepared.tables {
    if source == "todo_approvals" || source == "todo_sources" {
      continue;
    }
    for row in rows {
      insert(&mut tx, target_table(source), row).await?;
    }
  }

  // The one queue every moved item lands in.
  insert(
    &mut tx,
    "queues",
    &[
      ("name", Cell::Text(QUEUE.to_owned())),
      ("state", Cell::Text("open".to_owned())),
      ("holds", Cell::Text("[]".to_owned())),
      ("changed_at", Cell::Int(0)),
    ],
  )
  .await?;

  if dry_run {
    tx.rollback()
      .await
      .map_err(|error| format!("the dry run could not be rolled back: {error}"))?;
  } else {
    tx.commit()
      .await
      .map_err(|error| format!("the import could not be committed: {error}"))?;
  }

  let sample = if dry_run {
    Sample {
      checked: 0,
      failures: Vec::new(),
    }
  } else {
    compare(&pool, &prepared).await
  };

  let exported = |source: &str| prepared.manifest.tables.get(source).map(|(_, count)| *count).unwrap_or(0);
  let written = |source: &str| {
    prepared
      .tables
      .iter()
      .find(|(held, _)| held == source)
      .map(|(_, rows)| rows.len() as u64)
      .unwrap_or(0)
  };
  let tables = vec![
    Moved {
      source: "todos".to_owned(),
      exported: exported("todos"),
      written: written("todos"),
      note: "items; approvals folded into the row".to_owned(),
    },
    Moved {
      source: "todo_notes".to_owned(),
      exported: exported("todo_notes"),
      written: written("todo_notes"),
      note: "notes".to_owned(),
    },
    Moved {
      source: "todo_approvals".to_owned(),
      exported: exported("todo_approvals"),
      written: prepared.approvals,
      note: "folded into items.approvals".to_owned(),
    },
    Moved {
      source: "todo_sources".to_owned(),
      exported: exported("todo_sources"),
      written: prepared.merges,
      note: "folded into the source item's merged_into".to_owned(),
    },
    Moved {
      source: "work_orders".to_owned(),
      exported: exported("work_orders"),
      written: written("work_orders"),
      note: "work_orders".to_owned(),
    },
    Moved {
      source: "job_questions".to_owned(),
      exported: exported("job_questions"),
      written: written("job_questions"),
      note: "questions".to_owned(),
    },
    Moved {
      source: "question_talk".to_owned(),
      exported: exported("question_talk"),
      written: written("question_talk"),
      note: "replies".to_owned(),
    },
    Moved {
      source: "todos.report".to_owned(),
      exported: exported("todos"),
      written: written("todos.report"),
      note: "reports; one per item carrying a report".to_owned(),
    },
  ];
  pool.close().await;

  Ok(Report {
    dry_run,
    snapshot: prepared.manifest.snapshot.clone(),
    read_at: prepared.manifest.read_at.clone(),
    tables,
    sample,
  })
}

/// The count-and-sample compare alone, against a store the import wrote.
pub async fn verify(database: &str, export: &Path) -> Result<Sample, String> {
  let prepared = prepare(export)?;
  let pool = SqlitePoolOptions::new()
    .max_connections(1)
    .connect(database)
    .await
    .map_err(|error| format!("{database} could not be opened: {error}"))?;
  let sample = compare(&pool, &prepared).await;
  pool.close().await;

  Ok(sample)
}

/// A connection of the import's own, for a caller that only wants to check a store the import wrote.
pub async fn connect(database: &str) -> Result<SqliteConnection, String> {
  let options = SqliteConnectOptions::from_str(database)
    .map_err(|error| format!("{database}: {error}"))?
    .create_if_missing(false)
    .journal_mode(SqliteJournalMode::Wal);

  sqlx::Connection::connect_with(&options)
    .await
    .map_err(|error| format!("{database} could not be opened: {error}"))
}
