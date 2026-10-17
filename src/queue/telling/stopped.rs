//! An item the owner stopped, told to the agent: the stop sets the item aside and ends its runs, which the
//! agent hears of in the owner's chat as one fact rather than reading it off the queue.

use job_codes::{Fact, QueueFact};

use super::Teller;
use crate::Refusal;

impl Teller<'_> {
  /// The owner's stop of an item, told to the agent in the owner's chat as a turn of its own.
  pub async fn tell_owner_stopped(&self, stopped: QueueFact) -> Result<(), Refusal> {
    self.tell_owner(Fact::Queue(stopped)).await
  }
}
