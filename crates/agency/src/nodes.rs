//! Nodes: the machines work may run on, how SSH reaches them, and where the next heavy run goes.

use serde::{Deserialize, Serialize};

/// A node as bmo-agency last read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
  pub name: String,
  pub label: String,
  pub kind: String,
  pub role: Option<String>,
  pub state: String,
  pub checked_at: Option<i64>,
}

/// How SSH reaches a node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshReach {
  pub host: String,
  pub port: u16,
  pub user: String,
  pub key_path: Option<String>,
  pub shell: Option<String>,
}

/// A machine a heavy run is placed on: this one where `node` is none, else that node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkMachine {
  pub name: String,
  pub label: String,
  pub node: Option<String>,
}
