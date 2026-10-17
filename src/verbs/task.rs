//! A job launched on a worker from a shell (docs/verbs.md, "A job launched from a shell"): the launch its
//! words name handed to the server's own `POST /task`, so the running service decides every launch and this
//! verb judges nothing. A server that does not answer launches nothing.

use std::time::Duration;

use serde::Serialize;

use super::stand::{Answer, Stand, VerbError};

// A launch waits for a checkout's job lock where a runner is still letting it go, and for the work order's
// lock where it starts one.
const LAUNCH_WAIT: Duration = Duration::from_secs(180);

/// The usage a launch read wrongly is answered with.
pub const USAGE: &str = "usage: job start --chat <chat> [--todo <item>] [--worker <name>] [--model <model>] [--by <who>] [--why <one line>] [--cowork] --prompt <task>";

/// The launch the words ask for, as the server's route reads it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TaskAsked {
  pub chat: i64,
  pub prompt: String,
  pub model: Option<String>,
  pub item: Option<i64>,
  pub purpose: Option<String>,
  pub worker: Option<String>,
  pub cowork: bool,
  pub started_by: Option<String>,
}

/// The launch asked of the server: its answer as it came, or its refusal.
pub async fn task(stand: &Stand, asked: &TaskAsked) -> Result<Answer, VerbError> {
  let body = serde_json::to_value(asked).unwrap_or_default();

  match stand.ask_server("task", &body, LAUNCH_WAIT).await {
    Ok(answer) => Ok(vec![answer.to_string()].into()),
    Err(refused) => Err(VerbError::Ended {
      code: 1,
      said: format!("task refused: {}; nothing was launched.", refused.error),
    }),
  }
}
