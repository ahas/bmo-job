//! The service's port to bmo-agency (docs/api.md, "bmo-agency"): every call it makes there, declared by
//! the trait `Agency` and made over HTTP by `AgencyClient`. The shapes the calls carry stand in one
//! module to an area.

pub mod client;
pub mod executions;
pub mod log;
pub mod machine;
pub mod nodes;
pub mod places;
pub mod pool;
pub mod records;
pub mod routes;
pub mod runs;
pub mod sessions;
pub mod workers;

use std::future::Future;

pub use client::AgencyClient;
pub use executions::Execution;
pub use log::{JobLines, LogLine, ToolCall, ToolResult};
pub use machine::{LaunchHeld, Room};
pub use nodes::{Node, SshReach, WorkMachine};
pub use places::{BranchSlots, Slot, Workbench};
pub use pool::{
  PlaceWrite, PoolAsking, PoolRequest, PoolState, Reservation, ReservationAsking, ReservationState,
};
pub use records::{TurnRecord, TurnRecordKind};
pub use routes::{Mapping, PinChoice, Provider, RouteAsk, Tier};
pub use runs::{Run, RunClosing, RunOpening};
pub use sessions::{Context, Delivery, Output, Scope, Session, SessionId, SessionState, Start};
pub use workers::{Pin, WorkerRuntime};

/// Why a call on bmo-agency failed. Each names the call it was.
#[derive(Debug, thiserror::Error)]
pub enum AgencyError {
  /// The address given is no `http:` address a path can be added to.
  #[error("{address} is not an http address bmo-agency can be reached at")]
  NotAnAddress { address: String },
  /// The call did not reach bmo-agency, or no answer came back.
  #[error("{call}: bmo-agency did not answer: {error}")]
  Unreachable { call: &'static str, error: String },
  /// bmo-agency refused the call, with its status, code and sentence as it gave them.
  #[error("{call}: bmo-agency refused with {status} {code}: {error}")]
  Refused {
    call: &'static str,
    status: u16,
    code: String,
    error: String,
  },
  /// bmo-agency answered with a body that does not read as the call's answer or as a refusal.
  #[error("{call}: bmo-agency answered {status} with a body that does not read: {error}")]
  Malformed {
    call: &'static str,
    status: u16,
    error: String,
  },
}

/// What a call on bmo-agency comes back with.
pub type Answer<T> = Result<T, AgencyError>;

/// Every call the service makes on bmo-agency. A read of one thing by its id answers `None` where
/// bmo-agency has no such thing.
pub trait Agency: Send + Sync {
  /// Starts a session.
  fn start_session(&self, start: &Start) -> impl Future<Output = Answer<Session>> + Send;

  /// Sends a session one turn.
  fn send_turn(&self, session: &SessionId, prompt: &str) -> impl Future<Output = Answer<Delivery>> + Send;

  /// Reads what a session runs on now.
  fn session_state(&self, session: &SessionId)
  -> impl Future<Output = Answer<Option<SessionState>>> + Send;

  /// Reads what a session has written in its latest turn so far.
  fn session_output(&self, session: &SessionId) -> impl Future<Output = Answer<Option<Output>>> + Send;

  /// Interrupts a session's turn.
  fn interrupt_session(&self, session: &SessionId) -> impl Future<Output = Answer<()>> + Send;

  /// Folds a session's context.
  fn fold_session(&self, session: &SessionId) -> impl Future<Output = Answer<()>> + Send;

  /// Ends a session.
  fn end_session(&self, session: &SessionId) -> impl Future<Output = Answer<()>> + Send;

  /// Lists the sessions.
  fn sessions(&self) -> impl Future<Output = Answer<Vec<Session>>> + Send;

  /// Lists the executions, only those not yet ended where `open_only`.
  fn executions(&self, open_only: bool) -> impl Future<Output = Answer<Vec<Execution>>> + Send;

  /// Reads one execution.
  fn execution(&self, id: &str) -> impl Future<Output = Answer<Option<Execution>>> + Send;

  /// Asks an execution cancelled.
  fn cancel_execution(&self, id: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Drops the cancel asked of an execution and of every one it covers, answering those dropped.
  fn drop_cancels(&self, id: &str) -> impl Future<Output = Answer<Vec<String>>> + Send;

  /// Opens a run, answering its id.
  fn open_run(&self, opening: &RunOpening) -> impl Future<Output = Answer<i64>> + Send;

  /// Closes a run.
  fn close_run(&self, run: i64, closing: &RunClosing) -> impl Future<Output = Answer<()>> + Send;

  /// Reads a run.
  fn run(&self, run: i64) -> impl Future<Output = Answer<Option<Run>>> + Send;

  /// Counts the starts an agent's run has logged.
  fn run_starts(&self, agent: &str, run: i64) -> impl Future<Output = Answer<u64>> + Send;

  /// Reads the last line a job's run logged.
  fn job_last_line(&self, job: i64) -> impl Future<Output = Answer<Option<LogLine>>> + Send;

  /// Reads what a job's run logged.
  fn job_lines(&self, job: i64) -> impl Future<Output = Answer<JobLines>> + Send;

  /// Keeps one thing a run wrote down under the run's name.
  fn keep_turn_record(&self, record: &TurnRecord) -> impl Future<Output = Answer<()>> + Send;

  /// Reads a worker's runtime.
  fn worker_runtime(&self, worker: &str) -> impl Future<Output = Answer<Option<WorkerRuntime>>> + Send;

  /// Pins a worker to a provider, and to a model where the pin names one.
  fn pin_worker(&self, worker: &str, pin: &Pin) -> impl Future<Output = Answer<()>> + Send;

  /// Takes a worker's own pin away.
  fn unpin_worker(&self, worker: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Pauses a worker's keepalive from `at`.
  fn pause_keepalive(&self, worker: &str, at: i64) -> impl Future<Output = Answer<()>> + Send;

  /// Resumes a worker's keepalive.
  fn resume_keepalive(&self, worker: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Records the context a worker carried last.
  fn record_context(&self, worker: &str, tokens: u64) -> impl Future<Output = Answer<()>> + Send;

  /// Lists the providers.
  fn providers(&self) -> impl Future<Output = Answer<Vec<Provider>>> + Send;

  /// Lists the model tiers.
  fn tiers(&self) -> impl Future<Output = Answer<Vec<Tier>>> + Send;

  /// Resolves a route to a model, `None` where no route serves it.
  fn resolve_route(&self, asked: &RouteAsk) -> impl Future<Output = Answer<Option<Mapping>>> + Send;

  /// Reads what a pin's value chooses.
  fn check_pin(&self, value: &str) -> impl Future<Output = Answer<PinChoice>> + Send;

  /// Enqueues a pool request.
  fn enqueue_request(&self, asking: &PoolAsking) -> impl Future<Output = Answer<PoolRequest>> + Send;

  /// Reads a pool request by its token.
  fn pool_request(&self, token: &str) -> impl Future<Output = Answer<Option<PoolRequest>>> + Send;

  /// Lists the pool requests waiting or active, only the owner's where one is named.
  fn open_requests(&self, owner: Option<&str>) -> impl Future<Output = Answer<Vec<PoolRequest>>> + Send;

  /// Leases a pool request its place where the pool has room.
  fn lease_place(&self, token: &str) -> impl Future<Output = Answer<PoolRequest>> + Send;

  /// Binds the child that runs in a pool request's place.
  fn bind_child(&self, token: &str, child: &str) -> impl Future<Output = Answer<PoolRequest>> + Send;

  /// Finishes a pool request, answering false where it had already ended.
  fn finish_request(
    &self,
    token: &str,
    outcome: &str,
    cancel: bool,
  ) -> impl Future<Output = Answer<bool>> + Send;

  /// Reserves seats in the pool.
  fn reserve_seats(&self, asking: &ReservationAsking) -> impl Future<Output = Answer<Reservation>> + Send;

  /// Lists the reservations held, only the owner's where one is named.
  fn held_reservations(&self, owner: Option<&str>)
  -> impl Future<Output = Answer<Vec<Reservation>>> + Send;

  /// Releases a reservation, answering false where it was no longer held.
  fn release_reservation(&self, token: &str, reason: &str) -> impl Future<Output = Answer<bool>> + Send;

  /// Records what a place's settings were written for.
  fn record_place_write(&self, place: &str, write: &PlaceWrite) -> impl Future<Output = Answer<()>> + Send;

  /// Lists the nodes.
  fn nodes(&self) -> impl Future<Output = Answer<Vec<Node>>> + Send;

  /// Reads how SSH reaches a node.
  fn ssh_reach(&self, node: &str) -> impl Future<Output = Answer<Option<SshReach>>> + Send;

  /// Places the next heavy run, `None` where no machine has room.
  fn place_heavy_run(&self) -> impl Future<Output = Answer<Option<WorkMachine>>> + Send;

  /// Whether a user holds an enabled Claude account.
  fn claude_account_held(&self, user: &str) -> impl Future<Output = Answer<bool>> + Send;

  /// Reads the room on the machine.
  fn machine_room(&self) -> impl Future<Output = Answer<Room>> + Send;

  /// Tells bmo-agency launches are held, and why.
  fn tell_launch_held(&self, notice: &LaunchHeld) -> impl Future<Output = Answer<()>> + Send;

  /// Reads a worker's workbench.
  fn workbench(&self, worker: &str) -> impl Future<Output = Answer<Workbench>> + Send;

  /// Reads a pool's branch slots.
  fn branch_slots(&self, pool: &str) -> impl Future<Output = Answer<BranchSlots>> + Send;
}
