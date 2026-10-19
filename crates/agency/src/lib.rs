//! The service's port to bmo-agency (docs/api.md, "bmo-agency"): every call it makes there, declared by
//! the trait `Agency` and made over HTTP by `AgencyClient`, and `WorkTurns`, which carries a work turn on the
//! session contract. The shapes the calls carry stand in one module to an area.

pub mod client;
pub mod executions;
pub mod hooks;
pub mod log;
pub mod machine;
pub mod nodes;
pub mod places;
pub mod pool;
pub mod records;
pub mod routes;
pub mod runs;
pub mod sessions;
pub mod turns;
pub mod work;
pub mod workers;

use std::future::Future;

use futures_util::stream::BoxStream;

pub use client::AgencyClient;
pub use executions::Execution;
pub use hooks::HookPoint;
pub use log::{JobLines, LogLine, LogNote, ToolCall, ToolResult};
pub use machine::{LaunchHeld, Room, RoomReturned};
pub use nodes::{Node, SshReach};
pub use places::{HeldPlace, PlaceAsk, PlaceRecord, PlacesRetirement, TakenPlace, Workbench};
pub use pool::{
  PlaceWrite, PoolAsking, PoolRequest, PoolRole, PoolRoom, PoolState, Reservation, ReservationAsking,
  ReservationState,
};
pub use records::{TurnRecord, TurnRecordKind};
pub use routes::{Mapping, PinChoice, Provider, RouteAsk, Tier};
pub use runs::{Run, RunClosing, RunOpening};
pub use sessions::{
  Context, Delivery, EngineError, Output, Scope, Session, SessionId, SessionState, Start, Turn,
  TurnContext, TurnEnd, TurnOutcome,
};
pub use turns::{
  AccountFailure, AsideOutcome, ConfirmAnswer, Confirmation, Handover, JobDocuments, LiveTask, ModelSpend,
  Prepared, Question, Reply, RequestUsage, Resume, RunEnding, RunWorkflow, TurnAsking, TurnFailure,
  TurnMapping, TurnSeat, TurnSpan, TurnState, WentOut, WorkParts, WorkTurn,
};
pub use work::WorkTurns;
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
  /// The session contract does not carry what the call asks, so nothing was sent.
  #[error("{call}: {why}")]
  Uncarried { call: &'static str, why: String },
}

/// What a call on bmo-agency comes back with.
pub type Answer<T> = Result<T, AgencyError>;

/// Each time bmo-agency says the machine's room returned, for as long as its stream stays open; a failure
/// reading it ends the stream.
pub type RoomReturns = BoxStream<'static, Answer<RoomReturned>>;

/// Every call the service makes on bmo-agency. A read of one thing by its id answers `None` where
/// bmo-agency has no such thing.
pub trait Agency: Send + Sync {
  /// Starts a session.
  fn start_session(&self, start: &Start) -> impl Future<Output = Answer<Session>> + Send;

  /// Sends a session one turn, answered with how it ended once it has.
  fn send_turn(&self, turn: &Turn) -> impl Future<Output = Answer<TurnEnd>> + Send;

  /// Pushes words into the turn a session is working.
  fn push_words(
    &self,
    session: &SessionId,
    words: &str,
  ) -> impl Future<Output = Answer<AsideOutcome>> + Send;

  /// Reads what a session runs on now; read for `model`, a worker's state also names the session a turn on that
  /// model would continue.
  fn session_state(
    &self,
    session: &SessionId,
    model: Option<&str>,
  ) -> impl Future<Output = Answer<Option<SessionState>>> + Send;

  /// Answers the question a session's turn stands on, by the question's id.
  fn answer_question(
    &self,
    session: &SessionId,
    question: &str,
    answer: &ConfirmAnswer,
  ) -> impl Future<Output = Answer<()>> + Send;

  /// Reads what a session's turn now going, or its last one, has written so far.
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

  /// Asks a run stopped by its name, a unit's or an execution's: everything it owns cancelled, and the unit
  /// stopped where it still stands.
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
  fn run_starts(&self, agent: &str, run: &str) -> impl Future<Output = Answer<u64>> + Send;

  /// Counts the tool calls an agent's run has logged.
  fn run_tool_calls(&self, agent: &str, run: &str) -> impl Future<Output = Answer<u64>> + Send;

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

  /// Stops a worker's keepalive.
  fn pause_keepalive(&self, worker: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Resumes a worker's keepalive.
  fn resume_keepalive(&self, worker: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Says the owner's words to a worker, for its session.
  fn say_to_worker(&self, worker: &str, text: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Lists the providers.
  fn providers(&self) -> impl Future<Output = Answer<Vec<Provider>>> + Send;

  /// Lists the model tiers.
  fn tiers(&self) -> impl Future<Output = Answer<Vec<Tier>>> + Send;

  /// Lists the roles the pool knows.
  fn pool_roles(&self) -> impl Future<Output = Answer<Vec<PoolRole>>> + Send;

  /// Reads the places one provider's tier holds in the pool, and how many are taken.
  fn pool_room(&self, provider: &str, tier: &str) -> impl Future<Output = Answer<PoolRoom>> + Send;

  /// Takes a branch's place in its order's pool of standing checkouts.
  fn take_place(&self, ask: &PlaceAsk) -> impl Future<Output = Answer<TakenPlace>> + Send;

  /// Lays an order's checkout, its head and its open change, into a place's directory.
  fn prepare_place(&self, source: &str, cwd: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Records what a finished branch left in its place.
  fn remember_place(&self, record: &PlaceRecord) -> impl Future<Output = Answer<()>> + Send;

  /// Takes down what stands for the places an ended order worked in that no task sits in now.
  fn retire_places(&self, retirement: &PlacesRetirement) -> impl Future<Output = Answer<()>> + Send;

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

  /// Lets a pool request into its place where its turn has come, `None` while it waits on.
  fn lease_place(&self, token: &str) -> impl Future<Output = Answer<Option<PoolRequest>>> + Send;

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

  /// Whether a user holds an enabled Claude account.
  fn claude_account_held(&self, user: &str) -> impl Future<Output = Answer<bool>> + Send;

  /// Lists the directories the logins keep their session records under, the machine's own first.
  fn claude_homes(&self) -> impl Future<Output = Answer<Vec<String>>> + Send;

  /// Whether the deployment rests now, which closes Claude Code's road to a job.
  fn resting(&self) -> impl Future<Output = Answer<bool>> + Send;

  /// Reads the room on the machine.
  fn machine_room(&self) -> impl Future<Output = Answer<Room>> + Send;

  /// Tells bmo-agency launches are held, and why.
  fn tell_launch_held(&self, notice: &LaunchHeld) -> impl Future<Output = Answer<()>> + Send;

  /// Hears bmo-agency say the machine's room returned, each time from the moment the stream opens until
  /// it ends.
  fn room_returns(&self) -> impl Future<Output = Answer<RoomReturns>> + Send;

  /// Reads a worker's workbench.
  fn workbench(&self, worker: &str) -> impl Future<Output = Answer<Workbench>> + Send;

  /// Hands a worker's session over.
  fn hand_over_session(&self, worker: &str, handover: &Handover)
  -> impl Future<Output = Answer<()>> + Send;

  /// Notes a run of a worker's.
  fn note_run(&self, worker: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Makes a worker's bench ready for a job.
  fn make_bench(&self, worker: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Sweeps the servers a worker's jobs left listening, answering what was swept.
  fn sweep_servers(&self, worker: &str) -> impl Future<Output = Answer<Vec<String>>> + Send;

  /// Reads what a worker's agent definition gives a job.
  fn job_documents(&self, worker: &str) -> impl Future<Output = Answer<JobDocuments>> + Send;

  /// Which of the files touch the prefix a worker's session is opened on.
  fn prefix_touched(
    &self,
    worker: &str,
    files: &[String],
  ) -> impl Future<Output = Answer<Vec<String>>> + Send;

  /// Whether the engine behind a registered model answers its health and stands aside for nothing.
  fn model_health(&self, provider: &str, model: &str) -> impl Future<Output = Answer<bool>> + Send;

  /// Rests a Claude login until the moment given.
  fn rest_login(&self, account: &str, until: i64) -> impl Future<Output = Answer<()>> + Send;

  /// Begins the rest a done job's session usage calls for.
  fn usage_rest(&self, account: &str, usage: &serde_json::Value)
  -> impl Future<Output = Answer<()>> + Send;

  /// Writes a note into a run's log.
  fn write_note(&self, note: &LogNote) -> impl Future<Output = Answer<()>> + Send;

  /// Where the copy of a node's checkout is kept on this machine.
  fn node_copy(&self, node: &str, checkout: &str) -> impl Future<Output = Answer<String>> + Send;

  /// Lists the hook points and whether one is registered at each.
  fn hook_points(&self) -> impl Future<Output = Answer<Vec<HookPoint>>> + Send;

  /// Registers an address at a hook point, in place of any before.
  fn register_hook(&self, point: &str, url: &str) -> impl Future<Output = Answer<()>> + Send;
}
