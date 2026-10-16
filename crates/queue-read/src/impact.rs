//! An item's impact grounds: the feature nodes of the code graph a change reaches, each with the outward
//! contract that changes on it, and whether the change needs a new home. The verdict they come to, one
//! job or a co-work, is stated here alone; everything that weighs an item asks it.

use serde::{Deserialize, Serialize};

/// The grounds an item's co-work setting is judged on, as the item keeps them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactGrounds {
  /// Every graph node the change reaches, the node to change first, then what the graph's impact reached.
  pub nodes: Vec<ImpactNode>,
  /// Whether a new module, table or other home must be decided before the work is written.
  #[serde(default)]
  pub new_home: bool,
}

/// One node the change reaches.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImpactNode {
  /// The node's graph id.
  pub id: String,
  /// The outward contract that changes on it (API, schema, wire format, policy file or documented
  /// behaviour), or null where only its inside changes.
  #[serde(default)]
  pub contract: Option<String>,
}

/// How an item's job runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
  Single,
  Cowork,
}

/// Why written grounds are not grounds.
#[derive(Debug, PartialEq, Eq)]
pub enum Fault {
  /// Not the shape at all, with the reader's own words.
  Shape(String),
  /// No node and no new home: nothing was weighed.
  Empty,
  /// A node without an id, or a contract written as nothing.
  Blank,
  /// The same node twice.
  Repeated(String),
}

impl std::fmt::Display for Fault {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Shape(why) => write!(f, "{why}"),
      Self::Empty => write!(f, "no node and no new home"),
      Self::Blank => write!(f, "a blank node id or contract (null is an inside-only change)"),
      Self::Repeated(id) => write!(f, "node {id} named twice"),
    }
  }
}

impl ImpactGrounds {
  /// The grounds written as JSON, held to their shape.
  pub fn read(text: &str) -> Result<Self, Fault> {
    let grounds: Self = serde_json::from_str(text).map_err(|why| Fault::Shape(why.to_string()))?;

    grounds.checked()
  }

  /// The grounds held to their shape: something weighed, every id and contract written, no node twice.
  pub fn checked(self) -> Result<Self, Fault> {
    if self.nodes.is_empty() && !self.new_home {
      return Err(Fault::Empty);
    }

    let mut seen: Vec<&str> = vec![];

    for node in &self.nodes {
      if node.id.trim().is_empty() || node.contract.as_deref().is_some_and(|said| said.trim().is_empty()) {
        return Err(Fault::Blank);
      }

      if seen.contains(&node.id.as_str()) {
        return Err(Fault::Repeated(node.id.clone()));
      }

      seen.push(&node.id);
    }

    Ok(self)
  }

  /// The grounds as a worker reads them, a line per node with the outward contract that changes on it,
  /// and a line where a new home must be decided.
  pub fn lines(&self) -> Vec<String> {
    self
      .nodes
      .iter()
      .map(|node| format!("- {}: {}", node.id, node.contract.as_deref().unwrap_or("inside only")))
      .chain(self.new_home.then(|| "- A new home must be decided.".to_owned()))
      .collect()
  }

  /// The form the item keeps them in.
  pub fn written(&self) -> String {
    serde_json::to_string(self).unwrap_or_default()
  }

  /// How many nodes have an outward contract that changes.
  pub fn contract_changes(&self) -> usize {
    self.nodes.iter().filter(|node| node.contract.is_some()).count()
  }

  /// The rule an item is weighed by: a change whose outward contract moves on two nodes or more, or which
  /// needs a new home, is a co-work; one contract, or a change inside nodes alone however many files it
  /// touches, is a single job.
  pub fn verdict(&self) -> Verdict {
    if self.new_home || self.contract_changes() >= 2 {
      Verdict::Cowork
    } else {
      Verdict::Single
    }
  }
}
