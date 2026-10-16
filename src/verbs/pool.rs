//! The live picture of the co-worker pool the console draws as tiles (docs/api.md, "The co-worker pool
//! board").

use std::future::Future;

use serde::Serialize;

use super::{Done, Target, Unbuilt};

/// What holds each place of the pool right now.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Board {
  pub read_at: i64,
  pub buckets: Vec<Bucket>,
  pub workers: Vec<WorkerTile>,
}

/// The places a tier has on a provider that is on and lends co-workers.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Bucket {
  pub provider: String,
  pub tier: String,
  pub capacity: u32,
  pub tiles: Vec<Tile>,
  pub waiting: Vec<Work>,
}

/// A worker's own turn.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkerTile {
  pub name: String,
  pub label: String,
  pub tile: Tile,
  pub waiting: Vec<Work>,
}

/// One place, and what holds it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Tile {
  pub state: TileState,
  pub work: Option<Work>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TileState {
  Idle,
  Working,
  Held,
}

/// The work a place holds or waits for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Work {
  pub kind: WorkKind,
  pub role: String,
  pub tier: String,
  pub provider: String,
  pub model: String,
  pub effort: Option<String>,
  pub since: Option<i64>,
  pub item: Option<i64>,
  /// The item's title, a run's number or a job's purpose, as it stands.
  pub title: Option<String>,
  /// The step, a depth or a worker, as it stands.
  pub detail: Option<String>,
  pub stop: Option<Target>,
  pub log: Option<LogPlace>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
  Turn,
  Cowork,
  Search,
  Browse,
  Native,
  Other,
}

/// Where the work's log is: the agent that writes it and the run it belongs to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct LogPlace {
  pub agent: String,
  pub run: String,
}

/// The pool read for its board.
pub trait PoolBoard {
  fn pool_board(&self) -> impl Future<Output = Done<Board>> + Send;
}

impl PoolBoard for Unbuilt {
  fn pool_board(&self) -> impl Future<Output = Done<Board>> + Send {
    Self::refused("pool_board")
  }
}
