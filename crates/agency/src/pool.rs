//! The co-worker pool: requests for a co-worker's place, reservations of seats, and the writes a place
//! was given. bmo-agency resolves the provider, model and effort a request or a reservation runs on itself,
//! from its role, its tier and the provider it names under the work key sent, and answers what it resolved.

use serde::{Deserialize, Serialize};

/// A pool request's asking: what it is enqueued with, and, as a request answers it, what bmo-agency
/// resolved it to. Of the asking only the key, the owner, the source, the role, the work key, the tier,
/// the provider, `yielding` and the place key are sent; a turn's is sent under the source `turn`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
  /// A reservation's seat, held for its owner's next request.
  Reserved,
  Active,
  /// Given back: released, or cancelled where it never ran.
  #[serde(rename = "released", alias = "cancelled")]
  Ended,
}

/// A pool request: its asking and where it stands.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PoolRequest {
  #[serde(flatten)]
  pub asking: PoolAsking,
  pub token: String,
  pub state: PoolState,
  pub enqueued_at: i64,
  pub leased_at: Option<i64>,
  /// The unit launched into its place.
  #[serde(rename = "childId")]
  pub child: Option<String>,
  pub ended_at: Option<i64>,
  pub outcome: Option<String>,
}

/// A reservation's asking: what seats are reserved with, and, as a reservation answers it, what bmo-agency
/// resolved them to. The model and effort are not sent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReservationAsking {
  pub reservation_key: String,
  pub owner: String,
  pub source: String,
  pub role: String,
  /// The work key the seats are resolved and kept under.
  pub work_key: String,
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
#[serde(rename_all = "camelCase")]
pub struct Reservation {
  #[serde(flatten)]
  pub asking: ReservationAsking,
  pub token: String,
  pub state: ReservationState,
  pub created_at: i64,
}

/// A role the pool knows: what it is, whether it is on and lent as a co-worker, and its default tier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
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
#[serde(rename_all = "camelCase")]
pub struct PoolRoom {
  pub capacity: i64,
  pub active: i64,
  pub reserved: i64,
  pub waiting: i64,
}

/// The provider and model a place's settings were written for.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceWrite {
  pub provider: String,
  pub model: String,
  pub effort: Option<String>,
  pub upstream: Option<String>,
}
