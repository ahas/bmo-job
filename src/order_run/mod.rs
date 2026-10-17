//! The work-order runner (docs/queue.md, "The work-order runner"): its tick, standing on its own clock, and
//! the pieces each tick stands on — what a step is handed, a step launched, the branch places, the checkout
//! an order holds, its stops and retries, and the order's seats and places in bmo-agency's co-worker pool.

pub mod check;
pub mod context;
pub mod hold;
pub mod launch;
pub mod places;
pub mod pool;
pub mod standing;
pub mod stops;
pub mod tick;

use std::path::PathBuf;

use job_codes::{Fact, WorkOrderStop};
use job_policy::Policy;
use job_receiver::{Instruction, Said};
use sea_orm::DatabaseConnection;

use crate::queue::job_start::Host;
use crate::queue::running::Running;
use crate::queue::units::Units;

/// Why a step of the runner did not go through, as the order's record keeps it: one English sentence.
pub type Fault = String;

/// The family the runner's facts are listed under.
pub const WORK_ORDER_SORT: &str = "work_order";

/// What the runner stands on: the database, the calls on other units, the policy, the host a step's unit is
/// started on, the folder a work order's files are written under, the tree a builder turn adds steps from,
/// the checkout the built-in worker works in, the machine's `systemctl`, and the owner's chat its facts go
/// into.
pub struct Ground<'a> {
  pub db: &'a DatabaseConnection,
  pub units: &'a dyn Units,
  pub policy: &'a Policy,
  pub host: &'a Host,
  pub documents: PathBuf,
  pub tree: PathBuf,
  pub source: PathBuf,
  pub systemctl: Option<PathBuf>,
  pub owner_chat: i64,
}

impl<'a> Ground<'a> {
  /// The reading of what runs.
  pub fn running(&self) -> Running<'a, DatabaseConnection> {
    Running {
      db: self.db,
      units: self.units,
      systemctl: self.systemctl.clone(),
    }
  }

  /// The folder an item's work order keeps its prompts, result documents and logs in.
  pub fn order_root(&self, todo: i64) -> PathBuf {
    self.documents.join(todo.to_string())
  }

  /// A fact told to the agent in the owner's chat; a failure to queue it is the receiver's, written to the
  /// service's log and never the runner's.
  pub async fn tell(&self, stop: WorkOrderStop) {
    let instruction = Instruction {
      agent: None,
      chat: Some(self.owner_chat),
      said: Said::Fact(Fact::WorkOrder(stop)),
      kind: None,
      sort: WORK_ORDER_SORT.to_owned(),
      summary: None,
      expires_at: None,
      events: vec![],
      rides_until: None,
      via: None,
      run: None,
    };

    if let Err(failed) = self.units.queue_instruction(&instruction).await {
      eprintln!("bmo-job: a work order's fact was not queued: {}", failed.error);
    }
  }
}

/// A refusal of the service or a fault of a call on another unit, as the runner keeps it.
pub fn faulted(why: impl Into<crate::Refusal>) -> Fault {
  why.into().error
}
