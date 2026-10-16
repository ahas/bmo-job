//! A job's run that failed: how it died, as its room is told it.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum FailureFact {
  RunFailed {
    room: Option<String>,
    cause: RunDeath,
    said: Option<String>,
    last_tool: Option<String>,
    tool_detail: Option<String>,
    run_log: String,
  },
}

coded!(FailureFact);

/// How a run died.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RunDeath {
  Exit { exit_code: i32 },
  Signal { signal: String },
  ErrorResult,
  Silent,
  Transport,
  Unknown,
}
