//! The service's launching tool (docs/tools.md): a job launched on a worker in the room the call names or the
//! turn's own, set off as that room's session, through the service's `POST /task` (docs/api.md, "Jobs").

use futures_util::future::BoxFuture;
use job_tool_call::turn::NO_ROOM;
use job_tool_call::{Answer, Entry, Service, Turn, input};
use serde::Deserialize;
use serde_json::{Value, json};

/// Every entry the executable answers, as its manifest declares them.
pub const ENTRIES: &[&dyn Entry] = &[&Task];

/// A job launched on a worker.
pub struct Task;

/// What a launch is asked with.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskInput {
  pub chat: Option<i64>,
  #[serde(default)]
  pub cowork: bool,
  /// The message the call acts on, which the host judges its grant by.
  pub message: Option<i64>,
  pub model: Option<String>,
  pub prompt: String,
  pub todo: Option<i64>,
  pub why: Option<String>,
  pub worker: Option<String>,
}

/// The launch the service is asked, in `room`, set off by the session of the turn's room or of the room itself.
pub fn task_body(turn: &Turn, room: i64, asked: TaskInput) -> Value {
  json!({
    "chat": room,
    "prompt": asked.prompt,
    "model": asked.model,
    "item": asked.todo,
    "purpose": asked.why,
    "worker": asked.worker,
    "cowork": asked.cowork,
    "started_by": format!("chat:{}", turn.chat().unwrap_or(room)),
  })
}

impl Entry for Task {
  fn namespace(&self) -> &'static str {
    "bmo"
  }

  fn name(&self) -> &'static str {
    "task"
  }

  fn call<'a>(&'a self, turn: &'a Turn, arguments: Value) -> BoxFuture<'a, Answer> {
    Box::pin(async move {
      let asked: TaskInput = match input(&self.full_name(), arguments) {
        Ok(asked) => asked,
        Err(refused) => return refused,
      };
      let Some(room) = turn.room(asked.chat) else {
        return Answer::refusal(NO_ROOM);
      };
      let service = match Service::of_process() {
        Ok(service) => service,
        Err(why) => return Answer::refusal(why),
      };

      match service.post("task", &task_body(turn, room, asked)).await {
        Ok(answered) => Answer::text(answered.to_string()),
        Err(why) => Answer::refusal(why),
      }
    })
  }
}
