//! Where a job's end meets the queue (docs/queue.md, "Telling the agent"): the agent told that a job ended
//! and what that means for its item, the queue's standing told at a moment that moved it, a job's report
//! delivered on its roads, and the sweep of the open job rows — a dead runner's row closed, the reports and
//! endings a runner left waiting taken, a circling job and a quiet one told; and words somebody else added
//! to an item. What to do with any of it is the agent's: nothing here names an item to launch.
//!
//! Each fact goes to the agent's session through bmo-receiver's instructions, as a fact of `job-codes`.

mod dead;
mod ending;
pub mod failure;
mod quiet;
mod report;
mod runner;
mod standing;
mod stopped;
mod words;

pub use dead::WAITING_ENDING;
pub use report::WAITING_REPORT;

use std::path::PathBuf;

use job_codes::Fact;
use job_policy::Policy;
use job_receiver::{Instruction, Said};
use sea_orm::DatabaseConnection;

pub use dead::{gone_report, stopped_job_report};
pub use quiet::{JobWait, WAIT_PREFIX, probe_target};
pub use report::outward_summary;
pub use standing::QUEUE_STALL_TELL_MS;

use super::running::Running;
use super::units::Units;
use super::workers::{Worker, workers};
use crate::Refusal;
use crate::events::Events;

/// The family the queue's instructions are listed under.
pub const JOB_SORT: &str = "job";

/// The event a fact of a job's ending is, by its item.
pub fn job_ended_event(todo_id: Option<i64>) -> String {
  todo_id.map_or_else(|| "job_ended".to_owned(), |todo| format!("job_ended:{todo}"))
}

/// What the queue's telling stands on: the database it reads and writes, the calls on other units, the
/// policy, the owner's chat its facts go into, where the workers stand by default, the service's own unit
/// whose journal a broken run is read in, the machine's `systemctl`, and the stream the screens hear the
/// queue move on.
pub struct Teller<'a> {
  pub db: &'a DatabaseConnection,
  pub units: &'a dyn Units,
  pub policy: &'a Policy,
  pub owner_chat: i64,
  pub source: PathBuf,
  pub service_unit: String,
  pub systemctl: Option<PathBuf>,
  pub events: Option<&'a Events>,
}

/// Where a room's own news goes: its work chat, and the room it names where that room's registration is
/// gone.
pub(crate) struct NewsRoom {
  pub chat: i64,
  pub removed: Option<i64>,
}

impl<'a> Teller<'a> {
  fn running(&self) -> Running<'a, DatabaseConnection> {
    Running {
      db: self.db,
      units: self.units,
      systemctl: self.systemctl.clone(),
    }
  }

  // A fact into a chat's session, for the agent named or the deployment's own, with the events it is and,
  // where it rides the next turn, until when.
  async fn tell_as(
    &self,
    agent: Option<String>,
    fact: Fact,
    events: Vec<String>,
    rides_until: Option<i64>,
    chat: i64,
  ) -> Result<(), Refusal> {
    let instruction = Instruction {
      agent,
      chat: Some(chat),
      said: Said::Fact(fact),
      kind: None,
      sort: JOB_SORT.to_owned(),
      summary: None,
      expires_at: None,
      events,
      rides_until,
      via: None,
      run: None,
    };

    self.units.queue_instruction(&instruction).await.map(drop)
  }

  async fn tell(
    &self,
    fact: Fact,
    events: Vec<String>,
    rides_until: Option<i64>,
    chat: i64,
  ) -> Result<(), Refusal> {
    self.tell_as(None, fact, events, rides_until, chat).await
  }

  // A fact into the owner's session, opening a turn and carrying no event.
  async fn tell_owner(&self, fact: Fact) -> Result<(), Refusal> {
    self.tell(fact, vec![], None, self.owner_chat).await
  }

  // The workers there are, the built-in standing whether its row is written or not.
  async fn workers(&self) -> Result<Vec<Worker>, Refusal> {
    workers(self.db, &self.source).await
  }

  // Where the agent's own news about a job filed from a chat goes, and the room it names where that room's
  // registration is gone.
  async fn news_room(&self, chat: i64) -> Result<NewsRoom, Refusal> {
    let room = self.units.room(chat).await?;

    Ok(NewsRoom {
      chat: room.work_chat,
      removed: (!room.registered && room.work_chat != chat).then_some(chat),
    })
  }

  // What every look at the queue does first: a dead runner's row closed.
  async fn upkeep(&self, now: i64) -> Result<(), Refusal> {
    self.close_dead(now).await.map(drop)
  }
}

// What a run said, cut to so many UTF-16 units, the measure the kept cause and line are cut by.
fn utf16_cut(text: &str, units: usize) -> &str {
  let mut seen = 0;

  for (at, character) in text.char_indices() {
    seen += character.len_utf16();

    if seen > units {
      return &text[..at];
    }
  }

  text
}
