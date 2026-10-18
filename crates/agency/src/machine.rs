//! The room on the machine the service runs on, the notice that launches are held for want of it, and the
//! word that the room returned. The machine's room is the one area of bmo-agency whose keys are camelCase.

use serde::{Deserialize, Serialize};

/// The room on the machine.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Room {
  /// Why launches are held now; empty where there is room.
  #[serde(default)]
  pub held: Vec<String>,
  /// The jobs at work on the machine.
  pub jobs: u64,
  /// The jobs whose place runs a browser.
  pub browser_jobs: u64,
  /// The browser main processes running.
  pub browsers: u64,
}

/// Launches held at `at`, in epoch milliseconds, and why. An empty `held` withdraws the notice.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LaunchHeld {
  pub at: i64,
  pub held: Vec<String>,
}

/// The room returned under a held launch's notice, which bmo-agency let go: when the launch was held, and
/// what held it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomReturned {
  pub at: i64,
  #[serde(default)]
  pub held: Vec<String>,
}
