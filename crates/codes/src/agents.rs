//! Workers, their checkouts on nodes and where they are placed: the refusals of a change to them.

use axum::http::StatusCode;
use strum::{IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum AgentRefusal {
  #[error("there is no worker {worker}")]
  NoWorker { worker: String },
  #[error("{worker} is not a worker the owner added, so it is not set to rest or woken")]
  NotRestable { worker: String },
  #[error("a worker brought back to this machine needs its path on it")]
  BackHereNeedsPath,
  #[error("the change names nothing to change")]
  NothingToChange,
  #[error("{base} is not a base")]
  NotABase { base: String },
  #[error("{base} stands on {node}, so it takes no node checkout")]
  BaseOnNode { base: String, node: String },
  #[error("{base} already has its checkout on {node}")]
  CheckoutAlreadyOnNode { base: String, node: String },
  #[error("the checkout of {base} would be named {name}, which breaks the worker rule")]
  CheckoutNameLong { base: String, name: String },
  #[error("an agent named {name} already stands")]
  AgentNameTaken { name: String },
  #[error("a node's path starts with / or ~/")]
  NodePathForm,
  #[error("{base} has no checkout on {node}")]
  CheckoutNotOnNode { base: String, node: String },
  #[error("a job runs on {worker}")]
  CheckoutBusy { worker: String },
  #[error("{worker} could not be taken away")]
  CheckoutNotRemoved { worker: String },
  #[error("there is no node {node}")]
  NoNode { node: String },
  #[error("{node} takes no job: {why}")]
  NodeTakesNoJobs { node: String, why: String },
  #[error("the built-in worker stays on this machine")]
  BuiltinStaysHere,
  #[error("{worker} is a support worker of {base} and stands where its base stands")]
  SupportFollowsBase { worker: String, base: String },
  #[error("{worker} has support workers or node checkouts, so it stays on this machine")]
  HasSupport { worker: String },
  #[error(
    "{name} is no worker name: it starts with a small letter or a digit, runs 2 to 32 characters, and holds only small letters, digits, dots, underscores and dashes"
  )]
  WorkerNameForm { name: String },
  #[error("the label is empty")]
  LabelEmpty,
  #[error("a label runs to {most} characters")]
  LabelLong { most: u32 },
  #[error("the path is empty")]
  PathEmpty,
  #[error("{path} is not an absolute path")]
  PathNotAbsolute { path: String },
  #[error("{path} does not exist")]
  PathAbsent { path: String },
  #[error("{path} is not a directory")]
  PathNotDirectory { path: String },
  #[error(
    "something that is not a directory stands at {path}, so the working directory cannot be made there"
  )]
  MadePathBlocked { path: String },
  #[error("a preamble runs to {most} characters")]
  PreambleLong { most: u32 },
  #[error("a dev server command is one line")]
  DevCommandLines,
  #[error("a dev server command runs to {most} characters")]
  DevCommandLong { most: u32 },
  #[error(
    "{worker} is a support worker of {base}, so its label, path, preamble, dev server and pin are changed in {base}'s settings"
  )]
  SupportChangedByBase { worker: String, base: String },
  #[error("{worker} is the built-in worker, which runs in this repository, so its path is not moved")]
  BuiltinPathFixed { worker: String },
  #[error("{worker} is a support worker of {base}: lower {base}'s support count instead of removing it")]
  SupportRemovedByCount { worker: String, base: String },
  #[error("{worker} is the built-in worker, which is never removed")]
  BuiltinNotRemoved { worker: String },
  #[error(
    "{worker} is a support worker of {base} and has no support settings of its own: change them in {base}'s"
  )]
  SupportHasNoSettings { worker: String, base: String },
  #[error("a support count is a whole number from 0 to {most}")]
  SupportCountRange { most: u32 },
  #[error("a keepalive count is a whole number of 0 or more")]
  KeepaliveCountForm,
  #[error("the keepalive count ({keepalive}) cannot pass the support count ({count})")]
  KeepaliveOverCount { keepalive: u32, count: u32 },
  #[error("{base}'s name is too long for its support worker's name {name}")]
  SupportNameLong { base: String, name: String },
}

coded!(AgentRefusal);

impl Refused for AgentRefusal {
  fn status(&self) -> StatusCode {
    match self {
      Self::BackHereNeedsPath
      | Self::NothingToChange
      | Self::NotABase { .. }
      | Self::CheckoutNameLong { .. }
      | Self::NodePathForm
      | Self::WorkerNameForm { .. }
      | Self::LabelEmpty
      | Self::LabelLong { .. }
      | Self::PathEmpty
      | Self::PathNotAbsolute { .. }
      | Self::PathAbsent { .. }
      | Self::PathNotDirectory { .. }
      | Self::MadePathBlocked { .. }
      | Self::PreambleLong { .. }
      | Self::DevCommandLines
      | Self::DevCommandLong { .. }
      | Self::SupportCountRange { .. }
      | Self::KeepaliveCountForm
      | Self::KeepaliveOverCount { .. }
      | Self::SupportNameLong { .. } => StatusCode::BAD_REQUEST,
      Self::NoWorker { .. } | Self::CheckoutNotOnNode { .. } | Self::NoNode { .. } => StatusCode::NOT_FOUND,
      Self::CheckoutNotRemoved { .. } => StatusCode::INTERNAL_SERVER_ERROR,
      Self::NotRestable { .. }
      | Self::BaseOnNode { .. }
      | Self::CheckoutAlreadyOnNode { .. }
      | Self::AgentNameTaken { .. }
      | Self::CheckoutBusy { .. }
      | Self::NodeTakesNoJobs { .. }
      | Self::BuiltinStaysHere
      | Self::SupportFollowsBase { .. }
      | Self::HasSupport { .. }
      | Self::SupportChangedByBase { .. }
      | Self::BuiltinPathFixed { .. }
      | Self::SupportRemovedByCount { .. }
      | Self::BuiltinNotRemoved { .. }
      | Self::SupportHasNoSettings { .. } => StatusCode::CONFLICT,
    }
  }
}
