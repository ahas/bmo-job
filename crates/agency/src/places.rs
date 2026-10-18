//! The places work stands in: a worker's workbench, and a work order's branch places in its pool of
//! standing checkouts, which bmo-agency keeps and chooses among. Paths are absolute on the machine.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// A worker's workbench and the item scratch directories under it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Workbench {
  pub root: PathBuf,
  #[serde(default)]
  pub scratches: Vec<PathBuf>,
}

/// A place a task sits in now: the checkout its order works in and the place's number.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldPlace {
  pub cwd: String,
  pub slot: i64,
}

/// A branch's place asked for: the checkout its order works in, the task's model selection, role and tier,
/// and every place a task sits in now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceAsk {
  pub cwd: String,
  pub selection: String,
  pub role: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub tier: Option<String>,
  pub held: Vec<HeldPlace>,
}

/// The place taken: its number, and its directory, which a branch's turn works in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TakenPlace {
  pub slot: i64,
  pub cwd: PathBuf,
}

/// What a finished branch left in its place: the checkout its order works in, the place, the role, and the
/// provider, model, effort and upstream it answered on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceRecord {
  pub cwd: String,
  pub slot: i64,
  pub role: String,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub mode: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub model: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub effort: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub upstream: Option<String>,
}

/// An ended order's places handed back: the checkout it worked in, the places its tasks used, and every
/// place a task sits in now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlacesRetirement {
  pub cwd: String,
  pub used: Vec<i64>,
  pub held: Vec<HeldPlace>,
}
