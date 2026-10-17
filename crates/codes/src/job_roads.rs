//! The roads a job's run goes out on, as the agent is told them: a switch, its answer, a road closed and
//! every road tried.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RoadFact {
  RoadSwitching {
    job_id: i64,
    item: Option<i64>,
    worker: String,
    requested: String,
    from: String,
    to: String,
    cause: SwitchCause,
  },
  RoadAnswered {
    job_id: i64,
    item: Option<i64>,
    worker: String,
    requested: String,
    from: String,
    to: String,
  },
  RoadUnavailable {
    job_id: i64,
    item: Option<i64>,
    worker: String,
    requested: String,
    selection: String,
    failure: RoadFailure,
  },
  RoadsExhausted {
    job_id: i64,
    item: Option<i64>,
    worker: String,
    requested: String,
    attempted: Vec<String>,
  },
}

coded!(RoadFact);

/// Why a run goes out on another road than the one it asked for.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SwitchCause {
  FirstUnavailable,
  PreviousFailed,
}

/// What a failed run says of its road.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum RoadFailure {
  Quota,
  Outage,
  Access,
}

/// The structure a work order's middle turn answers in.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum WorkOutput {
  Plan,
  Synthesis,
}
