//! Room holdings: the service declared to bmo-receiver as a unit that holds rows naming a room.

use serde::{Deserialize, Serialize};

/// The service's declaration as a holder.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holder {
  /// The base of the service's own API, which bmo-receiver asks what a room holds.
  pub address: String,
  pub holdings: Vec<Holding>,
}

/// One kind of row the service holds naming a room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Holding {
  pub name: String,
  pub effect: Effect,
}

/// What a holding does as its room is removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
  /// The rows stay and keep the room's chat after it is removed.
  Kept,
  /// bmo-receiver shows what the service answers before it removes the room.
  Warns,
}
