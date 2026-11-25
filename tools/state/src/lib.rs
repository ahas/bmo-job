//! The chat agent's state readings for the tool host (the chat agent's `bmo` namespace): what runs and what
//! waits on this machine, a room's job runs, one item's work order and a room's archived messages. The
//! running jobs and the room's runs are the agency's, the work order is the job service's, and the archive is
//! the receiver's, each read over its own documented API.

use serde::Deserialize;
use serde_json::Value;

use tool_call::{
  Answer, Handed, entry_name, job_service, read_handed, receiver_service, turn_room, write_answer,
};

/// The entries this executable answers, as its manifest declares them.
pub const ENTRIES: [&str; 4] = ["running", "jobs", "work_order", "archive_scan"];

/// What the two run readings take.
#[derive(Debug, Default, Deserialize)]
pub struct RunsArgs {
  #[serde(default)]
  pub chat: Option<i64>,
  #[serde(default)]
  pub limit: Option<i64>,
  /// A job's report whole.
  #[serde(default)]
  pub full: bool,
}

/// One work order asked for.
#[derive(Debug, Deserialize)]
pub struct WorkOrderArgs {
  pub item: i64,
}

/// A room's archive read.
#[derive(Debug, Default, Deserialize)]
pub struct ScanArgs {
  #[serde(default)]
  pub chat: Option<i64>,
  #[serde(default)]
  pub id: Option<i64>,
  #[serde(default)]
  pub limit: Option<i64>,
  #[serde(default)]
  pub before_id: Option<i64>,
  #[serde(default)]
  pub contains: Option<String>,
  #[serde(default)]
  pub sender: Option<i64>,
}

fn text(value: &Value, key: &str) -> Option<String> {
  value
    .get(key)
    .and_then(Value::as_str)
    .map(str::to_owned)
    .filter(|text| !text.is_empty())
}

fn number(value: &Value, key: &str) -> Option<i64> {
  value.get(key).and_then(Value::as_i64)
}

/// A job or a run as one line.
pub fn run_line(job: &Value, full: bool) -> String {
  let id = number(job, "id").unwrap_or(0);
  let state = text(job, "state").unwrap_or_default();
  let item = number(job, "itemId").map(|item| format!("item #{item}")).unwrap_or_default();
  let worker = text(job, "workerName").or_else(|| text(job, "runName")).unwrap_or_default();
  let role = text(job, "roleName").unwrap_or_default();
  let room = number(job, "roomId").map(|room| format!("room {room}")).unwrap_or_default();
  let mut one = format!("#{id} [{state}] {item}");
  for part in [worker, role, room] {
    if !part.is_empty() {
      one.push_str(&format!(" · {part}"));
    }
  }

  if let Some(outcome) = text(job, "outcome") {
    one.push_str(&format!(" · {outcome}"));
  }

  if full && let Some(report) = text(job, "report") {
    one.push_str(&format!("\n  {}", report.replace('\n', "\n  ")));
  }

  one
}

/// One work order's task as one line: its stage, its state, its title and its assignee, and where the step
/// waits on another item's work, the item it waits on and the repository that work must stand on.
pub fn task_line(task: &Value) -> String {
  let assignee = text(task, "assigneeName").map(|name| format!(" ({name})")).unwrap_or_default();
  let waits = task
    .get("after")
    .filter(|after| after.is_object())
    .map(|after| {
      format!(
        " · waits on item #{} in {}",
        number(after, "itemId").unwrap_or(0),
        text(after, "repository").unwrap_or_default()
      )
    })
    .unwrap_or_default();

  format!(
    "  {}. [{}] {}{}{}",
    number(task, "stage").unwrap_or(0),
    text(task, "state").unwrap_or_default(),
    text(task, "title").unwrap_or_default(),
    assignee,
    waits
  )
}

/// The room a call reads: the one it names or the turn's own, or why it names none.
fn room(args: &RunsArgs, handed: &Handed) -> Result<i64, Answer> {
  args.chat.or_else(|| turn_room(&handed.session)).ok_or_else(|| {
    Answer::refusal(
      "this call names no room: give chat, the room's id, or make it in a turn that answers one",
    )
  })
}

/// The entries: the two run readings, one work order and the room's archive.
pub async fn answer(handed: &Handed) -> Answer {
  let name = entry_name(&handed.entry);
  if !ENTRIES.contains(&name) {
    return Answer::refusal(format!("this executable holds no entry {}", handed.entry));
  }

  match name {
    "running" => running(handed).await,
    "jobs" => jobs(handed).await,
    "work_order" => work_order(handed).await,
    "archive_scan" => archive_scan(handed).await,
    other => Answer::refusal(format!("this executable holds no entry {other}")),
  }
}

async fn running(handed: &Handed) -> Answer {
  let args: RunsArgs = match serde_json::from_value(handed.arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("running: {why}")),
  };
  let service = match tool_call::agency_service(&handed.session) {
    Ok(service) => service,
    Err(why) => return Answer::refusal(why),
  };
  let limit = args.limit.unwrap_or(50).clamp(1, 500).to_string();
  // What runs is the machine's, not one room's: the room's own runs are the `jobs` reading.
  let query = vec![("running", "true".to_owned()), ("limit", limit)];

  match tool_call::get(&service, "/jobs", &query).await {
    Ok(page) => {
      let lines: Vec<String> = page
        .get("items")
        .and_then(Value::as_array)
        .map(|jobs| jobs.iter().map(|job| run_line(job, args.full)).collect())
        .unwrap_or_default();

      if lines.is_empty() {
        Answer::lines(vec!["Nothing is running on this machine now.".to_owned()])
      } else {
        Answer::lines(lines)
      }
    }
    Err(why) => Answer::refusal(why),
  }
}

async fn jobs(handed: &Handed) -> Answer {
  let args: RunsArgs = match serde_json::from_value(handed.arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("jobs: {why}")),
  };
  let chat = match room(&args, handed) {
    Ok(chat) => chat,
    Err(refused) => return refused,
  };
  let service = match tool_call::agency_service(&handed.session) {
    Ok(service) => service,
    Err(why) => return Answer::refusal(why),
  };
  let limit = args.limit.unwrap_or(50).clamp(1, 500).to_string();
  let query = vec![("roomId", chat.to_string()), ("limit", limit)];

  match tool_call::get(&service, "/jobs", &query).await {
    Ok(page) => {
      let lines: Vec<String> = page
        .get("items")
        .and_then(Value::as_array)
        .map(|jobs| jobs.iter().map(|job| run_line(job, args.full)).collect())
        .unwrap_or_default();

      if lines.is_empty() {
        Answer::lines(vec![format!("Room {chat} has no job runs.")])
      } else {
        Answer::lines(lines)
      }
    }
    Err(why) => Answer::refusal(why),
  }
}

async fn work_order(handed: &Handed) -> Answer {
  let args: WorkOrderArgs = match serde_json::from_value(handed.arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("work_order: {why}")),
  };
  let service = match job_service(&handed.session) {
    Ok(service) => service,
    Err(why) => return Answer::refusal(why),
  };
  let order = match tool_call::get(&service, &format!("/items/{}/work-order", args.item), &[]).await {
    Ok(order) => order,
    Err(why) => return Answer::refusal(why),
  };
  let tasks = tool_call::get(
    &service,
    &format!("/items/{}/work-order/tasks", args.item),
    &[("limit", "500".to_owned())],
  )
  .await;
  let mut lines = vec![format!("item #{} — {}", args.item, text(&order, "state").unwrap_or_default())];

  if let Some(purpose) = text(&order, "purpose") {
    lines.push(format!("purpose: {purpose}"));
  }
  if let Some(directive) = text(&order, "directive") {
    lines.push(format!("directive: {directive}"));
  }
  if let Ok(page) = tasks
    && let Some(tasks) = page.get("items").and_then(Value::as_array)
    && !tasks.is_empty()
  {
    lines.push("tasks".to_owned());
    for task in tasks {
      lines.push(task_line(task));
    }
  }

  Answer::lines(lines)
}

async fn archive_scan(handed: &Handed) -> Answer {
  let args: ScanArgs = match serde_json::from_value(handed.arguments.clone()) {
    Ok(args) => args,
    Err(why) => return Answer::refusal(format!("archive_scan: {why}")),
  };
  let chat = match room(
    &RunsArgs {
      chat: args.chat,
      ..Default::default()
    },
    handed,
  ) {
    Ok(chat) => chat,
    Err(refused) => return refused,
  };
  let service = match receiver_service(&handed.session) {
    Ok(service) => service,
    Err(why) => return Answer::refusal(why),
  };
  let limit = args.limit.unwrap_or(100).clamp(1, 500).to_string();

  if let Some(id) = args.id {
    let one = match tool_call::get(&service, &format!("/rooms/{chat}/messages/{id}"), &[]).await {
      Ok(one) => one,
      Err(why) => return Answer::refusal(why),
    };

    return Answer::lines(vec![message_line(&one)]);
  }

  let mut query = vec![("limit", limit)];
  if let Some(before) = args.before_id {
    query.push(("before", before.to_string()));
  }
  if let Some(words) = args.contains.as_deref().filter(|words| !words.trim().is_empty()) {
    query.push(("q", words.to_owned()));
  }

  match tool_call::get(&service, &format!("/rooms/{chat}/messages"), &query).await {
    Ok(page) => {
      let mut lines: Vec<String> = page
        .get("items")
        .and_then(Value::as_array)
        .map(|messages| messages.iter().map(message_line).collect())
        .unwrap_or_default();

      if let Some(sender) = args.sender {
        let kept: Vec<String> = page
          .get("items")
          .and_then(Value::as_array)
          .map(|messages| {
            messages
              .iter()
              .filter(|message| number(message, "senderId") == Some(sender))
              .map(message_line)
              .collect()
          })
          .unwrap_or_default();
        lines = kept;
      }

      Answer::lines(lines)
    }
    Err(why) => Answer::refusal(why),
  }
}

pub fn message_line(message: &Value) -> String {
  let id = number(message, "id").unwrap_or(0);
  let sender = text(message, "senderName").unwrap_or_default();
  let at = number(message, "sentAt").unwrap_or(0);
  let body = text(message, "text").unwrap_or_default();
  let body = body.replace('\n', " ");

  format!("#{id} {sender} {at}: {body}")
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
