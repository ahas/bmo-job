//! Words added to an item by somebody other than the agent's own turn and the item's own job, told to the
//! agent: no worker reads a person's or a companion's words, so the agent folds them into the item's brief
//! and hands a running job the change. The fact is queue-rows'.

use job_codes::Fact;
use queue_rows::WordsAdded;

use super::Teller;
use crate::Refusal;

impl Teller<'_> {
  /// Words somebody else added to an item, told to the agent in the owner's chat as a turn of its own.
  pub async fn tell_words_added(&self, added: &WordsAdded<'_>) -> Result<(), Refusal> {
    self.tell_owner(Fact::Todo(added.fact())).await
  }
}
