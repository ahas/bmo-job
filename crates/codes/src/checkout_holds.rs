//! A work order's checkout: what a release waits on, where an order stands in its worker's line, what
//! became of its change, and why the checkout could not be moved.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CheckoutStop {
  CheckoutNotMoved {
    item: i64,
    place: CheckoutPlace,
    why: GitFailure,
  },
  ParkedNotMerged {
    item: i64,
    why: GitFailure,
  },
}

coded!(CheckoutStop);

/// What a checkout's release waits on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ReleaseWait {
  OpenJob {
    worker: String,
    job: i64,
  },
  Lock,
  EndedUnits {
    worker: String,
    units: Vec<String>,
  },
  EndedRecord {
    worker: String,
    unit: String,
    id: i64,
    label: String,
    more: u32,
  },
  OwnTurns {
    live: Vec<String>,
  },
  /// `starts` in epoch milliseconds.
  Checking {
    count: u32,
    starts: i64,
  },
  Release,
}

/// Where an order stands in its worker's line.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum LinePlace {
  ResumeTurn,
  Passing,
  Held,
  OwnerStop { why: String },
}

/// What became of an order's change at its end or its stop.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CheckoutNote {
  Unsettled {
    tries: u32,
    item: i64,
  },
  Unpushed {
    commit: String,
    refused: String,
  },
  Uncommitted {
    why: Option<String>,
  },
  Left {
    paths: Vec<String>,
  },
  Parked {
    count: u32,
    branch: String,
    commit: String,
    stopped: bool,
  },
  NotParked {
    why: Option<String>,
  },
  NotOnMain {
    main: String,
    said: String,
  },
}

/// The checkout an order works in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CheckoutPlace {
  OwnWorktree { worker: String },
  BaseCheckout,
}

/// Why a git step on a checkout went wrong.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum GitFailure {
  NoBranch { branch: String },
  NotFetched { branch: String, said: String },
  StepFailed { step: GitStep, said: String },
}

/// A git step on a checkout.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum GitStep {
  Add,
  Commit,
  ReadCommit,
  ReadHead,
  WriteTree,
  WriteCommit,
  MoveBranch,
  TakeBack,
  ReadBranch,
}
