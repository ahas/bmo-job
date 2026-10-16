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
}

coded!(AgentRefusal);

impl Refused for AgentRefusal {
  fn status(&self) -> StatusCode {
    match self {
      Self::BackHereNeedsPath
      | Self::NothingToChange
      | Self::NotABase { .. }
      | Self::CheckoutNameLong { .. }
      | Self::NodePathForm => StatusCode::BAD_REQUEST,
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
      | Self::HasSupport { .. } => StatusCode::CONFLICT,
    }
  }
}
