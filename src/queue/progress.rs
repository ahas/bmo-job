//! How far a running job's work order has come (docs/queue.md, "What is running"): the step it is at,
//! the round where it was opened again, and which build step of how many, as every place that shows a
//! running job reads it and puts it into words.

use job_codes::work_lines::WorkStep;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How far a work order has come.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkProgress {
  /// The round, where the order was opened again past its first.
  pub round: Option<u32>,
  pub step: WorkStep,
  /// Which build step it is at, counted among the build steps alone, where the order has any.
  pub build: Option<BuildCount>,
}

/// One build step of how many.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildCount {
  pub at: u32,
  pub of: u32,
}

/// The step a research stage of this sort is.
pub fn stage_step(kind: Option<&str>) -> WorkStep {
  match kind {
    Some("review") => WorkStep::Review,
    _ => WorkStep::Investigate,
  }
}

fn count(value: usize) -> u32 {
  u32::try_from(value).unwrap_or(u32::MAX)
}

/// How far a work order has come: none for one not under way, or at a phase no step names.
pub fn work_progress(order: &Value) -> Option<WorkProgress> {
  let text = |key: &str| order.get(key).and_then(Value::as_str).unwrap_or_default();

  if order.is_null() || matches!(text("status"), "draft" | "done") {
    return None;
  }

  let round = order
    .get("round")
    .and_then(Value::as_u64)
    .filter(|round| *round > 1)
    .map(|round| u32::try_from(round).unwrap_or(u32::MAX));
  let mut build = None;
  let step = match text("phase") {
    "planning" => WorkStep::Plan,
    "research" => {
      let at = order.get("stage").and_then(Value::as_u64).unwrap_or(0);
      let stage = order
        .get("stages")
        .and_then(|stages| stages.get(usize::try_from(at).unwrap_or(usize::MAX)));

      stage_step(stage.and_then(|stage| stage.get("kind")).and_then(Value::as_str))
    }
    "synthesis" => WorkStep::Synthesis,
    "implementation" => {
      let tasks = order.get("tasks").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
      // A task the order has gone past is anything not waiting, running or failed.
      let current = tasks.iter().position(|task| {
        matches!(
          task.get("state").and_then(Value::as_str).unwrap_or_default(),
          "pending" | "running" | "failed"
        )
      });

      if !tasks.is_empty() {
        build = Some(BuildCount {
          at: count(current.map_or(tasks.len(), |at| at + 1)),
          of: count(tasks.len()),
        });
      }

      WorkStep::Build
    }
    "finalization" => WorkStep::Final,
    _ => return None,
  };

  Some(WorkProgress { round, step, build })
}
