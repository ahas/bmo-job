//! The port's HTTP client: each call made on the method and path the API document gives it, its body
//! written and its answer read as JSON.

use futures_util::StreamExt;
use futures_util::stream::BoxStream;
use reqwest::{Client, Method, RequestBuilder, Response, StatusCode};
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};
use sse_stream::SseStream;
use url::Url;

use crate::workers::{AgentRow, runtime_of};
use crate::{
  Agency, AgencyError, Answer, AsideOutcome, Delivery, Execution, Handover, JobDocuments, JobLines,
  LaunchHeld, LogLine, LogNote, Mapping, Node, Pin, PinChoice, PlaceAsk, PlaceRecord, PlaceWrite,
  PlacesRetirement, PoolAsking, PoolRequest, PoolRole, PoolRoom, Provider, Reply, Reservation,
  ReservationAsking, Room, RoomReturns, RouteAsk, Run, RunClosing, RunOpening, Session, SessionAnswered,
  SessionId, SessionKept, SessionState, SshReach, Start, TakenPlace, Tier, Turn, TurnEvent, TurnEvents,
  TurnRecord, Workbench, WorkerRuntime,
};

/// The kind of event bmo-agency publishes once a held launch's notice is let go for the room returned.
const ROOM_RETURNED: &str = "machine-room-returned";

/// The kind of event bmo-agency publishes as a turn put into a session begins.
const TURN_STARTED: &str = "turn-started";

/// The kind of event bmo-agency publishes as a turn put into a session ends.
const TURN_ENDED: &str = "turn-ended";

/// The code bmo-agency refuses a read by an id with where it keeps nothing there, as the base chat library
/// names it. Any other 404, a path it serves no route on among them, is a refusal.
const NOTHING_HELD: &str = "nothing-held";

/// The code a session operation is refused with where bmo-agency keeps no such session.
const UNKNOWN_SESSION: &str = "unknown-session";

/// The kind of the line a run's log opens each start of the run with.
const START_LINE: &str = "start";

/// The kind of the line a run's log keeps each tool call under.
const TOOL_LINE: &str = "tool";

/// The most lines a read of a run's log answers at once.
const RUN_LOG_PAGE: usize = 500;

/// The field of a worker's row its pin is changed by.
const PINNED_PROVIDER: &str = "pinnedProvider";

/// The choice of a provider that leaves a worker to its walk, unpinned.
const UNPINNED: &str = "auto";

/// The field of a worker's row its keepalive is stopped and started by.
const KEEPALIVE_STOPPED: &str = "keepaliveStopped";

/// The kind of the rest a full window writes on a login.
const LIMIT_REST: &str = "limit";

/// The source bmo-agency enqueues a builder turn's place under; any other request is a branch's seat.
const TURN_SOURCE: &str = "turn";

/// The source bmo-agency enqueues a branch's seat under.
const COWORK_SOURCE: &str = "cowork";

/// The state a tile of the pool's board stands in while its request is let in.
const WORKING_TILE: &str = "working";

/// The state a tile of the pool's board stands in while a reservation holds it.
const HELD_TILE: &str = "held";

/// The calls on bmo-agency, made over HTTP to the address given.
#[derive(Clone, Debug)]
pub struct AgencyClient {
  base: Url,
  token: String,
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
struct Dropped {
  dropped: Vec<String>,
}

#[derive(Deserialize)]
struct Admitted {
  request: Option<PoolRequest>,
}

#[derive(Deserialize)]
struct Reservations {
  reservations: Vec<Reservation>,
}

#[derive(Deserialize)]
struct Agents {
  agents: Vec<AgentRow>,
}

/// One line of a run's log, as far as counting its starts reads it.
#[derive(Deserialize)]
struct RunLine {
  id: i64,
  agent: String,
  kind: String,
}

#[derive(Deserialize)]
struct RunLines {
  lines: Vec<RunLine>,
}

/// What the pool's board holds of one provider's tier: its places' tiles and the requests waiting on them.
#[derive(Deserialize)]
struct Bucket {
  provider: String,
  tier: String,
  capacity: i64,
  tiles: Vec<Tile>,
  waiting: Vec<Value>,
}

#[derive(Deserialize)]
struct Tile {
  state: String,
}

#[derive(Deserialize)]
struct Board {
  buckets: Vec<Bucket>,
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
struct Homes {
  homes: Vec<String>,
}

#[derive(Deserialize)]
struct Spoken {
  said: AsideOutcome,
}

#[derive(Deserialize)]
struct Listed {
  sessions: Vec<Session>,
}

#[derive(Deserialize)]
struct Carried {
  carried: bool,
}

#[derive(Deserialize)]
struct Kept {
  kept: SessionKept,
}

#[derive(Deserialize)]
struct Swept {
  lines: Vec<String>,
}

#[derive(Deserialize)]
struct Touched {
  touched: Vec<String>,
}

#[derive(Deserialize)]
struct Health {
  answers: bool,
}

#[derive(Deserialize)]
struct Copy {
  path: String,
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
  /// A client of bmo-agency at `base`, which must be an `http:` address, calling with its control `token`.
  pub fn new(base: Url, token: impl Into<String>) -> Answer<Self> {
    if base.scheme() != "http" || base.cannot_be_a_base() {
      return Err(AgencyError::NotAnAddress {
        address: base.to_string(),
      });
    }

    Ok(Self {
      base,
      token: token.into(),
      http: Client::new(),
    })
  }

  /// The address the client calls.
  pub fn address(&self) -> &Url {
    &self.base
  }

  /// A request on the path made of `segments` under the address, each percent-encoded, with `query`, carrying
  /// the control token.
  fn request(&self, method: Method, segments: &[&str], query: &[(&str, &str)]) -> RequestBuilder {
    let mut url = self.base.clone();

    if let Ok(mut path) = url.path_segments_mut() {
      path.pop_if_empty().extend(segments);
    }

    if !query.is_empty() {
      url.query_pairs_mut().extend_pairs(query);
    }

    self.http.request(method, url).bearer_auth(&self.token)
  }

  async fn send(&self, call: &'static str, request: RequestBuilder) -> Answer<Response> {
    request.send().await.map_err(unreachable(call))
  }

  /// A call answered with a body.
  async fn answer<T: DeserializeOwned>(&self, call: &'static str, request: RequestBuilder) -> Answer<T> {
    read(call, self.send(call, request).await?).await
  }

  /// A read of one thing by its id, where a 404 refused with the code `nothing` is nothing found.
  async fn found<T: DeserializeOwned>(
    &self,
    call: &'static str,
    nothing: &str,
    request: RequestBuilder,
  ) -> Answer<Option<T>> {
    let response = self.send(call, request).await?;
    let status = response.status();

    if status != StatusCode::NOT_FOUND {
      return read(call, response).await.map(Some);
    }

    let body = response.bytes().await.map_err(unreachable(call))?;

    match refused(call, status, &body) {
      AgencyError::Refused { code, .. } if code == nothing => Ok(None),
      refusal => Err(refusal),
    }
  }

  /// One field of a worker's row changed, as bmo-agency's worker update takes it.
  async fn update_worker(&self, call: &'static str, worker: &str, field: &str, value: Value) -> Answer<()> {
    let mut body = Map::new();

    body.insert("action".into(), json!("update"));
    body.insert("name".into(), json!(worker));
    body.insert(field.into(), value);

    let _: Value = self.answer(call, self.request(Method::POST, &["worker"], &[]).json(&body)).await?;

    Ok(())
  }

  /// The lines of `kind` an agent wrote in a run's log, read a page at a time from the beginning, each page
  /// after the last line of the one before, until a page holds fewer than a page's lines.
  async fn run_lines(&self, call: &'static str, agent: &str, run: &str, kind: &str) -> Answer<u64> {
    let mut after = 0;
    let mut counted = 0;

    loop {
      let from = after.to_string();
      let request = self.request(Method::GET, &["run-log"], &[("run", run), ("after", &from)]);
      let page: RunLines = self.answer(call, request).await?;

      counted += page.lines.iter().filter(|line| line.kind == kind && line.agent == agent).count() as u64;

      match page.lines.last() {
        Some(last) if page.lines.len() >= RUN_LOG_PAGE => after = last.id,
        _ => return Ok(counted),
      }
    }
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

  /// The stream of the events of `kinds`, each read by `read` from its kind and its data. An event `read`
  /// passes over, or a comment between events, says nothing.
  async fn events<T: Send + 'static>(
    &self,
    call: &'static str,
    kinds: &str,
    read: fn(&str, &str) -> Option<serde_json::Result<T>>,
  ) -> Answer<BoxStream<'static, Answer<T>>> {
    let request = self.request(Method::GET, &["events"], &[("kinds", kinds)]);
    let response = self.send(call, request).await?;
    let status = response.status();

    if !status.is_success() {
      let body = response.bytes().await.map_err(unreachable(call))?;

      return Err(refused(call, status, &body));
    }

    let events = SseStream::from_bytes_stream(response.bytes_stream()).filter_map(move |sse| async move {
      match sse {
        Err(error) => Some(Err(AgencyError::Unreachable {
          call,
          error: error.to_string(),
        })),
        Ok(sse) => {
          let data = sse.data.unwrap_or_default();

          read(sse.event.as_deref().unwrap_or_default(), &data).map(|read| {
            read.map_err(|error| AgencyError::Malformed {
              call,
              status: status.as_u16(),
              error: error.to_string(),
            })
          })
        }
      }
    });

    Ok(events.boxed())
  }
}

impl Agency for AgencyClient {
  async fn start_session(&self, start: &Start) -> Answer<Session> {
    let request = self.request(Method::POST, &["session", "start"], &[]).json(start);

    self.answer("start_session", request).await
  }

  async fn send_turn(&self, turn: &Turn) -> Answer<Delivery> {
    let request = self.request(Method::POST, &["session", "turn"], &[]).json(turn);
    let delivered: Delivered = self.answer("send_turn", request).await?;

    Ok(delivered.delivery)
  }

  async fn push_words(&self, session: &SessionId, words: &str) -> Answer<AsideOutcome> {
    let request = self
      .request(Method::POST, &["session", "push"], &[])
      .json(&json!({ "session": session, "words": words }));
    let spoken: Spoken = self.answer("push_words", request).await?;

    Ok(spoken.said)
  }

  async fn session_state(&self, session: &SessionId) -> Answer<Option<SessionState>> {
    let request = self.request(Method::GET, &["session", "state"], &[("session", &session.0)]);

    self.found("session_state", UNKNOWN_SESSION, request).await
  }

  async fn interrupt_session(&self, session: &SessionId) -> Answer<()> {
    let request = self
      .request(Method::POST, &["session", "interrupt"], &[])
      .json(&json!({ "session": session }));

    self.done("interrupt_session", request).await
  }

  async fn fold_session(&self, session: &SessionId) -> Answer<()> {
    let request = self
      .request(Method::POST, &["session", "fold"], &[])
      .json(&json!({ "session": session }));

    self.done("fold_session", request).await
  }

  async fn end_session(&self, session: &SessionId) -> Answer<()> {
    let request = self
      .request(Method::POST, &["session", "end"], &[])
      .json(&json!({ "session": session }));

    self.done("end_session", request).await
  }

  async fn sessions(&self) -> Answer<Vec<Session>> {
    let listed: Listed =
      self.answer("sessions", self.request(Method::GET, &["session", "list"], &[])).await?;

    Ok(listed.sessions)
  }

  async fn turn_events(&self) -> Answer<TurnEvents> {
    let kinds = format!("{TURN_STARTED},{TURN_ENDED}");

    self
      .events("turn_events", &kinds, |kind, data| match kind {
        TURN_STARTED => Some(serde_json::from_str(data).map(TurnEvent::Started)),
        TURN_ENDED => Some(serde_json::from_str(data).map(TurnEvent::Ended)),
        _ => None,
      })
      .await
  }

  async fn executions(&self, open_only: bool) -> Answer<Vec<Execution>> {
    let open = if open_only { "true" } else { "false" };
    let request = self.request(Method::GET, &["executions"], &[("open", open)]);

    self.answer("executions", request).await
  }

  async fn execution(&self, id: &str) -> Answer<Option<Execution>> {
    let request = self.request(Method::GET, &["executions", id], &[]);

    self.found("execution", NOTHING_HELD, request).await
  }

  async fn cancel_execution(&self, id: &str) -> Answer<()> {
    let request = self.request(Method::POST, &["runs", "stop"], &[]).json(&json!({ "run": id }));
    let _: Value = self.answer("cancel_execution", request).await?;

    Ok(())
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

    self.found("run", NOTHING_HELD, self.request(Method::GET, &["runs", &run], &[])).await
  }

  async fn run_starts(&self, agent: &str, run: &str) -> Answer<u64> {
    self.run_lines("run_starts", agent, run, START_LINE).await
  }

  async fn run_tool_calls(&self, agent: &str, run: &str) -> Answer<u64> {
    self.run_lines("run_tool_calls", agent, run, TOOL_LINE).await
  }

  async fn job_last_line(&self, job: i64) -> Answer<Option<LogLine>> {
    let job = job.to_string();
    let request = self.request(Method::GET, &["log", "jobs", &job, "last"], &[]);

    self.found("job_last_line", NOTHING_HELD, request).await
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
    let listed: Agents = self.answer("worker_runtime", self.request(Method::GET, &["agents"], &[])).await?;

    Ok(runtime_of(&listed.agents, worker))
  }

  async fn pin_worker(&self, worker: &str, pin: &Pin) -> Answer<()> {
    self.update_worker("pin_worker", worker, PINNED_PROVIDER, json!(pin.choice())).await
  }

  async fn unpin_worker(&self, worker: &str) -> Answer<()> {
    self.update_worker("unpin_worker", worker, PINNED_PROVIDER, json!(UNPINNED)).await
  }

  async fn pause_keepalive(&self, worker: &str) -> Answer<()> {
    self.update_worker("pause_keepalive", worker, KEEPALIVE_STOPPED, json!(true)).await
  }

  async fn resume_keepalive(&self, worker: &str) -> Answer<()> {
    self.update_worker("resume_keepalive", worker, KEEPALIVE_STOPPED, json!(false)).await
  }

  async fn say_to_worker(&self, worker: &str, text: &str) -> Answer<()> {
    let request = self
      .request(Method::POST, &["workers", worker, "words"], &[])
      .json(&json!({ "text": text }));

    self.done("say_to_worker", request).await
  }

  async fn providers(&self) -> Answer<Vec<Provider>> {
    self.answer("providers", self.request(Method::GET, &["providers"], &[])).await
  }

  async fn tiers(&self) -> Answer<Vec<Tier>> {
    self.answer("tiers", self.request(Method::GET, &["tiers"], &[])).await
  }

  async fn resolve_route(&self, asked: &RouteAsk) -> Answer<Option<Mapping>> {
    let request = self.request(Method::POST, &["routes", "resolve"], &[]).json(asked);

    self.found("resolve_route", NOTHING_HELD, request).await
  }

  async fn check_pin(&self, value: &str) -> Answer<PinChoice> {
    let request = self.request(Method::POST, &["pins", "check"], &[]).json(&json!({ "value": value }));

    self.answer("check_pin", request).await
  }

  async fn enqueue_request(&self, asking: &PoolAsking) -> Answer<PoolRequest> {
    let source = if asking.turn { TURN_SOURCE } else { COWORK_SOURCE };
    let request = self.request(Method::POST, &["pool", "enqueue"], &[]).json(&json!({
      "source": source,
      "requestKey": asking.request_key,
      "owner": asking.owner,
      "role": asking.role,
      "selection": asking.tier,
      "tier": asking.tier,
      "provider": asking.provider,
      "yielding": asking.yielding,
      "placeKey": asking.place_key,
    }));

    self.answer("enqueue_request", request).await
  }

  async fn pool_request(&self, token: &str) -> Answer<Option<PoolRequest>> {
    let request = self.request(Method::GET, &["pool", "requests", token], &[]);

    self.found("pool_request", NOTHING_HELD, request).await
  }

  async fn open_requests(&self, owner: Option<&str>) -> Answer<Vec<PoolRequest>> {
    let query: Vec<(&str, &str)> = owner.map(|owner| ("owner", owner)).into_iter().collect();
    let request = self.request(Method::GET, &["pool", "requests"], &query);

    self.answer("open_requests", request).await
  }

  async fn lease_place(&self, token: &str) -> Answer<Option<PoolRequest>> {
    let request = self.request(Method::POST, &["pool", "acquire"], &[]).json(&json!({ "token": token }));
    let admitted: Admitted = self.answer("lease_place", request).await?;

    Ok(admitted.request)
  }

  async fn bind_child(&self, token: &str, child: &str) -> Answer<PoolRequest> {
    let request = self
      .request(Method::POST, &["pool", "bind"], &[])
      .json(&json!({ "token": token, "childId": child }));

    self.answer("bind_child", request).await
  }

  async fn finish_request(&self, token: &str, outcome: &str, cancel: bool) -> Answer<bool> {
    let request = self
      .request(Method::POST, &["pool", "finish"], &[])
      .json(&json!({ "token": token, "outcome": outcome, "cancel": cancel }));
    let held: Held = self.answer("finish_request", request).await?;

    Ok(held.held)
  }

  async fn reserve_seats(&self, asking: &ReservationAsking) -> Answer<Reservation> {
    let request = self.request(Method::POST, &["pool", "reserve"], &[]).json(&json!({
      "reservationKey": asking.reservation_key,
      "owner": asking.owner,
      "source": asking.source,
      "role": asking.role,
      "selection": asking.tier,
      "tier": asking.tier,
      "provider": asking.provider,
      "count": asking.wanted,
    }));

    self.answer("reserve_seats", request).await
  }

  async fn held_reservations(&self, owner: Option<&str>) -> Answer<Vec<Reservation>> {
    let query: Vec<(&str, &str)> = owner.map(|owner| ("owner", owner)).into_iter().collect();
    let request = self.request(Method::GET, &["pool", "reservations"], &query);
    let held: Reservations = self.answer("held_reservations", request).await?;

    Ok(held.reservations)
  }

  async fn release_reservation(&self, token: &str, reason: &str) -> Answer<bool> {
    let request = self
      .request(Method::POST, &["pool", "reservations", "release"], &[])
      .json(&json!({ "token": token, "outcome": reason }));
    let held: Held = self.answer("release_reservation", request).await?;

    Ok(held.held)
  }

  async fn record_place_write(&self, place: &str, write: &PlaceWrite) -> Answer<()> {
    let request = self.request(Method::PUT, &["pool", "places", place, "write"], &[]).json(write);

    self.done("record_place_write", request).await
  }

  async fn nodes(&self) -> Answer<Vec<Node>> {
    self.answer("nodes", self.request(Method::GET, &["nodes"], &[])).await
  }

  async fn ssh_reach(&self, node: &str) -> Answer<Option<SshReach>> {
    let request = self.request(Method::GET, &["nodes", node, "reach"], &[]);

    self.found("ssh_reach", NOTHING_HELD, request).await
  }

  async fn claude_account_held(&self, user: &str) -> Answer<bool> {
    let request = self.request(Method::GET, &["accounts", "claude", "held"], &[("user", user)]);
    let held: Held = self.answer("claude_account_held", request).await?;

    Ok(held.held)
  }

  async fn claude_homes(&self) -> Answer<Vec<String>> {
    let request = self.request(Method::GET, &["accounts", "claude", "homes"], &[]);
    let homes: Homes = self.answer("claude_homes", request).await?;

    Ok(homes.homes)
  }

  async fn resting(&self) -> Answer<bool> {
    let rest: Rest = self.answer("resting", self.request(Method::GET, &["rest"], &[])).await?;

    Ok(rest.resting)
  }

  async fn machine_room(&self) -> Answer<Room> {
    self.answer("machine_room", self.request(Method::GET, &["machine", "room"], &[])).await
  }

  async fn tell_launch_held(&self, notice: &LaunchHeld) -> Answer<()> {
    let request = self.request(Method::POST, &["machine", "launch-held"], &[]).json(notice);

    self.done("tell_launch_held", request).await
  }

  async fn room_returns(&self) -> Answer<RoomReturns> {
    self
      .events("room_returns", ROOM_RETURNED, |kind, data| {
        (kind == ROOM_RETURNED).then(|| serde_json::from_str(data))
      })
      .await
  }

  async fn workbench(&self, worker: &str) -> Answer<Workbench> {
    let request = self.request(Method::GET, &["places", "workbench", worker], &[]);

    self.answer("workbench", request).await
  }

  async fn pool_roles(&self) -> Answer<Vec<PoolRole>> {
    self.answer("pool_roles", self.request(Method::GET, &["pool", "roles"], &[])).await
  }

  async fn pool_room(&self, provider: &str, tier: &str) -> Answer<PoolRoom> {
    let board: Board = self.answer("pool_room", self.request(Method::GET, &["coworker-pool"], &[])).await?;
    let Some(bucket) = board.buckets.iter().find(|one| one.provider == provider && one.tier == tier) else {
      return Ok(PoolRoom {
        capacity: 0,
        active: 0,
        reserved: 0,
        waiting: 0,
      });
    };
    let standing = |state: &str| bucket.tiles.iter().filter(|tile| tile.state == state).count() as i64;

    Ok(PoolRoom {
      capacity: bucket.capacity,
      active: standing(WORKING_TILE),
      reserved: standing(HELD_TILE),
      waiting: bucket.waiting.len() as i64,
    })
  }

  async fn take_place(&self, ask: &PlaceAsk) -> Answer<TakenPlace> {
    self
      .answer("take_place", self.request(Method::POST, &["places", "take"], &[]).json(ask))
      .await
  }

  async fn prepare_place(&self, source: &str, cwd: &str) -> Answer<()> {
    let request = self
      .request(Method::POST, &["places", "prepare"], &[])
      .json(&json!({ "source": source, "cwd": cwd }));

    self.done("prepare_place", request).await
  }

  async fn remember_place(&self, record: &PlaceRecord) -> Answer<()> {
    self
      .done("remember_place", self.request(Method::POST, &["places", "remember"], &[]).json(record))
      .await
  }

  async fn retire_places(&self, retirement: &PlacesRetirement) -> Answer<()> {
    let request = self.request(Method::POST, &["places", "retire"], &[]).json(retirement);

    self.done("retire_places", request).await
  }

  async fn carry_out_reset(&self, worker: &str) -> Answer<bool> {
    let request = self.request(Method::POST, &["workers", worker, "sessions", "reset"], &[]);
    let carried: Carried = self.answer("carry_out_reset", request).await?;

    Ok(carried.carried)
  }

  async fn record_session(
    &self,
    worker: &str,
    answered: &SessionAnswered,
    cwd: &str,
    job: i64,
  ) -> Answer<SessionKept> {
    let request = self
      .request(Method::POST, &["workers", worker, "sessions", "record"], &[])
      .json(&json!({ "answered": answered, "cwd": cwd, "job": job }));
    let kept: Kept = self.answer("record_session", request).await?;

    Ok(kept.kept)
  }

  async fn hand_over_session(&self, worker: &str, handover: &Handover) -> Answer<()> {
    let request = self
      .request(Method::POST, &["workers", worker, "sessions", "handover"], &[])
      .json(handover);

    self.done("hand_over_session", request).await
  }

  async fn note_run(&self, worker: &str) -> Answer<()> {
    self
      .done("note_run", self.request(Method::POST, &["workers", worker, "runs"], &[]))
      .await
  }

  async fn record_usage(&self, worker: &str, reply: &Reply, run: Option<&str>) -> Answer<()> {
    let request = self
      .request(Method::POST, &["workers", worker, "usage"], &[])
      .json(&json!({ "reply": reply, "run": run }));

    self.done("record_usage", request).await
  }

  async fn make_bench(&self, worker: &str) -> Answer<()> {
    self
      .done("make_bench", self.request(Method::POST, &["workers", worker, "bench"], &[]))
      .await
  }

  async fn sweep_servers(&self, worker: &str) -> Answer<Vec<String>> {
    let request = self.request(Method::POST, &["workers", worker, "servers", "sweep"], &[]);
    let swept: Swept = self.answer("sweep_servers", request).await?;

    Ok(swept.lines)
  }

  async fn job_documents(&self, worker: &str) -> Answer<JobDocuments> {
    let request = self.request(Method::GET, &["workers", worker, "job-documents"], &[]);

    self.answer("job_documents", request).await
  }

  async fn prefix_touched(&self, worker: &str, files: &[String]) -> Answer<Vec<String>> {
    let request = self
      .request(Method::POST, &["workers", worker, "prefix-touched"], &[])
      .json(&json!({ "files": files }));
    let touched: Touched = self.answer("prefix_touched", request).await?;

    Ok(touched.touched)
  }

  async fn model_health(&self, provider: &str, model: &str) -> Answer<bool> {
    let request = self.request(Method::GET, &["providers", provider, "models", model, "health"], &[]);
    let health: Health = self.answer("model_health", request).await?;

    Ok(health.answers)
  }

  async fn rest_login(&self, account: &str, until: i64) -> Answer<()> {
    let request = self
      .request(Method::POST, &["rest"], &[])
      .json(&json!({ "until": until / 1000, "kind": LIMIT_REST, "account": account }));
    let _: Value = self.answer("rest_login", request).await?;

    Ok(())
  }

  async fn usage_rest(&self, account: &str, usage: &serde_json::Value) -> Answer<()> {
    let request = self
      .request(Method::POST, &["accounts", "claude", account, "usage-rest"], &[])
      .json(&json!({ "usage": usage }));

    self.done("usage_rest", request).await
  }

  async fn write_note(&self, note: &LogNote) -> Answer<()> {
    self
      .done("write_note", self.request(Method::POST, &["log", "notes"], &[]).json(note))
      .await
  }

  async fn node_copy(&self, node: &str, checkout: &str) -> Answer<String> {
    let request = self.request(Method::GET, &["nodes", node, "copy"], &[("checkout", checkout)]);
    let copy: Copy = self.answer("node_copy", request).await?;

    Ok(copy.path)
  }
}
