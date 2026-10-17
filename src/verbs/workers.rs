//! Workers added, changed, removed and placed on nodes (docs/api.md, "Workers").

use std::future::Future;

use serde::Serialize;

use super::{Done, Unbuilt};

/// A worker to add.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkerAdd {
  pub name: String,
  pub label: Option<String>,
  /// Its checkout; a directory of its own where none is given.
  pub path: Option<String>,
  pub preamble: Option<String>,
  pub dev_command: Option<String>,
}

/// The worker added.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkerAdded {
  pub name: String,
  pub label: String,
  pub cwd: String,
}

/// A change to a worker. A field left out is left as it stands; a workbench text given as none clears
/// it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorkerUpdate {
  pub label: Option<String>,
  pub path: Option<String>,
  pub preamble: Option<Option<String>>,
  pub dev_command: Option<Option<String>>,
  pub keepalive_stopped: Option<bool>,
  /// `auto`, a provider or `provider::model`.
  pub pinned_provider: Option<String>,
  pub support_count: Option<i64>,
  pub support_keepalive: Option<i64>,
}

/// Where a worker's checkouts stand on the nodes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeChange {
  AddCheckout {
    name: String,
    node: String,
    path: Option<String>,
  },
  RemoveCheckout {
    name: String,
    node: String,
  },
  /// A worker of its own stood on a node, or brought back to this machine where no node is named.
  Place {
    name: String,
    node: Option<String>,
    path: Option<String>,
  },
}

/// A checkout added on a node or taken off it: its worker and where it stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NodeCheckout {
  pub worker: String,
  pub cwd: String,
}

/// The workers changed.
pub trait Workers {
  fn add_worker(&self, add: WorkerAdd) -> impl Future<Output = Done<WorkerAdded>> + Send;

  fn update_worker(&self, name: String, update: WorkerUpdate) -> impl Future<Output = Done<()>> + Send;

  fn remove_worker(&self, name: String) -> impl Future<Output = Done<()>> + Send;

  /// A checkout added or taken off, answering it; a worker placed, answering none.
  fn change_node(&self, change: NodeChange) -> impl Future<Output = Done<Option<NodeCheckout>>> + Send;
}

impl Workers for Unbuilt {
  fn add_worker(&self, _: WorkerAdd) -> impl Future<Output = Done<WorkerAdded>> + Send {
    Self::refused("add_worker")
  }

  fn update_worker(&self, _: String, _: WorkerUpdate) -> impl Future<Output = Done<()>> + Send {
    Self::refused("update_worker")
  }

  fn remove_worker(&self, _: String) -> impl Future<Output = Done<()>> + Send {
    Self::refused("remove_worker")
  }

  fn change_node(&self, _: NodeChange) -> impl Future<Output = Done<Option<NodeCheckout>>> + Send {
    Self::refused("change_node")
  }
}
