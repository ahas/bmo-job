//! The readings (docs/api.md, "Readings"; docs/reading.md): the queue and an item as a person reads them
//! and as a worker reads them, a room's jobs, a worker's job line and its prompt, the jobs that have just
//! run, and a work order with where its tasks stand.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query};
use axum::routing::get;
use job_agency::Agency;
use job_base::Refusal;
use job_codes::WorkOrderRefusal;
use job_gate::Gate;
use queue_read::lines::{Facing, Wanted, todo_list_items};
use queue_read::person_reading::{item_lines, item_reading};
use queue_read::queue::{Queue, TodoItem};
use queue_read::work_order::{self, Place};
use queue_read::worker_lines::{
  Kept, list_texts, show_texts, todo_list_worker_lines, todo_show_worker_lines,
};
use serde::Serialize;
use serde_json::{Value, json};

use super::{Answered, State, body};
use crate::queue::units::WORKER_LANGUAGE;
use crate::server::Server;
use crate::server::request::refused;
use crate::server::units;
use crate::verbs::Verbs;

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new()
    .route("/todos", get(listed::<V>))
    .route("/todos/worker-lines", get(listed_for_worker::<V>))
    .route("/todos/{item}", get(shown::<V>))
    .route("/todos/{item}/worker-lines", get(shown_for_worker::<V>))
    .route("/rooms/{chat}/jobs", get(jobs::<V>))
    .route("/workers/{name}/job", get(job_line::<V>))
    .route("/workers/{name}/job/prompt", get(job_prompt::<V>))
    .route("/jobs/recent", get(recent::<V>))
    .route("/work-orders/{item}", get(order::<V>))
    .route("/work-orders/{item}/current", get(current::<V>))
    .route("/work-orders/{item}/tasks/{task}", get(task::<V>))
}

type Asked = Query<Vec<(String, String)>>;

// Which rows a listing is asked for: a flag given bare or with any value but `false` or `0` is set.
fn wanted(query: &[(String, String)]) -> Wanted {
  let set =
    |name: &str| query.iter().any(|(key, value)| key == name && !matches!(value.as_str(), "false" | "0"));

  Wanted {
    all: set("all"),
    pending_only: set("pending"),
    held_only: set("held"),
    merged_only: set("merged"),
    chat_id: query
      .iter()
      .find(|(key, _)| key == "chat")
      .and_then(|(_, chat)| chat.trim().parse().ok()),
  }
}

// The queue read once, with what its lines are read against: the workers, the held items whose run still
// goes, and bmo-agency's provider order.
async fn opened<V: Verbs>(state: &Server<V>) -> Result<(Queue, Facing), Refusal> {
  let queue = Queue::open(&state.db).await?;
  let providers = state.agency.providers().await.map_err(units::agency)?;
  let facing = Facing {
    workers: queue.workers().to_vec(),
    stopping: queue.hold_pending_ids(queue.all_todos()),
    providers,
  };

  Ok((queue, facing))
}

// The English bmo-gate keeps of what is written in another language; none where it does not answer, so
// all of it stands under the untranslated mark.
async fn kept<V: Verbs>(state: &Server<V>, texts: Vec<String>) -> Kept {
  if texts.is_empty() {
    return Kept::new();
  }

  state.gate.kept(WORKER_LANGUAGE, &texts).await.unwrap_or_else(|failed| {
    eprintln!("bmo-job: the kept English could not be read: {failed}");
    Kept::new()
  })
}

// The item asked for: the one of that number where what is asked is one, or the unfinished one of that
// title.
fn asked_item<'a>(queue: &'a Queue, asked: &str) -> Result<&'a TodoItem, Refusal> {
  let trimmed = asked.trim();
  let found = match trimmed.parse::<i64>().ok().filter(|id| id.to_string() == trimmed) {
    Some(id) => queue.get_todo(id),
    None => queue.find_todo_by_title(asked),
  };

  found.ok_or_else(|| refused(WorkOrderRefusal::ItemAbsent))
}

// A reading of a span names no tool's server: the tool host keeps that catalogue.
fn no_toolbook(_: &str) -> Option<(String, String)> {
  None
}

async fn listed<V: Verbs>(state: State<V>, Query(query): Asked) -> Answered {
  let (queue, facing) = opened(&state).await?;

  body(item_lines(queue.all_todos(), &wanted(&query), &facing))
}

async fn shown<V: Verbs>(state: State<V>, Path(asked): Path<String>) -> Answered {
  let (mut queue, facing) = opened(&state).await?;
  let id = asked_item(&queue, &asked)?.id;

  queue.read_item(&state.db, id).await?;

  let item = asked_item(&queue, &id.to_string())?;

  body(item_reading(&queue, item, &facing, &no_toolbook))
}

async fn listed_for_worker<V: Verbs>(state: State<V>, Query(query): Asked) -> Answered {
  let (queue, facing) = opened(&state).await?;
  let wanted = wanted(&query);
  let shown: Vec<TodoItem> =
    todo_list_items(queue.all_todos(), &wanted, &facing).into_iter().cloned().collect();
  let kept = kept(&state, list_texts(&shown)).await;

  body(todo_list_worker_lines(queue.all_todos(), &wanted, &facing, &kept))
}

async fn shown_for_worker<V: Verbs>(state: State<V>, Path(asked): Path<String>) -> Answered {
  let (mut queue, facing) = opened(&state).await?;
  let id = asked_item(&queue, &asked)?.id;

  queue.read_item(&state.db, id).await?;

  let item = asked_item(&queue, &id.to_string())?;
  let kept = kept(&state, show_texts(&queue, item)).await;

  body(todo_show_worker_lines(&queue, item, &facing, &kept, &state.names))
}

async fn jobs<V: Verbs>(
  state: State<V>,
  Path(chat): Path<i64>,
  Query(query): Query<HashMap<String, String>>,
) -> Answered {
  let limit = query.get("limit").and_then(|limit| limit.trim().parse().ok());

  body(queue_read::runs::jobs(&state.db, chat, limit).await?)
}

// The unit of the job a worker's screen was opened at, where it names one.
fn opened_at(query: &HashMap<String, String>) -> Option<&str> {
  query.get("run").map(|run| run.trim()).filter(|run| !run.is_empty())
}

async fn job_line<V: Verbs>(
  state: State<V>,
  Path(worker): Path<String>,
  Query(query): Query<HashMap<String, String>>,
) -> Answered {
  let job = match queue_read::runs::worker_job(&state.db, &worker, opened_at(&query)).await? {
    Some(job) => queue_read::runs::job_line(&state.db, job).await?,
    None => None,
  };
  let job = match job {
    // A running job has written no spans yet: its tool calls are counted in its run's log.
    Some(mut line) if line.finished_at.is_none() => {
      let calls = state.agency.run_tool_calls(&worker, &line.run).await.map_err(units::agency)?;

      line.tools = i64::try_from(calls).unwrap_or(i64::MAX);
      Some(line)
    }
    finished => finished,
  };

  body(json!({ "job": job }))
}

async fn job_prompt<V: Verbs>(
  state: State<V>,
  Path(worker): Path<String>,
  Query(query): Query<HashMap<String, String>>,
) -> Answered {
  let prompt = match queue_read::runs::worker_job(&state.db, &worker, opened_at(&query)).await? {
    Some(job) => queue_read::runs::job_prompt(&state.db, job).await?,
    None => None,
  };

  body(json!({ "prompt": prompt }))
}

async fn recent<V: Verbs>(state: State<V>, Query(query): Query<HashMap<String, String>>) -> Answered {
  let limit = query.get("limit").and_then(|limit| limit.trim().parse().ok());

  body(queue_read::runs::recent_jobs(&state.db, limit).await?)
}

// An item's work order: the one the database holds, or the draft the item would start from.
async fn read_order<V: Verbs>(state: &Server<V>, item: i64) -> Result<Value, Refusal> {
  work_order::read(&state.db, item)
    .await?
    .ok_or_else(|| refused(WorkOrderRefusal::ItemAbsent))
}

async fn order<V: Verbs>(state: State<V>, Path(item): Path<i64>) -> Answered {
  let order = read_order(&state, item).await?;
  let lines = work_order::lines(&order, &state.names.owner);

  body(json!({ "order": order, "lines": lines }))
}

/// A task with where it stands.
#[derive(Serialize)]
struct Placed<'a> {
  place: Place,
  task: &'a Value,
}

async fn current<V: Verbs>(state: State<V>, Path(item): Path<i64>) -> Answered {
  let order = read_order(&state, item).await?;
  let placed: Vec<Placed<'_>> = work_order::current(&order)
    .into_iter()
    .map(|(place, task)| Placed { place, task })
    .collect();

  body(placed)
}

async fn task<V: Verbs>(state: State<V>, Path((item, task)): Path<(i64, String)>) -> Answered {
  let order = read_order(&state, item).await?;
  let (place, found) = work_order::task_named(&order, &task)
    .ok_or_else(|| refused(WorkOrderRefusal::TaskAbsent { task: task.clone() }))?;

  body(Placed { place, task: found })
}
