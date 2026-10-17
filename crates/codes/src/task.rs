//! Launching a job: the refusal of a launch asked wrong, what a launch came to, and the notes it carries.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::{Refused, listed};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum TaskRefusal {
  #[error("a job takes the room it reports to and what to do")]
  TaskNeedsRoomAndWork,
  #[error("a launch says who launched it: chat:<chat> from a session, or the launcher's own name")]
  LaunchNeedsLauncher,
  #[error("a run of item #{item} is named by the item: a line of why is for a job with no item")]
  ItemLaunchTakesNoWhy { item: i64 },
  #[error("a launch's line of why is one line: what the job is for")]
  WhyOneLine,
  #[error("a job with no item says why it was launched, in one line naming what it is for")]
  ItemlessNeedsWhy,
  #[error("a session launches a TODO item of {worker}'s queue{}", front.map(|front| format!(", whose front is #{front}")).unwrap_or_default())]
  SessionLaunchesItem { worker: String, front: Option<i64> },
  #[error("#{item} is no TODO item")]
  LaunchItemAbsent { item: i64 },
  #[error("#{item} {title} is done, and a finished item is not launched again")]
  LaunchItemDone { item: i64, title: String },
  #[error("#{item} {title} is merged into #{into}, whose job carries its work")]
  LaunchItemMerged { item: i64, title: String, into: i64 },
  #[error("#{item} {title} is held{}, and is not launched until it is released", why.as_ref().map(|why| format!(" ({why})")).unwrap_or_default())]
  LaunchItemHeld {
    item: i64,
    title: String,
    why: Option<String>,
  },
  #[error("#{item} {title} is not approved, and waits for the owner's word")]
  LaunchItemUnapproved { item: i64, title: String },
  #[error("#{item} {title} is {worker}'s work, not {asked}'s")]
  LaunchItemOtherWorker {
    item: i64,
    title: String,
    worker: String,
    asked: String,
  },
  #[error(
    "#{item} {title}: its last {streak} launches died before reaching the work, so it waits {wait_ms} ms more"
  )]
  LaunchWaits {
    item: i64,
    title: String,
    streak: u32,
    wait_ms: i64,
  },
  #[error(
    "the checkout {cwd} is at work already ({}), so nothing else is launched there",
    listed(holders)
  )]
  CheckoutHeld {
    cwd: String,
    holders: Vec<CheckoutHolder>,
  },
  #[error(
    "the checkout {cwd} is held by the work orders of {}, and no other checkout of the worker is free",
    listed(&orders.iter().map(|item| format!("#{item}")).collect::<Vec<_>>())
  )]
  CheckoutHeldByOrder { cwd: String, orders: Vec<i64> },
  #[error("#{item} is at work already ({}), so it is not launched a second time", listed(holders))]
  LaunchItemAtWork {
    item: i64,
    holders: Vec<CheckoutHolder>,
  },
  #[error(
    "{worker} is a support worker beyond {base}'s count, so nothing is launched on it; launch on {base}"
  )]
  LaunchBeyondCount { worker: String, base: String },
  #[error(
    "the machine has no room for another job ({}), and the queue is told again once it has",
    listed(held)
  )]
  LaunchHeldByMachine { held: Vec<String> },
  #[error(
    "#{item}'s requester, user {user}, {}",
    grant.as_ref().map_or_else(|| "has no Claude account of their own turned on to run it".to_owned(), |grant| format!("does not hold {grant}"))
  )]
  RequesterLacks {
    item: i64,
    user: String,
    grant: Option<String>,
  },
  #[error("a co-work runs an item's work order, and this launch names no item")]
  CoworkNeedsItem,
  #[error("there is no runner {runner}; the runners are {}", listed(runners))]
  RunnerUnknown { runner: String, runners: Vec<String> },
  #[error("a depth is one of {}", listed(depths))]
  RunnerDepthWrong { depths: Vec<String> },
  #[error("the {runner} launcher did not start the run: {why}")]
  RunnerNotLaunched { runner: String, why: String },
  #[error(
    "the user's service manager cannot be reached ({why}), so nothing was launched and the item's launch wait is untouched"
  )]
  ManagerUnreachable { why: String },
  #[error("job {job}'s unit did not start, so the job ended as an error")]
  UnitNotStarted { job: i64 },
  #[error("the job's text could not be written for its unit: {why}")]
  PromptNotWritten { why: String },
}

coded!(TaskRefusal);

impl Refused for TaskRefusal {
  fn status(&self) -> StatusCode {
    match self {
      Self::TaskNeedsRoomAndWork
      | Self::LaunchNeedsLauncher
      | Self::ItemLaunchTakesNoWhy { .. }
      | Self::WhyOneLine
      | Self::ItemlessNeedsWhy
      | Self::SessionLaunchesItem { .. }
      | Self::RunnerUnknown { .. }
      | Self::RunnerDepthWrong { .. }
      | Self::CoworkNeedsItem => StatusCode::BAD_REQUEST,
      Self::RunnerNotLaunched { .. } | Self::UnitNotStarted { .. } | Self::PromptNotWritten { .. } => {
        StatusCode::INTERNAL_SERVER_ERROR
      }
      Self::ManagerUnreachable { .. } => StatusCode::SERVICE_UNAVAILABLE,
      Self::LaunchItemAbsent { .. } => StatusCode::NOT_FOUND,
      Self::LaunchItemDone { .. }
      | Self::LaunchItemMerged { .. }
      | Self::LaunchItemHeld { .. }
      | Self::LaunchItemUnapproved { .. }
      | Self::LaunchItemOtherWorker { .. }
      | Self::LaunchWaits { .. }
      | Self::CheckoutHeld { .. }
      | Self::CheckoutHeldByOrder { .. }
      | Self::LaunchItemAtWork { .. }
      | Self::LaunchBeyondCount { .. }
      | Self::LaunchHeldByMachine { .. }
      | Self::RequesterLacks { .. } => StatusCode::CONFLICT,
    }
  }
}

/// One job at work in a checkout, as a refusal of a launch there names it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckoutHolder {
  pub worker: String,
  pub job: i64,
  pub item: Option<i64>,
  pub unit: String,
  /// True where the job borrowed the checkout.
  pub borrowed: bool,
}

impl std::fmt::Display for CheckoutHolder {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(out, "{}'s job {} as {}", self.worker, self.job, self.unit)?;

    if let Some(item) = self.item {
      write!(out, " of item #{item}")?;
    }

    if self.borrowed {
      write!(out, " on a borrowed checkout")?;
    }

    Ok(())
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TaskAnswer {
  JobLaunched {
    job: i64,
    unit: String,
    worker: String,
    cwd: String,
    chat: i64,
    notes: Vec<LaunchNote>,
  },
  OrderStarted {
    item: i64,
    phase: String,
    notes: Vec<LaunchNote>,
  },
  OrderAlreadyActive {
    item: i64,
    notes: Vec<LaunchNote>,
  },
}

coded!(TaskAnswer);

/// What a launch says beside what it came to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum LaunchNote {
  CoworkOffCheckout { worker: String, cwd: String },
}
