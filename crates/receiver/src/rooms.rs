//! Rooms: the owner, what a room is to the deployment, the agents who answer in it, and its messages.

use serde::{Deserialize, Serialize};

/// The deployment's owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
  /// The owner as a sender.
  pub sender: i64,
  /// The owner's private chat.
  pub chat: i64,
}

/// What a room is to the deployment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Room {
  pub chat: i64,
  /// Whether it is the owner's private chat with the deployment's agent.
  pub owner_room: bool,
  /// Whether its registration stands.
  pub registered: bool,
  /// Where the work filed from it is told: the room itself while its registration stands, the owner's
  /// private chat once it is gone.
  pub work_chat: i64,
}

/// The agents who answer in a room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answerers {
  /// The agent that answers where a room names none.
  pub default: String,
  /// The agent that speaks for the room.
  pub voice: String,
  /// Every agent named to answer in the room that is not resting.
  pub answerers: Vec<Answerer>,
}

/// One agent named to answer in a room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answerer {
  pub agent: String,
  /// Whether it answers beside the voice rather than as it.
  pub companion: bool,
}

/// One message said in a room.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
  /// Its sender, none where no sender is known.
  pub sender: Option<i64>,
  pub sender_name: Option<String>,
  pub sent_at: i64,
  pub text: String,
}
