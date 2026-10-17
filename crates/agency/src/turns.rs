//! Work turns: one turn of a job's work carried by bmo-agency end to end, what it answered or how it failed,
//! and what a worker's sessions keep of it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Who a work turn runs as: the worker on its own session, or a co-work branch on one thrown away.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnSeat {
  Worker,
  Branch {
    todo: Option<i64>,
    role: Option<String>,
  },
}

/// Which session a work turn goes on: the worker's kept one, a fresh one, or the one named.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Resume {
  Auto,
  Fresh,
  Session { id: String },
}

/// The mapping a work turn's line is laid on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnMapping {
  pub model: String,
  pub effort: Option<String>,
  pub tier: Option<String>,
  pub answer_tokens: Option<u64>,
}

/// What a work turn is prepared with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnAsking {
  pub agent: String,
  pub seat: TurnSeat,
  pub provider: String,
  /// The model the worker's session record is kept under.
  pub model: String,
  pub mapping: TurnMapping,
  pub resume: Resume,
  pub cwd: String,
  /// The run's name.
  pub name: String,
  /// What a person watching reads over the turn.
  pub title: String,
  pub thread: Option<String>,
  /// The job the turn is of; none for a research run's turn.
  pub job: Option<i64>,
  pub chat: i64,
  pub unit: Option<String>,
  pub user: Option<String>,
  pub work_key: String,
  /// Whether words said to the worker are pushed into the turn.
  pub hears_words: bool,
  /// The variables the program the turn starts is started with beside its own.
  #[serde(default)]
  pub variables: BTreeMap<String, String>,
}

/// A work turn prepared: its id, and the session it will continue, none where it opens a fresh one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prepared {
  pub turn: String,
  pub session: Option<String>,
}

/// A work turn as it stands.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorkTurn {
  pub turn: String,
  #[serde(flatten)]
  pub state: TurnState,
  #[serde(default)]
  pub spans: Vec<TurnSpan>,
  #[serde(default)]
  pub compacted: bool,
  pub session: Option<String>,
  pub went_out: Option<WentOut>,
}

/// Where a work turn stands.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum TurnState {
  Running {
    question: Option<Question>,
  },
  /// The walk moved to a login whose session differs from the one prepared: the turn waits for its prompt
  /// again.
  Reprepared,
  Answered {
    reply: Box<Reply>,
  },
  Failed {
    failure: Option<TurnFailure>,
    evidence: Option<String>,
  },
}

/// What a work turn went out on, once known.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WentOut {
  pub provider: String,
  pub model: String,
  pub effort: Option<String>,
  pub tier: Option<String>,
}

/// A question the program stopped on, waiting for an answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Question {
  pub id: String,
  #[serde(flatten)]
  pub kind: Confirmation,
}

/// What the program asks: leave to run a command, or something this reading does not know.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Confirmation {
  Permission { text: String, amendable: bool },
  Unknown,
}

/// The answer a question takes: let it go on, leave the question standing for the pane, or refuse it with
/// the words given.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ConfirmAnswer {
  Allow,
  Leave,
  Refuse { words: Option<String> },
}

/// What came of words pushed into a running turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsideOutcome {
  Delivered,
  /// The run waits on a person and took nothing.
  Waiting,
  Refused,
}

/// One span of where a turn's time or spending went.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TurnSpan {
  pub kind: String,
  pub name: String,
  pub seconds: f64,
  pub times: i64,
  pub tokens: Option<i64>,
  pub input_tokens: Option<i64>,
  pub output_tokens: Option<i64>,
  pub cache_read_tokens: Option<i64>,
  pub cache_write_tokens: Option<i64>,
}

/// What a turn answered.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply {
  pub mode: String,
  pub model: Option<String>,
  pub effort: Option<String>,
  pub text: String,
  pub session_id: Option<String>,
  pub account: Option<String>,
  pub context_ratio: Option<f64>,
  pub context_tokens: Option<i64>,
  /// The session usage the run reported, as it stands.
  pub session_usage: Option<Value>,
  #[serde(default)]
  pub spend: Vec<ModelSpend>,
  #[serde(default)]
  pub live_tasks: Vec<LiveTask>,
  #[serde(default)]
  pub workflows: Vec<RunWorkflow>,
  pub upstream: Option<String>,
  pub request_usages: Option<Vec<RequestUsage>>,
}

/// What one model of a run spent.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelSpend {
  pub mode: Option<String>,
  pub model: String,
  pub tokens: i64,
  pub input_tokens: i64,
  pub output_tokens: i64,
  pub cache_read_tokens: i64,
  pub cache_write_tokens: i64,
}

/// A background task of the run's own, still running when the turn ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LiveTask {
  pub id: String,
  pub description: String,
}

/// A workflow the run launched, with how it ended where it did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunWorkflow {
  pub call: String,
  pub name: String,
  pub status: Option<String>,
}

/// What one model request of a turn spent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestUsage {
  pub input_tokens: i64,
  pub output_tokens: i64,
  pub cache_read_tokens: i64,
  pub cache_write_tokens: i64,
}

/// What a failed turn's login said of itself rather than of the work.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountFailure {
  /// The login's window is full; another login answers and this one rests until its window resets.
  Limit,
  /// The login is gone; nobody runs on it until a person signs in again.
  Login,
  /// The service itself would not answer, and no other login would either.
  Outage,
  /// The account's token for the work nodes did not answer on a node.
  NodeToken,
}

/// How a run that died ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunEnding {
  Exit { code: i64 },
}

/// A failed turn, as bmo-agency read it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TurnFailure {
  Limit {
    resets_at: Option<i64>,
    account: Option<String>,
    #[serde(default)]
    silenced: bool,
  },
  Account {
    failure: AccountFailure,
    detail: String,
  },
  Died {
    ending: RunEnding,
    said: Option<String>,
    message: String,
  },
  Person,
  Untrusted {
    folder: String,
    config: String,
  },
  Stopped,
  Transport {
    message: String,
  },
  Refused {
    message: String,
  },
  Unavailable {
    scope: String,
    mode: String,
    reason: String,
  },
  Error {
    name: String,
    message: String,
  },
}

/// The session a job's last answer came back on, as a worker's records keep it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SessionAnswered {
  pub mode: String,
  pub session_id: Option<String>,
  pub account: Option<String>,
  #[serde(default)]
  pub spend: Vec<ModelSpend>,
  pub context_ratio: Option<f64>,
  pub context_tokens: Option<i64>,
  /// The model the job's last attempt went out on.
  pub model: String,
}

/// What became of a session's record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKept {
  Recorded,
  /// Recorded, and past the fold's line.
  Full,
  /// It answered on a lesser model, so it was dropped.
  Dropped,
  None,
}

/// A session handed over: the kept one on the model where none is named, its record dropped after.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Handover {
  pub model: String,
  pub cwd: String,
  pub thread: Option<String>,
  pub session: Option<String>,
  pub note: Option<String>,
}

/// What a worker's agent definition gives a job: what it is told of the code graph, the trailer every job is
/// held to, and the instruction its own handoff note is written by where its folder holds one.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobDocuments {
  pub graph: String,
  pub trailer: String,
  pub handoff: Option<String>,
}
