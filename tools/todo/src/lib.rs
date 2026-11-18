//! The queue's entries for the tool host, in the worker namespace and in the chat agent's `bmo`: the two
//! readings, the filing of a row, the words added to one and its approval. Each answers out of the job
//! service's own routes, with the caller token the deployment keeps for the chat agent so an approval is an
//! approver's (bmo-job's `docs/api.md`, 「Items」). The worker's readings show each title in the worker's language
//! (`titles`).

pub mod titles;

use serde::Deserialize;
use serde_json::{Value, json};

use tool_call::{
  Answer, Handed, Service, entry_name, gate_service, job_service, queue_name, read_handed, turn_agent,
  turn_room, write_answer,
};

use crate::titles::{AsWritten, Kept, Titles};

/// The entries this executable answers, as its manifest declares them.
pub const ENTRIES: [&str; 7] = [
  "todo_list",
  "todo_show",
  "bmo_todo_list",
  "bmo_todo_show",
  "todo_add",
  "todo_append",
  "todo_approve",
];

/// The most rows one reading asks for.
const MOST: &str = "200";

/// One reading's arguments.
#[derive(Debug, Default, Deserialize)]
pub struct ListArgs {
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

/// One item asked for.
#[derive(Debug, Deserialize)]
pub struct ShowArgs {
  pub item: String,
}

/// A row filed out of the conversation.
#[derive(Debug, Deserialize)]
pub struct AddArgs {
  pub title: String,
  #[serde(default)]
  pub what: Option<String>,
  #[serde(default)]
  pub r#do: Option<String>,
  #[serde(default)]
  pub conclusion: Option<String>,
  #[serde(default)]
  pub chat: Option<i64>,
  #[serde(default)]
  pub requester: Option<String>,
  #[serde(default)]
  pub model: Option<String>,
}

/// Words added to a row.
#[derive(Debug, Deserialize)]
pub struct AppendArgs {
  pub item: String,
  pub text: String,
  #[serde(default)]
  pub by: Option<String>,
}

/// An approval, or its taking back.
#[derive(Debug, Deserialize)]
pub struct ApproveArgs {
  pub item: String,
  #[serde(default)]
  pub note: Option<String>,
  #[serde(default)]
  pub unapprove: bool,
}

/// The states a row in the queue is read by, in the order its filters name them.
pub fn states(args: &ListArgs) -> Option<String> {
  if args.all == Some(true) {
    None
  } else if args.pending == Some(true) {
    Some("draft".to_owned())
  } else if args.held == Some(true) {
    Some("held".to_owned())
  } else if args.merged == Some(true) {
    Some("merged".to_owned())
  } else {
    // What is open: not finished, not set aside, not merged away.
    Some("draft,open,leased".to_owned())
  }
}

fn text(item: &Value, key: &str) -> Option<String> {
  item.get(key).and_then(Value::as_str).map(str::to_owned).filter(|text| !text.is_empty())
}

/// One item as one line, its title shown as `titles` shows it.
pub fn item_line(item: &Value, titles: &dyn Titles) -> String {
  let id = item.get("id").and_then(Value::as_i64).unwrap_or(0);
  let state = text(item, "state").unwrap_or_default();
  let title = titles.shown(&text(item, "title").unwrap_or_default());
  let mut line = format!("#{id} [{state}] {title}");

  if let Some(assignee) = text(item, "assigneeName") {
    line.push_str(&format!(" · {assignee}"));
  }

  if let Some(merged) = item.get("mergedInto").and_then(Value::as_i64) {
    line.push_str(&format!(" · merged into #{merged}"));
  }

  line
}

/// The item a reading names: its number where what was said is one, else the first unfinished row of that
/// title and, where none is, the first row of it.
pub async fn resolve(service: &Service, said: &str) -> Result<Value, String> {
  let trimmed = said.trim();

  if let Ok(id) = trimmed.parse::<i64>() {
    return tool_call::get(service, &format!("/items/{id}"), &[]).await;
  }

  let found =
    tool_call::get(service, "/items", &[("q", trimmed.to_owned()), ("limit", "50".to_owned())]).await?;
  let items = found.get("items").and_then(Value::as_array).cloned().unwrap_or_default();
  let unfinished =
    |item: &&Value| !matches!(text(item, "state").as_deref(), Some("done" | "dropped" | "merged"));

  items
    .iter()
    .find(unfinished)
    .or_else(|| items.first())
    .cloned()
    .ok_or_else(|| format!("there is no item {said:?} in the queue"))
}

fn joined(parts: &[Option<String>]) -> String {
  parts
    .iter()
    .flatten()
    .map(|part| part.trim())
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join("\n\n")
}

/// The two readings, the filing, the words added and the approval.
pub async fn answer(handed: &Handed) -> Answer {
  let name = entry_name(&handed.entry);
  if !ENTRIES.contains(&name) {
    return Answer::refusal(format!("this executable holds no entry {}", handed.entry));
  }

  let service = match job_service(&handed.session) {
    Ok(service) => service,
    Err(why) => return Answer::refusal(why),
  };

  match name {
    "todo_list" | "bmo_todo_list" => {
      list(&service, &handed.arguments, handed, Reader::of(&handed.entry)).await
    }
    "todo_show" | "bmo_todo_show" => {
      show(&service, &handed.arguments, handed, Reader::of(&handed.entry)).await
    }
    "todo_add" => add(&service, &handed.arguments, handed).await,
    "todo_append" => append(&service, &handed.arguments, handed).await,
    "todo_approve" => approve(&service, &handed.arguments).await,
    other => Answer::refusal(format!("this executable holds no entry {other}")),
  }
}

/// Whose reading an entry answers, which decides how its titles are shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reader {
  /// The worker's, in the worker's language.
  Worker,
  /// The chat agent's, as a person filed them.
  Chat,
}

/// The namespace the worker's readings stand under.
pub const WORKER_NAMESPACE: &str = "worker";

impl Reader {
  /// The reader of an entry by the namespace of its full name, `<namespace>/<name>`, as the host hands it: the
  /// worker's under the worker namespace, and the chat agent's under any other or none.
  pub fn of(entry: &str) -> Self {
    match entry.split_once('/') {
      Some((WORKER_NAMESPACE, _)) => Self::Worker,
      _ => Self::Chat,
    }
  }

  // How this reader is shown the titles of `items`.
  async fn titles(self, handed: &Handed, items: &[&Value]) -> Box<dyn Titles> {
    match self {
      Self::Chat => Box::new(AsWritten),
      Self::Worker => {
        let written: Vec<String> = items.iter().filter_map(|item| text(item, "title")).collect();
        let written: Vec<&str> = written.iter().map(String::as_str).collect();

        Box::new(Kept::read(gate_service(&handed.session).as_ref(), &written).await)
      }
    }
  }
}

async fn list(service: &Service, arguments: &Value, handed: &Handed, reader: Reader) -> Answer {
  let args: ListArgs = match serde_json::from_value(arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("todo_list: {why}")),
  };
  let mut query: Vec<(&str, String)> = vec![("limit", MOST.to_owned())];

  if let Some(states) = states(&args) {
    query.push(("state", states));
  }

  let chat = args.chat.or_else(|| turn_room(&handed.session));
  if let Some(chat) = chat {
    query.push(("scope", chat.to_string()));
  }

  match tool_call::get(service, "/items", &query).await {
    Ok(page) => {
      let items: Vec<&Value> = page.get("items").and_then(Value::as_array).into_iter().flatten().collect();
      let titles = reader.titles(handed, &items).await;

      Answer::lines(items.iter().map(|item| item_line(item, titles.as_ref())).collect())
    }
    Err(why) => Answer::refusal(why),
  }
}

async fn show(service: &Service, arguments: &Value, handed: &Handed, reader: Reader) -> Answer {
  let args: ShowArgs = match serde_json::from_value(arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("todo_show: {why}")),
  };
  let item = match resolve(service, &args.item).await {
    Ok(item) => item,
    Err(why) => return Answer::refusal(why),
  };
  let id = item.get("id").and_then(Value::as_i64).unwrap_or(0);
  let titles = reader.titles(handed, &[&item]).await;
  let mut lines = vec![format!(
    "#{id} {} [{}]",
    titles.shown(&text(&item, "title").unwrap_or_default()),
    text(&item, "state").unwrap_or_default()
  )];
  let mut facts = Vec::new();
  for key in ["queueName", "requesterName", "assigneeName", "scope"] {
    if let Some(value) = text(&item, key) {
      facts.push(format!("{key}: {value}"));
    }
  }
  if !facts.is_empty() {
    lines.push(facts.join(" · "));
  }
  if let Some(brief) = text(&item, "brief") {
    lines.push(String::new());
    lines.push(brief);
  }

  let notes = tool_call::get(service, &format!("/items/{id}/notes"), &[("limit", MOST.to_owned())]).await;
  if let Ok(page) = notes
    && let Some(items) = page.get("items").and_then(Value::as_array)
    && !items.is_empty()
  {
    lines.push(String::new());
    lines.push("Notes".to_owned());
    for note in items {
      lines.push(format!(
        "- {}: {}",
        text(note, "actorName").unwrap_or_default(),
        text(note, "text").unwrap_or_default()
      ));
    }
  }

  let questions =
    tool_call::get(service, "/questions", &[("itemId", id.to_string()), ("limit", MOST.to_owned())]).await;
  if let Ok(page) = questions
    && let Some(items) = page.get("items").and_then(Value::as_array)
    && !items.is_empty()
  {
    lines.push(String::new());
    lines.push("Questions".to_owned());
    for question in items {
      lines.push(question_line(service, question).await);
    }
  }

  let reports =
    tool_call::get(service, &format!("/items/{id}/reports"), &[("limit", MOST.to_owned())]).await;
  if let Ok(page) = reports
    && let Some(items) = page.get("items").and_then(Value::as_array)
    && !items.is_empty()
  {
    lines.push(String::new());
    lines.push("Reports".to_owned());
    for report in items {
      lines.push(format!(
        "- {} ({}): {}",
        text(report, "actorName").unwrap_or_default(),
        text(report, "kind").unwrap_or_default(),
        text(report, "body").unwrap_or_default()
      ));
    }
  }

  Answer::lines(lines)
}

// One question of an item as a line: its number, its state, its words and, where a reply settled it, that answer.
async fn question_line(service: &Service, question: &Value) -> String {
  let id = question.get("id").and_then(Value::as_i64).unwrap_or(0);
  let state = text(question, "state").unwrap_or_default();
  let mut line = format!("- #{id} [{state}] {}", text(question, "text").unwrap_or_default());

  if state == "answered"
    && let Ok(page) =
      tool_call::get(service, &format!("/questions/{id}/replies"), &[("limit", MOST.to_owned())]).await
    && let Some(settling) = page
      .get("items")
      .and_then(Value::as_array)
      .and_then(|replies| replies.iter().find(|reply| reply.get("settles") == Some(&Value::Bool(true))))
  {
    let answer = text(settling, "choice").or_else(|| text(settling, "text")).unwrap_or_default();

    line.push_str(&format!(" → {answer}"));
  }

  line
}

async fn add(service: &Service, arguments: &Value, handed: &Handed) -> Answer {
  let args: AddArgs = match serde_json::from_value(arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("todo_add: {why}")),
  };
  let brief = joined(&[args.what, args.r#do, args.conclusion]);
  let scope = args.chat.or_else(|| turn_room(&handed.session)).map(|chat| chat.to_string());
  let requester = args.requester.or_else(|| turn_agent(&handed.session));
  let mut labels = serde_json::Map::new();
  if let Some(model) = args.model {
    labels.insert("model".to_owned(), json!(model));
  }
  let body = json!({
    "title": args.title,
    "brief": brief,
    "queueName": queue_name(&handed.session),
    "requesterName": requester,
    "scope": scope,
    "labels": labels,
  });

  match tool_call::post(service, "/items", &body).await {
    Ok(item) => {
      let id = item.get("id").and_then(Value::as_i64).unwrap_or(0);
      let state = text(&item, "state").unwrap_or_default();
      Answer::structured(json!({ "id": id, "state": state }))
    }
    Err(why) => Answer::refusal(why),
  }
}

async fn append(service: &Service, arguments: &Value, handed: &Handed) -> Answer {
  let args: AppendArgs = match serde_json::from_value(arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("todo_append: {why}")),
  };
  let item = match resolve(service, &args.item).await {
    Ok(item) => item,
    Err(why) => return Answer::refusal(why),
  };
  let id = item.get("id").and_then(Value::as_i64).unwrap_or(0);
  let actor = args
    .by
    .or_else(|| turn_agent(&handed.session))
    .unwrap_or_else(|| "agent-session".to_owned());
  let body = json!({ "text": args.text, "actorName": actor });

  match tool_call::post(service, &format!("/items/{id}/notes"), &body).await {
    Ok(_) => Answer::text(format!("Added the words to item #{id}.")),
    Err(why) => Answer::refusal(why),
  }
}

async fn approve(service: &Service, arguments: &Value) -> Answer {
  let args: ApproveArgs = match serde_json::from_value(arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("todo_approve: {why}")),
  };
  if args.unapprove {
    return Answer::refusal(
      "the job service records an approval and moves nothing back: it keeps no route that takes one away",
    );
  }
  let item = match resolve(service, &args.item).await {
    Ok(item) => item,
    Err(why) => return Answer::refusal(why),
  };
  let id = item.get("id").and_then(Value::as_i64).unwrap_or(0);

  match tool_call::patch(service, &format!("/items/{id}"), &json!({ "approval": {} })).await {
    Ok(moved) => {
      if let Some(note) = args.note.filter(|note| !note.trim().is_empty()) {
        let _ = tool_call::post(
          service,
          &format!("/items/{id}/notes"),
          &json!({ "text": note, "actorName": "approval" }),
        )
        .await;
      }

      let state = text(&moved, "state").unwrap_or_default();
      Answer::structured(json!({ "id": id, "state": state }))
    }
    Err(why) => Answer::refusal(why),
  }
}

/// The executable's whole main.
pub async fn run() -> std::process::ExitCode {
  let handed = match read_handed() {
    Ok(handed) => handed,
    Err(why) => {
      eprintln!("{why}");
      return std::process::ExitCode::FAILURE;
    }
  };

  if handed.protocol != tool_call::PROTOCOL {
    return write_answer(&Answer::refusal(format!(
      "this tool speaks protocol {}, and was handed {}",
      tool_call::PROTOCOL,
      handed.protocol
    )));
  }

  if handed.kind != "call" {
    return write_answer(&Answer::refusal(format!("this tool answers calls alone, not {}", handed.kind)));
  }

  write_answer(&answer(&handed).await)
}
