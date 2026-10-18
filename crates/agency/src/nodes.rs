//! Nodes: the machines work may run on, and how SSH reaches them.

use serde::{Deserialize, Serialize};

/// A node as bmo-agency last read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct SshReach {
  pub host: String,
  pub port: u16,
  pub user: String,
  pub key_path: Option<String>,
  pub shell: Option<String>,
}
