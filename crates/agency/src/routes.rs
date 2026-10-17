//! Providers, tiers, routes and pins.

use serde::{Deserialize, Serialize};

use crate::workers::Pin;

/// A provider and the work lines it carries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provider {
  pub name: String,
  pub enabled: bool,
  pub registered: bool,
  /// True where it lends co-workers.
  #[serde(default)]
  pub coworker: bool,
  #[serde(default)]
  pub lines: Vec<String>,
}

/// A model tier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tier {
  pub name: String,
  /// True where a co-worker may run at it.
  pub coworker: bool,
}

/// What a route is resolved for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RouteAsk {
  pub work_key: String,
  pub provider: Option<String>,
  pub tier: String,
}

/// The model a route resolved to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mapping {
  pub provider: String,
  pub tier: String,
  pub model: String,
  pub effort: Option<String>,
  pub answer_tokens: Option<u64>,
}

/// What a pin's value chose.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PinChoice {
  Auto,
  Pinned(Pin),
}
