//! The port's HTTP client: each call made on the method and path the API document gives it, its body
//! written and its answer read as JSON.

use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use url::Url;

use crate::{
  Agency, AgencyError, Answer, BranchSlots, Delivery, Execution, JobLines, LaunchHeld, LogLine, Mapping,
  Node, Output, Pin, PinChoice, PlaceWrite, PoolAsking, PoolRequest, PoolRole, Provider, Reservation,
  ReservationAsking, Room, RouteAsk, Run, RunClosing, RunOpening, Session, SessionId, SessionState,
  SshReach, Start, Tier, TurnRecord, WorkMachine, Workbench, WorkerRuntime,
};

/// The calls on bmo-agency, made over HTTP to the address given.
#[derive(Clone, Debug)]
pub struct AgencyClient {
  base: Url,
  http: Client,
}

/// A refusal's body, as "A refused request" in the API document gives it.
#[derive(Deserialize)]
struct Said {
  error: String,
  code: String,
}

#[derive(Deserialize)]
struct Delivered {
  delivery: Delivery,
}

#[derive(Deserialize)]
struct Opened {
  id: i64,
}

#[derive(Deserialize)]
struct Counted {
  count: u64,
}

#[derive(Deserialize)]
struct Dropped {
  dropped: Vec<String>,
}

#[derive(Deserialize)]
struct Finished {
  finished: bool,
}

#[derive(Deserialize)]
struct Released {
  released: bool,
}

#[derive(Deserialize)]
struct Cache {
  ttl_ms: Option<i64>,
}

#[derive(Deserialize)]
struct Digest {
  digest: String,
}

#[derive(Deserialize)]
struct Held {
  held: bool,
}

#[derive(Deserialize)]
struct Rest {
  resting: bool,
}

#[derive(Deserialize)]
struct Placed {
  machine: Option<WorkMachine>,
}

/// The failure a status that is no success stands for: bmo-agency's refusal where its body reads as one.
fn refused(call: &'static str, status: StatusCode, body: &[u8]) -> AgencyError {
  match serde_json::from_slice::<Said>(body) {
    Ok(said) => AgencyError::Refused {
      call,
      status: status.as_u16(),
      code: said.code,
      error: said.error,
    },
    Err(error) => AgencyError::Malformed {
      call,
      status: status.as_u16(),
      error: error.to_string(),
    },
  }
}

fn unreachable(call: &'static str) -> impl FnOnce(reqwest::Error) -> AgencyError {
  move |error| AgencyError::Unreachable {
    call,
    error: error.to_string(),
  }
}

/// A response's body read as `T`, or the failure its status stands for.
async fn read<T: DeserializeOwned>(call: &'static str, response: Response) -> Answer<T> {
  let status = response.status();
  let body = response.bytes().await.map_err(unreachable(call))?;

  if !status.is_success() {
    return Err(refused(call, status, &body));
  }

  serde_json::from_slice(&body).map_err(|error| AgencyError::Malformed {
    call,
    status: status.as_u16(),
    error: error.to_string(),
  })
}

impl AgencyClient {
  /// A client of bmo-agency at `base`, which must be an `http:` address.
  pub fn new(base: Url) -> Answer<Self> {
    if base.scheme() != "http" || base.cannot_be_a_base() {
      return Err(AgencyError::NotAnAddress {
        address: base.to_string(),
      });
    }

    Ok(Self {
      base,
      http: Client::new(),
    })
  }

  /// The address the client calls.
  pub fn address(&self) -> &Url {
    &self.base
  }

  /// A request on the path made of `segments` under the address, each percent-encoded, with `query`.
  fn request(&self, method: Method, segments: &[&str], query: &[(&str, &str)]) -> RequestBuilder {
    let mut url = self.base.clone();

    if let Ok(mut path) = url.path_segments_mut() {
      path.pop_if_empty().extend(segments);
    }

    if !query.is_empty() {
      url.query_pairs_mut().extend_pairs(query);
    }

    self.http.request(method, url)
  }

  async fn send(&self, call: &'static str, request: RequestBuilder) -> Answer<Response> {
    request.send().await.map_err(unreachable(call))
  }

  /// A call answered with a body.
  async fn answer<T: DeserializeOwned>(&self, call: &'static str, request: RequestBuilder) -> Answer<T> {
    read(call, self.send(call, request).await?).await
  }

  /// A read of one thing by its id, where 404 is nothing found.
  async fn found<T: DeserializeOwned>(
    &self,
    call: &'static str,
    request: RequestBuilder,
  ) -> Answer<Option<T>> {
    let response = self.send(call, request).await?;

    if response.status() == StatusCode::NOT_FOUND {
      return Ok(None);
    }

    read(call, response).await.map(Some)
  }

  /// A call answered with no body.
  async fn done(&self, call: &'static str, request: RequestBuilder) -> Answer<()> {
    let response = self.send(call, request).await?;
    let status = response.status();

    if status.is_success() {
      return Ok(());
    }

    let body = response.bytes().await.map_err(unreachable(call))?;

    Err(refused(call, status, &body))
  }
}

impl Agency for AgencyClient {
  async fn start_session(&self, start: &Start) -> Answer<Session> {
    let request = self.request(Method::POST, &["sessions"], &[]).json(start);

    self.answer("start_session", request).await
  }

  async fn send_turn(&self, session: &SessionId, prompt: &str) -> Answer<Delivery> {
    let request = self
      .request(Method::POST, &["sessions", &session.0, "turns"], &[])
      .json(&json!({ "prompt": prompt }));
    let delivered: Delivered = self.answer("send_turn", request).await?;

    Ok(delivered.delivery)
  }

  async fn session_state(&self, session: &SessionId) -> Answer<Option<SessionState>> {
    let request = self.request(Method::GET, &["sessions", &session.0, "state"], &[]);

    self.found("session_state", request).await
  }

  async fn session_output(&self, session: &SessionId) -> Answer<Option<Output>> {
    let request = self.request(Method::GET, &["sessions", &session.0, "output"], &[]);

    self.found("session_output", request).await
  }

  async fn interrupt_session(&self, session: &SessionId) -> Answer<()> {
    let request = self.request(Method::POST, &["sessions", &session.0, "interrupt"], &[]);

    self.done("interrupt_session", request).await
  }

  async fn fold_session(&self, session: &SessionId) -> Answer<()> {
    let request = self.request(Method::POST, &["sessions", &session.0, "fold"], &[]);

    self.done("fold_session", request).await
  }

  async fn end_session(&self, session: &SessionId) -> Answer<()> {
    let request = self.request(Method::DELETE, &["sessions", &session.0], &[]);

    self.done("end_session", request).await
  }

  async fn sessions(&self) -> Answer<Vec<Session>> {
    self.answer("sessions", self.request(Method::GET, &["sessions"], &[])).await
  }

  async fn executions(&self, open_only: bool) -> Answer<Vec<Execution>> {
    let open = if open_only { "true" } else { "false" };
    let request = self.request(Method::GET, &["executions"], &[("open", open)]);

    self.answer("executions", request).await
  }

  async fn execution(&self, id: &str) -> Answer<Option<Execution>> {
    self.found("execution", self.request(Method::GET, &["executions", id], &[])).await
  }

  async fn cancel_execution(&self, id: &str) -> Answer<()> {
    let request = self.request(Method::POST, &["executions", id, "cancel"], &[]);

    self.done("cancel_execution", request).await
  }

  async fn drop_cancels(&self, id: &str) -> Answer<Vec<String>> {
    let request = self.request(Method::DELETE, &["executions", id, "cancel"], &[]);
    let dropped: Dropped = self.answer("drop_cancels", request).await?;

    Ok(dropped.dropped)
  }

  async fn open_run(&self, opening: &RunOpening) -> Answer<i64> {
    let request = self.request(Method::POST, &["runs"], &[]).json(opening);
    let opened: Opened = self.answer("open_run", request).await?;

    Ok(opened.id)
  }

  async fn close_run(&self, run: i64, closing: &RunClosing) -> Answer<()> {
    let run = run.to_string();
    let request = self.request(Method::POST, &["runs", &run, "close"], &[]).json(closing);

    self.done("close_run", request).await
  }

  async fn run(&self, run: i64) -> Answer<Option<Run>> {
    let run = run.to_string();

    self.found("run", self.request(Method::GET, &["runs", &run], &[])).await
  }

  async fn run_starts(&self, agent: &str, run: &str) -> Answer<u64> {
    let request = self.request(Method::GET, &["log", "starts"], &[("agent", agent), ("run", run)]);
    let counted: Counted = self.answer("run_starts", request).await?;

    Ok(counted.count)
  }

  async fn job_last_line(&self, job: i64) -> Answer<Option<LogLine>> {
    let job = job.to_string();
    let request = self.request(Method::GET, &["log", "jobs", &job, "last"], &[]);

    self.found("job_last_line", request).await
  }

  async fn job_lines(&self, job: i64) -> Answer<JobLines> {
    let job = job.to_string();

    self.answer("job_lines", self.request(Method::GET, &["log", "jobs", &job], &[])).await
  }

  async fn keep_turn_record(&self, record: &TurnRecord) -> Answer<()> {
    let request = self.request(Method::POST, &["turn-records"], &[]).json(record);

    self.done("keep_turn_record", request).await
  }

  async fn worker_runtime(&self, worker: &str) -> Answer<Option<WorkerRuntime>> {
    let request = self.request(Method::GET, &["workers", worker, "runtime"], &[]);

    self.found("worker_runtime", request).await
  }

  async fn pin_worker(&self, worker: &str, pin: &Pin) -> Answer<()> {
    let request = self.request(Method::PUT, &["workers", worker, "pin"], &[]).json(pin);

    self.done("pin_worker", request).await
  }

  async fn unpin_worker(&self, worker: &str) -> Answer<()> {
    let request = self.request(Method::DELETE, &["workers", worker, "pin"], &[]);

    self.done("unpin_worker", request).await
  }

  async fn pause_keepalive(&self, worker: &str, at: i64) -> Answer<()> {
    let request = self
      .request(Method::PUT, &["workers", worker, "keepalive-pause"], &[])
      .json(&json!({ "at": at }));

    self.done("pause_keepalive", request).await
  }

  async fn resume_keepalive(&self, worker: &str) -> Answer<()> {
    let request = self.request(Method::DELETE, &["workers", worker, "keepalive-pause"], &[]);

    self.done("resume_keepalive", request).await
  }

  async fn record_context(&self, worker: &str, tokens: u64) -> Answer<()> {
    let request = self
      .request(Method::PUT, &["workers", worker, "context-tokens"], &[])
      .json(&json!({ "tokens": tokens }));

    self.done("record_context", request).await
  }

  async fn providers(&self) -> Answer<Vec<Provider>> {
    self.answer("providers", self.request(Method::GET, &["providers"], &[])).await
  }

  async fn tiers(&self) -> Answer<Vec<Tier>> {
    self.answer("tiers", self.request(Method::GET, &["tiers"], &[])).await
  }

  async fn resolve_route(&self, asked: &RouteAsk) -> Answer<Option<Mapping>> {
    let request = self.request(Method::POST, &["routes", "resolve"], &[]).json(asked);

    self.found("resolve_route", request).await
  }

  async fn check_pin(&self, value: &str) -> Answer<PinChoice> {
    let request = self.request(Method::POST, &["pins", "check"], &[]).json(&json!({ "value": value }));

    self.answer("check_pin", request).await
  }

  async fn enqueue_request(&self, asking: &PoolAsking) -> Answer<PoolRequest> {
    let request = self.request(Method::POST, &["pool", "requests"], &[]).json(asking);

    self.answer("enqueue_request", request).await
  }

  async fn pool_request(&self, token: &str) -> Answer<Option<PoolRequest>> {
    let request = self.request(Method::GET, &["pool", "requests", token], &[]);

    self.found("pool_request", request).await
  }

  async fn open_requests(&self, owner: Option<&str>) -> Answer<Vec<PoolRequest>> {
    let query: Vec<(&str, &str)> = owner.map(|owner| ("owner", owner)).into_iter().collect();
    let request = self.request(Method::GET, &["pool", "requests"], &query);

    self.answer("open_requests", request).await
  }

  async fn lease_place(&self, token: &str) -> Answer<PoolRequest> {
    let request = self.request(Method::POST, &["pool", "requests", token, "lease"], &[]);

    self.answer("lease_place", request).await
  }

  async fn bind_child(&self, token: &str, child: &str) -> Answer<PoolRequest> {
    let request = self
      .request(Method::POST, &["pool", "requests", token, "bind"], &[])
      .json(&json!({ "child": child }));

    self.answer("bind_child", request).await
  }

  async fn finish_request(&self, token: &str, outcome: &str, cancel: bool) -> Answer<bool> {
    let request = self
      .request(Method::POST, &["pool", "requests", token, "finish"], &[])
      .json(&json!({ "outcome": outcome, "cancel": cancel }));
    let finished: Finished = self.answer("finish_request", request).await?;

    Ok(finished.finished)
  }

  async fn reserve_seats(&self, asking: &ReservationAsking) -> Answer<Reservation> {
    let request = self.request(Method::POST, &["pool", "reservations"], &[]).json(asking);

    self.answer("reserve_seats", request).await
  }

  async fn held_reservations(&self, owner: Option<&str>) -> Answer<Vec<Reservation>> {
    let query: Vec<(&str, &str)> = owner.map(|owner| ("owner", owner)).into_iter().collect();
    let request = self.request(Method::GET, &["pool", "reservations"], &query);

    self.answer("held_reservations", request).await
  }

  async fn release_reservation(&self, token: &str, reason: &str) -> Answer<bool> {
    let request = self
      .request(Method::POST, &["pool", "reservations", token, "release"], &[])
      .json(&json!({ "reason": reason }));
    let released: Released = self.answer("release_reservation", request).await?;

    Ok(released.released)
  }

  async fn record_place_write(&self, place: &str, write: &PlaceWrite) -> Answer<()> {
    let request = self.request(Method::PUT, &["pool", "places", place, "write"], &[]).json(write);

    self.done("record_place_write", request).await
  }

  async fn nodes(&self) -> Answer<Vec<Node>> {
    self.answer("nodes", self.request(Method::GET, &["nodes"], &[])).await
  }

  async fn ssh_reach(&self, node: &str) -> Answer<Option<SshReach>> {
    self.found("ssh_reach", self.request(Method::GET, &["nodes", node, "reach"], &[])).await
  }

  async fn place_heavy_run(&self) -> Answer<Option<WorkMachine>> {
    let request = self.request(Method::POST, &["nodes", "heavy-run"], &[]);
    let placed: Placed = self.answer("place_heavy_run", request).await?;

    Ok(placed.machine)
  }

  async fn claude_account_held(&self, user: &str) -> Answer<bool> {
    let request = self.request(Method::GET, &["accounts", "claude", "held"], &[("user", user)]);
    let held: Held = self.answer("claude_account_held", request).await?;

    Ok(held.held)
  }

  async fn resting(&self, agent: &str) -> Answer<bool> {
    let request = self.request(Method::GET, &["agents", agent, "rest"], &[]);
    let rest: Rest = self.answer("resting", request).await?;

    Ok(rest.resting)
  }

  async fn machine_room(&self) -> Answer<Room> {
    self.answer("machine_room", self.request(Method::GET, &["machine", "room"], &[])).await
  }

  async fn tell_launch_held(&self, notice: &LaunchHeld) -> Answer<()> {
    let request = self.request(Method::PUT, &["machine", "launch-held"], &[]).json(notice);

    self.done("tell_launch_held", request).await
  }

  async fn workbench(&self, worker: &str) -> Answer<Workbench> {
    let request = self.request(Method::GET, &["places", "workbench", worker], &[]);

    self.answer("workbench", request).await
  }

  async fn branch_slots(&self, pool: &str) -> Answer<BranchSlots> {
    let request = self.request(Method::GET, &["places", "branch-slots", pool], &[]);

    self.answer("branch_slots", request).await
  }

  async fn place_cache(&self, provider: &str, model: &str, upstream: Option<&str>) -> Answer<Option<i64>> {
    let mut query = vec![("model", model)];

    query.extend(upstream.map(|upstream| ("upstream", upstream)));

    let request = self.request(Method::GET, &["providers", provider, "cache"], &query);
    let cache: Cache = self.answer("place_cache", request).await?;

    Ok(cache.ttl_ms)
  }

  async fn pool_roles(&self) -> Answer<Vec<PoolRole>> {
    self.answer("pool_roles", self.request(Method::GET, &["pool", "roles"], &[])).await
  }

  async fn retire_place(&self, pool: &str, slot: i64) -> Answer<()> {
    let slot = slot.to_string();
    let request = self.request(Method::DELETE, &["places", "branch-slots", pool, &slot], &[]);

    self.done("retire_place", request).await
  }

  async fn prefix_digest(&self, cwd: &str, model: &str, effort: Option<&str>) -> Answer<String> {
    let request = self
      .request(Method::POST, &["places", "prefix-digest"], &[])
      .json(&json!({ "cwd": cwd, "model": model, "effort": effort }));
    let digest: Digest = self.answer("prefix_digest", request).await?;

    Ok(digest.digest)
  }
}
