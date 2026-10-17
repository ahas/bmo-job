//! The service's state readings (docs/tools.md): a room's jobs, the room the call names or the turn's own, read
//! at `GET /rooms/{chat}/jobs`, and an item's work order read at `GET /work-orders/{item}` (docs/api.md,
//! "Readings"), each answered as the lines of the reading schema.

use futures_util::future::BoxFuture;
use job_tool_call::turn::NO_ROOM;
use job_tool_call::{Answer, Entry, Service, Turn, input};
use serde::Deserialize;
use serde_json::Value;

/// Every entry the executable answers, as its manifest declares them.
pub const ENTRIES: &[&dyn Entry] = &[&Jobs, &WorkOrder];

/// A room's jobs, newest first.
pub struct Jobs;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobsInput {
  pub chat: Option<i64>,
  pub limit: Option<i64>,
}

/// One item's work order.
pub struct WorkOrder;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkOrderInput {
  pub item: i64,
}

fn service() -> Result<Service, Answer> {
  Service::of_process().map_err(Answer::refusal)
}

impl Entry for Jobs {
  fn namespace(&self) -> &'static str {
    "bmo"
  }

  fn name(&self) -> &'static str {
    "jobs"
  }

  fn call<'a>(&'a self, turn: &'a Turn, arguments: Value) -> BoxFuture<'a, Answer> {
    Box::pin(async move {
      let asked: JobsInput = match input(&self.full_name(), arguments) {
        Ok(asked) => asked,
        Err(refused) => return refused,
      };
      let Some(room) = turn.room(asked.chat) else {
        return Answer::refusal(NO_ROOM);
      };
      let service = match service() {
        Ok(service) => service,
        Err(refused) => return refused,
      };
      let query: Vec<(&str, String)> =
        asked.limit.map(|limit| ("limit", limit.to_string())).into_iter().collect();

      match service.get(&["rooms", &room.to_string(), "jobs"], &query).await {
        Ok(Value::Array(jobs)) => Answer::reading(jobs.iter().map(Value::to_string).collect()),
        Ok(other) => Answer::refusal(format!("the service answered no list of jobs: {other}")),
        Err(why) => Answer::refusal(why),
      }
    })
  }
}

impl Entry for WorkOrder {
  fn namespace(&self) -> &'static str {
    "bmo"
  }

  fn name(&self) -> &'static str {
    "work_order"
  }

  fn call<'a>(&'a self, _: &'a Turn, arguments: Value) -> BoxFuture<'a, Answer> {
    Box::pin(async move {
      let asked: WorkOrderInput = match input(&self.full_name(), arguments) {
        Ok(asked) => asked,
        Err(refused) => return refused,
      };
      let service = match service() {
        Ok(service) => service,
        Err(refused) => return refused,
      };

      match service.get(&["work-orders", &asked.item.to_string()], &[]).await {
        Ok(read) => {
          let lines: Vec<String> = read["lines"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();

          Answer::reading(lines)
        }
        Err(why) => Answer::refusal(why),
      }
    })
  }
}
