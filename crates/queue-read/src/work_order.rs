//! A work order (docs/reading.md, "A work order"): the draft an item is filed with, an item's order read
//! (the one stored, or the draft an item with none would start from) and the order in English lines, the
//! fewest that answer what it says and where it has got to.
//!
//! The order's structure carries every task's whole instruction and every task of every round of its
//! history, which on a long order runs to a hundred thousand characters; the structure is what a caller
//! asks for to see those. What a task gives the lines is its state, its role and the name the plan gave
//! it, which is what a caller names to skip, rerun or move it. A task no plan named gives the head of its
//! instruction instead, cut, since there is nothing else to tell it by.

use job_base::{now_ms, one};
use sea_orm::ConnectionTrait;
use serde_json::{Value, json};

use crate::queue::Read;

/// How much of an unnamed task's instruction stands in place of the title it has none of.
pub const TASK_HEAD_CHARS: usize = 60;

/// A draft seeded with the item's own words.
pub fn draft(todo_id: i64, intent: &str, instruction: &str) -> Value {
  json!({
    "todoId": todo_id, "intent": intent, "instruction": instruction, "status": "draft", "phase": "planning",
    "stage": 0, "stages": [], "tasks": [], "history": [], "control": null,
    "worker": "", "cwd": "", "chatId": 0, "model": "", "error": null,
    "updatedAt": now_ms(),
  })
}

/// An item's work order: the one the database holds, or the draft its own title and instruction would
/// start; none where there is no such item.
pub async fn read<C: ConnectionTrait>(db: &C, todo: i64) -> Read<Option<Value>> {
  let Some(item) = one(db, "SELECT title, instruction FROM todos WHERE id = $1", vec![todo.into()]).await?
  else {
    return Ok(None);
  };
  let stored: Option<String> =
    one(db, "SELECT body FROM work_orders WHERE todo_id = $1", vec![todo.into()])
      .await?
      .map(|row| row.try_get("", "body"))
      .transpose()?;

  match stored.and_then(|body| serde_json::from_str(&body).ok()) {
    Some(order) => Ok(Some(order)),
    None => Ok(Some(draft(
      todo,
      &item.try_get::<String>("", "title")?,
      &item.try_get::<String>("", "instruction")?,
    ))),
  }
}

/// Whether a value plainly is an order: an order handed back across a process boundary is asked here
/// first, and a caller falls back to the structure rather than reading a part that moved.
pub fn is_order(value: &Value) -> bool {
  let has = |key: &str, kind: fn(&Value) -> bool| value.get(key).is_some_and(kind);
  let texts = ["intent", "instruction", "status", "phase", "worker", "cwd", "model"];

  value.is_object()
    && ["todoId", "stage", "updatedAt"].iter().all(|key| has(key, Value::is_number))
    && texts.iter().all(|key| has(key, Value::is_string))
    && ["stages", "tasks", "history"].iter().all(|key| has(key, Value::is_array))
    && value.get("error").is_some_and(|error| error.is_null() || error.is_string())
}

// The whitespace laid flat and the text cut to `limit` characters, marked where it was.
fn head(text: &str, limit: usize) -> String {
  let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");

  if flat.chars().count() <= limit {
    flat
  } else {
    format!("{}\u{2026}", flat.chars().take(limit).collect::<String>())
  }
}

// A member that is there and not null.
fn field<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
  value.get(key).filter(|member| !member.is_null())
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
  value.get(key).and_then(Value::as_str).unwrap_or_default()
}

// A number as the order's own JSON writes it: a whole one without a fraction.
fn figure(value: &Value) -> String {
  match value.as_f64() {
    Some(number) if number.fract() == 0.0 && number.abs() < 9e15 => format!("{}", number as i64),
    Some(_) => value.to_string(),
    None => value.as_str().map_or_else(|| value.to_string(), str::to_owned),
  }
}

// The minute a time in milliseconds stands at, in UTC.
fn when(at: Option<&Value>) -> String {
  at.and_then(Value::as_f64)
    .and_then(|at| chrono::DateTime::from_timestamp_millis(at as i64))
    .map(|at| at.format("%Y-%m-%d %H:%M").to_string())
    .unwrap_or_default()
}

// A task named by the plan's name for it, or the head of its instruction.
fn task_name(task: &Value) -> String {
  field(task, "title")
    .and_then(Value::as_str)
    .map_or_else(|| head(text(task, "instruction"), TASK_HEAD_CHARS), str::to_owned)
}

fn task_line(task: &Value, at: usize) -> String {
  let state = text(task, "state");
  let attempt = match task.get("attempt").and_then(Value::as_f64) {
    Some(attempt) if attempt > 1.0 => format!(" (attempt {})", figure(&task["attempt"])),
    _ => String::new(),
  };
  // Why a task was passed over is part of what the order says: a stage read without it looks like work
  // that was never reached rather than work decided against.
  let why = field(task, "skip")
    .map_or_else(String::new, |skip| format!(": {}", head(text(skip, "why"), TASK_HEAD_CHARS)));
  let failed = match field(task, "error") {
    Some(error) if state == "failed" => {
      format!(": {}", head(error.as_str().unwrap_or_default(), TASK_HEAD_CHARS))
    }
    _ => String::new(),
  };

  format!("  {at}. {state}  {}  {}{attempt}{why}{failed}", text(task, "role"), task_name(task))
}

fn dash(text: &str) -> &str {
  if text.is_empty() { "-" } else { text }
}

fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
  value.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// The order in lines, the owner named as the deployment names them. Nothing here reads the database or
/// the clock: the same order reads the same way wherever it is printed.
pub fn lines(order: &Value, owner: &str) -> Vec<String> {
  let mut out = Vec::new();
  let round =
    field(order, "round").map_or_else(String::new, |round| format!(" \u{b7} round {}", figure(round)));
  let stages = list(order, "stages");
  let stage = order.get("stage").and_then(Value::as_f64).unwrap_or_default();
  // The stage counter stands only while the order is inside its stages. An order whose build list is
  // running has a stage number past the last stage, since the list belongs to no stage; the phase already
  // says where such an order has got to.
  let counter = if !stages.is_empty() && stage < stages.len() as f64 {
    format!(" {}/{}", stage as i64 + 1, stages.len())
  } else {
    String::new()
  };

  out.push(format!(
    "#{} {} \u{b7} {}{counter}{round}",
    figure(order.get("todoId").unwrap_or(&Value::Null)),
    text(order, "status"),
    text(order, "phase")
  ));
  out.push(format!(
    "worker: {} \u{b7} model: {} \u{b7} cwd: {}",
    dash(text(order, "worker")),
    dash(text(order, "model")),
    dash(text(order, "cwd"))
  ));
  out.push(format!("intent: {}", text(order, "intent")));
  out.push(format!("instruction: {}", text(order, "instruction")));

  if let Some(error) = field(order, "error") {
    out.push(format!("error: {}", error.as_str().map_or_else(|| error.to_string(), str::to_owned)));
  }

  // The stages as the plan wrote them, and then the build steps, which belong to no stage, under a
  // heading of their own rather than left out.
  for (index, stage) in stages.iter().enumerate() {
    out.push(format!("{}. {}", index + 1, text(stage, "title")));
    out.extend(list(stage, "tasks").iter().enumerate().map(|(at, task)| task_line(task, at + 1)));
  }

  let staged: Vec<&Value> = stages
    .iter()
    .flat_map(|stage| list(stage, "tasks"))
    .filter_map(|task| task.get("id"))
    .collect();
  let loose: Vec<&Value> = list(order, "tasks")
    .iter()
    .filter(|task| task.get("id").is_none_or(|id| !staged.contains(&id)))
    .collect();

  if !loose.is_empty() {
    out.push("build steps:".to_owned());
    out.extend(loose.iter().enumerate().map(|(at, task)| task_line(task, at + 1)));
  }

  if let Some(control) = field(order, "control") {
    out.push(format!(
      "control: {}  {}  {}",
      text(control, "state"),
      text(control, "role"),
      task_name(control)
    ));
  }

  // What the history and the earlier rounds hold is said as a count: a caller who wants one of them is
  // asking about that round, and the structure is where that answer is.
  let history = list(order, "history");

  if !history.is_empty() {
    out.push(format!("history: {} tasks", history.len()));
  }

  let earlier = list(order, "earlier");

  if !earlier.is_empty() {
    let rounds: Vec<String> =
      earlier.iter().map(|one| figure(one.get("round").unwrap_or(&Value::Null))).collect();

    out.push(format!("earlier rounds: {}", rounds.join(", ")));
  }

  // The facts about the checkout and the stop, each only where it is there: these decide whether an order
  // can be resumed at all.
  if order.get("holdsCheckout").and_then(Value::as_bool) == Some(true) {
    let wait = field(order, "releaseWait")
      .map_or_else(String::new, |wait| format!(" (waiting on {})", figure(wait)));

    out.push(format!("checkout: held by this order{wait}"));
  }

  let conflicts: Vec<String> = list(order, "conflicts").iter().map(figure).collect();

  if !conflicts.is_empty() {
    out.push(format!("conflicts: {}", conflicts.join(", ")));
  }

  if let Some(parked) = field(order, "parked") {
    out.push(format!("parked: {parked}"));
  }

  if let Some(stop) = field(order, "ownerStop") {
    out.push(format!("stopped by {owner}: {} ({})", text(stop, "why"), when(stop.get("at"))));
  }

  if let Some(closing) = field(order, "closing") {
    out.push(format!("winding down: {} ({})", text(closing, "why"), when(closing.get("at"))));
  }

  if let Some(stopped) = field(order, "stoppedAt") {
    let failures = field(order, "failedStops")
      .map_or_else(String::new, |count| format!(" \u{b7} {} stops were failures", figure(count)));

    out.push(format!("stopped: {}{failures}", when(Some(stopped))));
  }

  if let Some(place) = field(order, "queuePlace") {
    out.push(format!("queue: {place}"));
  }

  out.push(format!(
    "updated: {} \u{b7} the whole structure is --json",
    when(order.get("updatedAt"))
  ));

  out
}
