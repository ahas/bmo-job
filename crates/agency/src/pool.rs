//! The co-worker pool: requests for a co-worker's place, reservations of seats, and the writes a place
//! was given.

use serde::{Deserialize, Serialize};

/// What a pool request is enqueued with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolAsking {
  pub request_key: String,
  pub owner: String,
  pub source: String,
  pub role: String,
  pub work_key: String,
  pub provider: String,
  pub model: String,
  pub effort: Option<String>,
  pub tier: String,
  pub yielding: bool,
  pub place_key: Option<String>,
  pub turn: bool,
  pub reservation: Option<String>,
}

/// Where a pool request stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PoolState {
  Waiting,
  Active,
  Ended,
}

/// A pool request: its asking and where it stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolRequest {
  #[serde(flatten)]
  pub asking: PoolAsking,
  pub token: String,
  pub state: PoolState,
  pub enqueued_at: i64,
  pub leased_at: Option<i64>,
  pub child: Option<String>,
  pub ended_at: Option<i64>,
  pub outcome: Option<String>,
}

/// What seats are reserved with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReservationAsking {
  pub reservation_key: String,
  pub owner: String,
  pub source: String,
  pub role: String,
  pub provider: String,
  pub model: String,
  pub effort: Option<String>,
  pub tier: String,
  /// How many seats.
  pub wanted: u32,
}

/// Where a reservation stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReservationState {
  Held,
  Released,
}

/// A reservation: its asking and where it stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Reservation {
  #[serde(flatten)]
  pub asking: ReservationAsking,
  pub token: String,
  pub state: ReservationState,
  pub created_at: i64,
}

/// A role the pool knows: what it is, whether it is on and lent as a co-worker, and its default tier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolRole {
  pub name: String,
  pub description: String,
  pub enabled: bool,
  /// True where the pool lends it as a co-worker.
  pub pooled: bool,
  /// The tier it runs at by default.
  pub tier: String,
}

/// The places one provider's tier holds in the pool: how many there are, and how many are leased,
/// reserved and asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolRoom {
  pub capacity: i64,
  pub active: i64,
  pub reserved: i64,
  pub waiting: i64,
}

/// The provider and model a place's settings were written for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaceWrite {
  pub provider: String,
  pub model: String,
  pub effort: Option<String>,
  pub upstream: Option<String>,
}
