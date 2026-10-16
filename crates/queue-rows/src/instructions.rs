//! An order queued for the deployment's agent in a room, through bmo-receiver (docs/api.md, "Instructions"):
//! the words of the order in English, which no person reads, its family and the line it is listed under.

use job_base::Refusal;
use job_receiver::{Instruction, Said};

use crate::Units;

/// The event a raised row's instruction carries, which the console reads as the queue having moved.
pub const QUEUE_CHANGED: &str = "queue_changed";

/// One order for the agent.
pub struct Order<'a> {
  /// The room, the owner's private chat where none.
  pub chat: Option<i64>,
  pub text: String,
  /// Its family, such as `raised_row`.
  pub sort: &'a str,
  /// The line it is listed under.
  pub summary: Option<String>,
  /// The events its delivery carries.
  pub events: Vec<String>,
}

/// The order queued; its instruction's id.
pub async fn queue_order(units: &dyn Units, order: Order<'_>) -> Result<i64, Refusal> {
  let instruction = Instruction {
    agent: None,
    chat: order.chat,
    said: Said::Order(order.text),
    kind: None,
    sort: order.sort.to_owned(),
    summary: order.summary,
    expires_at: None,
    events: order.events,
    rides_until: None,
    via: None,
    run: None,
  };

  units.queue_instruction(&instruction).await
}
