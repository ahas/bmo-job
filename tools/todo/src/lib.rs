//! The service's TODO tool (docs/tools.md): in the `worker` namespace the queue as a worker reads it, its lines
//! at `GET /todos/worker-lines` and `GET /todos/{item}/worker-lines`; in the `bmo` namespace the queue as a person
//! reads it, at `GET /todos` and `GET /todos/{item}`, answered as the lines of the reading schema, beside the
//! three filings (`filing`), which run as the unit CLI's `todo` verb.

pub mod filing;

use futures_util::future::BoxFuture;
use job_tool_call::{Answer, Entry, Service, Turn, input};
use serde::Deserialize;
use serde_json::Value;

/// Every entry the executable answers, as its manifest declares them.
pub const ENTRIES: &[&dyn Entry] = &[
  &Called::new("worker", "todo_list", worker_list),
  &Called::new("worker", "todo_show", worker_show),
  &Called::new("bmo", "todo_list", person_list),
  &Called::new("bmo", "todo_show", person_show),
  &Called::new("bmo", "todo_add", filing::add),
  &Called::new("bmo", "todo_append", filing::append),
  &Called::new("bmo", "todo_approve", filing::approve),
];

/// What one entry's call is: the turn and the arguments in, the answer out.
pub type Call = for<'a> fn(&'a Turn, Value, &'a str) -> BoxFuture<'a, Answer>;

/// One entry, by its full name and its call.
pub struct Called {
  namespace: &'static str,
  name: &'static str,
  call: Call,
}

impl Called {
  pub const fn new(namespace: &'static str, name: &'static str, call: Call) -> Self {
    Self {
      namespace,
      name,
      call,
    }
  }
}

impl Entry for Called {
  fn namespace(&self) -> &'static str {
    self.namespace
  }

  fn name(&self) -> &'static str {
    self.name
  }

  fn call<'a>(&'a self, turn: &'a Turn, arguments: Value) -> BoxFuture<'a, Answer> {
    Box::pin(async move {
      let full = self.full_name();

      (self.call)(turn, arguments, &full).await
    })
  }
}

fn service() -> Result<Service, Answer> {
  Service::of_process().map_err(Answer::refusal)
}

/// Which rows a listing is asked for, as both namespaces name them.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListInput {
  #[serde(default)]
  pub all: Option<bool>,
  #[serde(default)]
  pub pending: Option<bool>,
  #[serde(default)]
  pub held: Option<bool>,
  #[serde(default)]
  pub merged: Option<bool>,
  #[serde(default)]
  pub chat: Option<i64>,
}

impl ListInput {
  /// The listing's query: each flag asked for, and the room.
  pub fn query(&self) -> Vec<(&'static str, String)> {
    let flags = [
      ("all", self.all),
      ("pending", self.pending),
      ("held", self.held),
      ("merged", self.merged),
    ];

    flags
      .into_iter()
      .filter(|(_, set)| *set == Some(true))
      .map(|(name, _)| (name, "true".to_owned()))
      .chain(self.chat.map(|chat| ("chat", chat.to_string())))
      .collect()
  }
}

/// One item asked for, by its number or the title of one that is not finished.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShowInput {
  pub item: String,
}

// A reading's members, each as a line: a string as it stands, anything else as its JSON.
fn members(read: Value) -> Vec<String> {
  match read {
    Value::Array(members) => members
      .into_iter()
      .map(|member| match member {
        Value::String(line) => line,
        other => other.to_string(),
      })
      .collect(),
    other => vec![other.to_string()],
  }
}

async fn read(segments: &[&str], query: &[(&str, String)]) -> Result<Vec<String>, Answer> {
  service()?.get(segments, query).await.map(members).map_err(Answer::refusal)
}

fn shown(input: &ShowInput) -> Result<&str, Answer> {
  Some(input.item.trim())
    .filter(|item| !item.is_empty())
    .ok_or_else(|| Answer::refusal("no item is named"))
}

fn worker_list<'a>(_: &'a Turn, arguments: Value, entry: &'a str) -> BoxFuture<'a, Answer> {
  Box::pin(async move {
    let asked: ListInput = match input(entry, arguments) {
      Ok(asked) => asked,
      Err(refused) => return refused,
    };

    match read(&["todos", "worker-lines"], &asked.query()).await {
      Ok(lines) => Answer::text(lines.join("\n")),
      Err(refused) => refused,
    }
  })
}

fn worker_show<'a>(_: &'a Turn, arguments: Value, entry: &'a str) -> BoxFuture<'a, Answer> {
  Box::pin(async move {
    let asked: ShowInput = match input(entry, arguments) {
      Ok(asked) => asked,
      Err(refused) => return refused,
    };
    let item = match shown(&asked) {
      Ok(item) => item,
      Err(refused) => return refused,
    };

    match read(&["todos", item, "worker-lines"], &[]).await {
      Ok(lines) => Answer::text(lines.join("\n")),
      Err(refused) => refused,
    }
  })
}

fn person_list<'a>(_: &'a Turn, arguments: Value, entry: &'a str) -> BoxFuture<'a, Answer> {
  Box::pin(async move {
    let asked: ListInput = match input(entry, arguments) {
      Ok(asked) => asked,
      Err(refused) => return refused,
    };

    match read(&["todos"], &asked.query()).await {
      Ok(lines) => Answer::reading(lines),
      Err(refused) => refused,
    }
  })
}

fn person_show<'a>(_: &'a Turn, arguments: Value, entry: &'a str) -> BoxFuture<'a, Answer> {
  Box::pin(async move {
    let asked: ShowInput = match input(entry, arguments) {
      Ok(asked) => asked,
      Err(refused) => return refused,
    };
    let item = match shown(&asked) {
      Ok(item) => item,
      Err(refused) => return refused,
    };

    match read(&["todos", item], &[]).await {
      Ok(lines) => Answer::reading(lines),
      Err(refused) => refused,
    }
  })
}
