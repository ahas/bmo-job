//! The service's verbs as the server is built on them (docs/api.md, "Routes"): one trait to an area, each
//! taking the typed request its routes read and answering what its routes answer. The verb code carries
//! them; `Unbuilt` stands for a build that carries none, answering every verb `not_built`. Beside them stand
//! the verbs a job runs from its own shell and a person runs at one (docs/verbs.md), each on the stand.

pub mod ask;
pub mod borrow;
pub mod branch_split;
pub mod checkout_setup;
pub mod cowork_timing;
pub mod job;
pub mod job_close;
pub mod job_figures;
pub mod job_guard;
pub mod land;
pub mod live_claude;
pub mod node_land;
pub mod overtime;
pub mod research;
pub mod stand;
pub mod task;
pub mod todo;
pub mod tool_calls;
pub mod toolbook;
pub mod waiting;
pub mod work_order;

pub mod jobs;
pub mod pool;
pub mod questions;
pub mod runners;
pub mod stops;
pub mod tasks;
pub mod todos;
pub mod words;
pub mod work_orders;
pub mod workers;

use std::future::{Ready, ready};

use job_base::Refusal;
use job_codes::{ControlRefusal, JobError};

pub use jobs::{JobEnded, Moment, Telling};
pub use pool::{Board, PoolBoard};
pub use questions::{Questions, Settling};
pub use runners::{RunnerLaunch, Runners};
pub use stops::{RunStops, Stop, Target, TargetKind};
pub use tasks::{TaskLaunch, Tasks};
pub use todos::{TodoAction, TodoChanged, Todos};
pub use words::{WordFile, WorkerWord, WorkerWords};
pub use work_orders::{OrderAction, WorkOrders};
pub use workers::{NodeChange, NodeCheckout, WorkerAdd, WorkerAdded, WorkerUpdate, Workers};

/// What a verb answers: what it came to, or why it refused.
pub type Done<T> = Result<T, Refusal>;

/// Every verb the server is built on.
pub trait Verbs:
  Telling
  + Runners
  + PoolBoard
  + Questions
  + RunStops
  + Tasks
  + Todos
  + WorkOrders
  + Workers
  + WorkerWords
  + Send
  + Sync
  + 'static
{
}

impl<T> Verbs for T where
  T: Telling
    + Runners
    + PoolBoard
    + Questions
    + RunStops
    + Tasks
    + Todos
    + WorkOrders
    + Workers
    + WorkerWords
    + Send
    + Sync
    + 'static
{
}

/// The verbs of a build that carries none: every one is refused `not_built`, naming the verb.
#[derive(Clone, Copy, Debug, Default)]
pub struct Unbuilt;

impl Unbuilt {
  pub(crate) fn refused<T>(verb: &str) -> Ready<Done<T>> {
    ready(Err(
      JobError::from(ControlRefusal::NotBuilt {
        verb: verb.to_owned(),
      })
      .into(),
    ))
  }
}
