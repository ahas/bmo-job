//! The agency port's contract: every call is made on the method, path and body the API document gives
//! it, and its answer is read as the call's shape (docs/api.md, "bmo-agency").

mod support;

use std::path::PathBuf;

use futures_util::StreamExt;
use job_agency::{
  Agency, AgencyClient, AgencyError, AsideOutcome, BranchSlots, Context, Delivery, EngineError, Execution,
  JobLines, LaunchHeld, LogLine, Mapping, Node, Pin, PinChoice, PlaceWrite, PoolAsking, PoolRequest,
  PoolRole, PoolRoom, PoolState, Provider, Reservation, ReservationAsking, ReservationState, Room,
  RoomReturned, RouteAsk, Run, RunClosing, RunOpening, Scope, Session, SessionId, SessionState, Slot,
  SshReach, Start, Tier, ToolCall, ToolResult, Turn, TurnContext, TurnEnded, TurnEvent, TurnOutcome,
  TurnRecord, TurnRecordKind, TurnStarted, Workbench, WorkerRuntime,
};
use serde_json::{Map, Value, json};
use support::agency::{Stub, TOKEN};
use url::Url;

fn session_id() -> SessionId {
  SessionId("s-1".into())
}

fn object(value: Value) -> Map<String, Value> {
  value.as_object().cloned().expect("the value is an object")
}

fn execution_json() -> Value {
  json!({
    "id": "e-2",
    "parent": "e-1",
    "root": "e-1",
    "line": "coordinator",
    "sharedPane": true,
    "startedAt": 1_000,
    "endedAt": null,
    "outcome": { "state": "done" },
    "cancelled": true,
    "details": { "pid": 42, "kind": "job" }
  })
}

fn execution() -> Execution {
  Execution {
    id: "e-2".into(),
    parent: Some("e-1".into()),
    root: Some("e-1".into()),
    line: Some("coordinator".into()),
    shared_pane: true,
    started_at: Some(1_000),
    ended_at: None,
    outcome: Some(object(json!({ "state": "done" }))),
    cancelled: true,
    details: object(json!({ "pid": 42, "kind": "job" })),
  }
}

/// A pool request as bmo-agency answers it, with the fields of its own beyond those the port reads.
fn pool_request_json(state: &str, child: Option<&str>) -> Value {
  json!({
    "sequence": 7,
    "token": "t-1",
    "requestKey": "key-1",
    "owner": "owner-1",
    "source": "research",
    "role": "builder",
    "workKey": "job-worker",
    "generation": 1,
    "endedAt": null,
    "outcome": null,
    "reservation": "r-1",
    "provider": "provider-a",
    "model": "model-a",
    "effort": "high",
    "tier": "top",
    "capacity": 4,
    "state": state,
    "enqueuedAt": 2_000,
    "leasedAt": null,
    "childId": child,
    "yielding": false,
    "placeKey": null,
    "turn": true
  })
}

fn pool_asking() -> PoolAsking {
  PoolAsking {
    request_key: "key-1".into(),
    owner: "owner-1".into(),
    source: "research".into(),
    role: "builder".into(),
    work_key: "job-worker".into(),
    provider: "provider-a".into(),
    model: "model-a".into(),
    effort: Some("high".into()),
    tier: "top".into(),
    yielding: false,
    place_key: None,
    turn: true,
    reservation: Some("r-1".into()),
  }
}

fn pool_request(state: PoolState, child: Option<&str>) -> PoolRequest {
  PoolRequest {
    asking: pool_asking(),
    token: "t-1".into(),
    state,
    enqueued_at: 2_000,
    leased_at: None,
    child: child.map(Into::into),
    ended_at: None,
    outcome: None,
  }
}

fn reservation_asking() -> ReservationAsking {
  ReservationAsking {
    reservation_key: "rk-1".into(),
    owner: "owner-1".into(),
    source: "research".into(),
    role: "builder".into(),
    provider: "provider-a".into(),
    model: "model-a".into(),
    effort: None,
    tier: "top".into(),
    wanted: 3,
  }
}

fn reservation_json() -> Value {
  json!({
    "token": "r-1",
    "reservationKey": "rk-1",
    "state": "held",
    "owner": "owner-1",
    "source": "research",
    "role": "builder",
    "provider": "provider-a",
    "model": "model-a",
    "effort": null,
    "tier": "top",
    "wanted": 3,
    "createdAt": 3_000
  })
}

fn reservation() -> Reservation {
  Reservation {
    asking: reservation_asking(),
    token: "r-1".into(),
    state: ReservationState::Held,
    created_at: 3_000,
  }
}

// Sessions.

#[tokio::test]
async fn start_session_posts_the_agent_and_its_scope() {
  let stub = Stub::start().await;
  let scope = json!({ "kind": "room", "room": "room-1" });

  stub.answer("POST /session/start", 200, json!({ "id": "s-1", "agent": "agent", "scope": scope }));

  let start = Start {
    agent: "agent".into(),
    scope: Scope::Room {
      room: "room-1".into(),
    },
  };
  let session = stub.client().start_session(&start).await.unwrap();

  assert_eq!(
    session,
    Session {
      id: session_id(),
      agent: "agent".into(),
      scope: start.scope.clone(),
    }
  );
  stub.took(
    "POST /session/start",
    "/session/start",
    Some(json!({ "agent": "agent", "scope": scope })),
  );
}

#[tokio::test]
async fn send_turn_posts_the_prompt_with_the_job_and_item_and_reads_the_delivery() {
  let stub = Stub::start().await;

  stub.answer("POST /session/turn", 200, json!({ "delivery": "blocked" }));

  let turn = Turn {
    session: session_id(),
    prompt: "go on".into(),
    context: TurnContext {
      job: Some(7),
      item: Some(12),
    },
  };
  let delivery = stub.client().send_turn(&turn).await.unwrap();

  assert_eq!(delivery, Delivery::Blocked);
  stub.took(
    "POST /session/turn",
    "/session/turn",
    Some(json!({ "session": "s-1", "prompt": "go on", "job": 7, "item": 12 })),
  );
}

#[tokio::test]
async fn push_words_posts_them_into_the_session_and_reads_what_was_said() {
  let stub = Stub::start().await;

  stub.answer("POST /session/push", 200, json!({ "said": "waiting" }));

  let said = stub.client().push_words(&session_id(), "Cover the tests too.").await.unwrap();

  assert_eq!(said, AsideOutcome::Waiting);
  stub.took(
    "POST /session/push",
    "/session/push",
    Some(json!({ "session": "s-1", "words": "Cover the tests too." })),
  );
}

#[tokio::test]
async fn session_state_reads_what_the_session_runs_on() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /session/state",
    200,
    json!({
      "session": "s-1",
      "provider": "provider-a",
      "model": "model-a",
      "place": "place-1",
      "generation": 2,
      "context": { "carried": 1_200, "window": 200_000 }
    }),
  );

  let state = stub.client().session_state(&session_id()).await.unwrap();

  assert_eq!(
    state,
    Some(SessionState {
      session: session_id(),
      provider: "provider-a".into(),
      model: "model-a".into(),
      place: "place-1".into(),
      generation: 2,
      context: Some(Context {
        carried: 1_200,
        window: 200_000,
      }),
    })
  );
  stub.took("GET /session/state", "/session/state?session=s-1", None);
}

#[tokio::test]
async fn interrupt_fold_and_end_each_post_the_session() {
  let stub = Stub::start().await;

  for route in ["POST /session/interrupt", "POST /session/fold", "POST /session/end"] {
    stub.answer(route, 200, json!({ "ok": true }));
  }

  let client = stub.client();

  client.interrupt_session(&session_id()).await.unwrap();
  client.fold_session(&session_id()).await.unwrap();
  client.end_session(&session_id()).await.unwrap();

  let took: Vec<(String, Option<Value>)> =
    stub.calls().into_iter().map(|call| (call.target, call.body)).collect();
  let body = Some(json!({ "session": "s-1" }));

  assert_eq!(
    took,
    [
      ("/session/interrupt".to_owned(), body.clone()),
      ("/session/fold".to_owned(), body.clone()),
      ("/session/end".to_owned(), body),
    ]
  );
}

#[tokio::test]
async fn sessions_lists_every_session() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /session/list",
    200,
    json!({ "sessions": [
      { "id": "s-1", "agent": "agent", "scope": { "kind": "agent" } },
      { "id": "s-2", "agent": "agent", "scope": { "kind": "worker" } }
    ] }),
  );

  let sessions = stub.client().sessions().await.unwrap();
  let scopes: Vec<Scope> = sessions.into_iter().map(|session| session.scope).collect();

  assert_eq!(scopes, [Scope::Agent, Scope::Worker]);
  stub.took("GET /session/list", "/session/list", None);
}

#[tokio::test]
async fn turn_events_hears_the_starts_and_ends_of_turns_until_the_stream_ends() {
  let stub = Stub::start().await;

  stub.answer_events(
    "GET /events",
    concat!(
      ": a comment between events\n\n",
      "event: turn-started\ndata: {\"session\":\"s-1\",\"run\":\"turn:a:1\",\"at\":1}\n\n",
      "event: road-walked\ndata: {\"stage\":\"chat\"}\n\n",
      "event: turn-ended\ndata: {\"session\":\"s-1\",\"run\":\"turn:a:1\",\"outcome\":\"failed\",",
      "\"answer\":null,\"reason\":\"it died\",\"engineError\":{\"kind\":\"rate_limit\"}}\n\n",
    ),
  );

  let events: Vec<TurnEvent> =
    stub.client().turn_events().await.unwrap().map(|event| event.unwrap()).collect().await;

  assert_eq!(
    events,
    [
      TurnEvent::Started(TurnStarted {
        session: session_id(),
        run: "turn:a:1".into(),
        at: 1,
      }),
      TurnEvent::Ended(TurnEnded {
        session: session_id(),
        run: "turn:a:1".into(),
        outcome: TurnOutcome::Failed,
        answer: None,
        reason: Some("it died".into()),
        engine_error: Some(EngineError {
          kind: Some("rate_limit".into()),
        }),
      }),
    ]
  );
  stub.took("GET /events", "/events?kinds=turn-started%2Cturn-ended", None);
}

#[tokio::test]
async fn every_call_carries_the_control_token() {
  let stub = Stub::start().await;

  stub.answer("GET /session/list", 200, json!({ "sessions": [] }));
  stub.answer("GET /tiers", 200, json!([]));

  let client = stub.client();

  client.sessions().await.unwrap();
  client.tiers().await.unwrap();

  let carried: Vec<Option<String>> = stub.calls().into_iter().map(|call| call.authorization).collect();
  let bearer = Some(format!("Bearer {TOKEN}"));

  assert_eq!(carried, [bearer.clone(), bearer]);
}

// Executions.

#[tokio::test]
async fn executions_asks_for_the_open_ones_only_where_told() {
  let stub = Stub::start().await;

  stub.answer("GET /executions", 200, json!([execution_json()]));

  let open = stub.client().executions(true).await.unwrap();
  let all = stub.client().executions(false).await.unwrap();
  let targets: Vec<String> = stub.calls().into_iter().map(|call| call.target).collect();

  assert_eq!(open, [execution()]);
  assert_eq!(all, open);
  assert_eq!(targets, ["/executions?open=true", "/executions?open=false"]);
}

#[tokio::test]
async fn execution_reads_one_with_its_details_as_they_stand() {
  let stub = Stub::start().await;

  stub.answer("GET /executions/{id}", 200, execution_json());

  let found = stub.client().execution("e-2").await.unwrap();

  assert_eq!(found, Some(execution()));
  stub.took("GET /executions/{id}", "/executions/e-2", None);
}

#[tokio::test]
async fn cancel_execution_asks_the_run_stopped_by_its_name() {
  let stub = Stub::start().await;

  stub.answer("POST /runs/stop", 200, json!({ "ok": true }));
  stub.client().cancel_execution("e-1").await.unwrap();
  stub.took("POST /runs/stop", "/runs/stop", Some(json!({ "run": "e-1" })));
}

#[tokio::test]
async fn drop_cancels_deletes_the_cancel_and_reads_those_dropped() {
  let stub = Stub::start().await;

  stub.answer("DELETE /executions/{id}/cancel", 200, json!({ "dropped": ["e-1", "e-2"] }));

  let dropped = stub.client().drop_cancels("e-1").await.unwrap();

  assert_eq!(dropped, ["e-1", "e-2"]);
  stub.took("DELETE /executions/{id}/cancel", "/executions/e-1/cancel", None);
}

// Runs.

#[tokio::test]
async fn open_run_posts_the_opening_and_reads_the_id() {
  let stub = Stub::start().await;

  stub.answer("POST /runs", 201, json!({ "id": 17 }));

  let opening = RunOpening {
    agent: "agent".into(),
    chat_id: Some(5),
    unit: "unit-1".into(),
    depth: 1,
    cap_ms: Some(60_000),
    started_at: 4_000,
    started_by: None,
    search_id: Some(12),
  };
  let id = stub.client().open_run(&opening).await.unwrap();

  assert_eq!(id, 17);
  stub.took(
    "POST /runs",
    "/runs",
    Some(json!({
      "agent": "agent",
      "chat_id": 5,
      "unit": "unit-1",
      "depth": 1,
      "cap_ms": 60_000,
      "started_at": 4_000,
      "started_by": null,
      "search_id": 12
    })),
  );
}

#[tokio::test]
async fn close_run_posts_the_closing() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /runs/{id}/close", 204);

  let closing = RunClosing {
    finished_at: 9_000,
    outcome: "done".into(),
    context_tokens: Some(30_000),
  };

  stub.client().close_run(17, &closing).await.unwrap();
  stub.took(
    "POST /runs/{id}/close",
    "/runs/17/close",
    Some(json!({ "finished_at": 9_000, "outcome": "done", "context_tokens": 30_000 })),
  );
}

#[tokio::test]
async fn run_reads_one_run() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /runs/{id}",
    200,
    json!({
      "id": 17,
      "agent": "agent",
      "chat_id": null,
      "unit": "unit-1",
      "depth": 2,
      "started_at": 4_000,
      "finished_at": null,
      "outcome": null,
      "context_tokens": null
    }),
  );

  let run = stub.client().run(17).await.unwrap();

  assert_eq!(
    run,
    Some(Run {
      id: 17,
      agent: "agent".into(),
      chat_id: None,
      unit: "unit-1".into(),
      depth: 2,
      started_at: 4_000,
      finished_at: None,
      outcome: None,
      context_tokens: None,
    })
  );
  stub.took("GET /runs/{id}", "/runs/17", None);
}

// The agent log.

#[tokio::test]
async fn run_starts_counts_the_agent_s_start_lines_in_the_run_s_log() {
  let stub = Stub::start().await;
  let line = |id: i64, agent: &str, kind: &str| json!({ "id": id, "agent": agent, "run": "run-17", "at": id, "kind": kind, "text": "", "jobId": null });

  stub.answer(
    "GET /run-log",
    200,
    json!({ "lines": [line(1, "agent", "start"), line(2, "agent", "tool"), line(3, "other", "start"), line(4, "agent", "start")] }),
  );

  let count = stub.client().run_starts("agent", "run-17").await.unwrap();

  assert_eq!(count, 2);
  stub.took("GET /run-log", "/run-log?run=run-17&after=0", None);
}

#[tokio::test]
async fn job_last_line_reads_the_line() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /log/jobs/{job}/last",
    200,
    json!({ "id": 80, "at": 5_000, "kind": "tool", "name": "Bash" }),
  );

  let line = stub.client().job_last_line(12).await.unwrap();

  assert_eq!(
    line,
    Some(LogLine {
      id: 80,
      at: 5_000,
      kind: "tool".into(),
      name: Some("Bash".into()),
    })
  );
  stub.took("GET /log/jobs/{job}/last", "/log/jobs/12/last", None);
}

#[tokio::test]
async fn job_lines_reads_the_calls_results_and_counts() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /log/jobs/{job}",
    200,
    json!({
      "calls": [{
        "id": 81,
        "agent": "agent",
        "run": 17,
        "reference": "call-1",
        "name": "Bash",
        "source": "event-1",
        "at": 5_100,
        "command": "cargo check",
        "args": "{\"command\":\"cargo check\"}"
      }],
      "results": [{
        "id": 82,
        "agent": "agent",
        "run": 17,
        "reference": "call-1",
        "complete": false,
        "length": 90_000,
        "at": 5_200
      }],
      "requests": 4,
      "compactions": 1
    }),
  );

  let lines = stub.client().job_lines(12).await.unwrap();

  assert_eq!(
    lines,
    JobLines {
      calls: vec![ToolCall {
        id: 81,
        agent: "agent".into(),
        run: Some(17),
        reference: Some("call-1".into()),
        name: "Bash".into(),
        source: Some("event-1".into()),
        at: 5_100,
        command: Some("cargo check".into()),
        args: Some("{\"command\":\"cargo check\"}".into()),
      }],
      results: vec![ToolResult {
        id: 82,
        agent: "agent".into(),
        run: Some(17),
        reference: Some("call-1".into()),
        complete: false,
        length: Some(90_000),
        at: 5_200,
      }],
      requests: 4,
      compactions: 1,
    }
  );
  stub.took("GET /log/jobs/{job}", "/log/jobs/12", None);
}

// Turn records.

#[tokio::test]
async fn keep_turn_record_posts_the_record_under_the_runs_name() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /turn-records", 204);

  let record = TurnRecord {
    run: "job:9".into(),
    kind: TurnRecordKind::TodoAdd,
    text: "Fix the launcher".into(),
    record_id: Some(41),
    wakes: false,
    at: 9_000,
  };

  stub.client().keep_turn_record(&record).await.unwrap();
  stub.took(
    "POST /turn-records",
    "/turn-records",
    Some(json!({
      "run": "job:9",
      "kind": "todo_add",
      "text": "Fix the launcher",
      "recordId": 41,
      "wakes": false,
      "at": 9_000
    })),
  );
}

#[tokio::test]
async fn words_appended_are_kept_as_their_own_kind() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /turn-records", 204);

  let record = TurnRecord {
    run: "chat:-5:1:1".into(),
    kind: TurnRecordKind::TodoAppend,
    text: "Check the docs too.".into(),
    record_id: None,
    wakes: false,
    at: 9_000,
  };

  stub.client().keep_turn_record(&record).await.unwrap();
  stub.took(
    "POST /turn-records",
    "/turn-records",
    Some(json!({
      "run": "chat:-5:1:1",
      "kind": "todo_append",
      "text": "Check the docs too.",
      "recordId": null,
      "wakes": false,
      "at": 9_000
    })),
  );
}

#[tokio::test]
async fn a_refused_turn_record_is_handed_on() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /turn-records",
    400,
    json!({ "error": "A turn record needs the run's name.", "code": "run_missing" }),
  );

  let record = TurnRecord {
    run: String::new(),
    kind: TurnRecordKind::TodoAdd,
    text: "Fix the launcher".into(),
    record_id: Some(41),
    wakes: false,
    at: 9_000,
  };

  match stub.client().keep_turn_record(&record).await {
    Err(AgencyError::Refused {
      call, status, code, ..
    }) => {
      assert_eq!((call, status, code.as_str()), ("keep_turn_record", 400, "run_missing"));
    }
    other => panic!("not handed on as a refusal: {other:?}"),
  }
}

// Workers' runtime.

#[tokio::test]
async fn worker_runtime_reads_the_worker_s_row_among_the_agents() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /agents",
    200,
    json!({
      "agents": [
        { "name": "agent", "kind": "chat" },
        {
          "name": "worker-2",
          "kind": "worker",
          "pinnedProvider": "provider-a",
          "pinnedModel": null,
          "keepalivePausedAt": 6_000,
          "contextTokens": 40_000
        }
      ],
      "roles": []
    }),
  );

  let runtime = stub.client().worker_runtime("worker-2").await.unwrap();

  assert_eq!(
    runtime,
    Some(WorkerRuntime {
      worker: "worker-2".into(),
      pin: Some(Pin {
        provider: "provider-a".into(),
        model: None,
      }),
      keepalive_paused_at: Some(6_000),
      context_tokens: Some(40_000),
    })
  );
  stub.took("GET /agents", "/agents", None);
  assert_eq!(stub.client().worker_runtime("worker-9").await.unwrap(), None);
}

#[tokio::test]
async fn a_support_worker_with_no_pin_of_its_own_is_answered_its_base_s() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /agents",
    200,
    json!({
      "agents": [
        { "name": "worker-2-1", "kind": "worker", "supportOf": "worker-2", "pinnedProvider": null },
        { "name": "worker-2", "kind": "worker", "pinnedProvider": "provider-a", "pinnedModel": "model-a" }
      ],
      "roles": []
    }),
  );

  let runtime = stub.client().worker_runtime("worker-2-1").await.unwrap().unwrap();

  assert_eq!(
    runtime.pin,
    Some(Pin {
      provider: "provider-a".into(),
      model: Some("model-a".into()),
    })
  );
}

#[tokio::test]
async fn pin_worker_updates_the_worker_s_pinned_provider() {
  let stub = Stub::start().await;
  let pin = Pin {
    provider: "provider-a".into(),
    model: Some("model-a".into()),
  };

  stub.answer("POST /worker", 200, json!({ "ok": true }));
  stub.client().pin_worker("worker-2", &pin).await.unwrap();
  stub.took(
    "POST /worker",
    "/worker",
    Some(json!({ "action": "update", "name": "worker-2", "pinnedProvider": "provider-a::model-a" })),
  );
}

#[tokio::test]
async fn unpin_worker_leaves_the_worker_to_its_walk() {
  let stub = Stub::start().await;

  stub.answer("POST /worker", 200, json!({ "ok": true }));
  stub.client().unpin_worker("worker-2").await.unwrap();
  stub.took(
    "POST /worker",
    "/worker",
    Some(json!({ "action": "update", "name": "worker-2", "pinnedProvider": "auto" })),
  );
}

#[tokio::test]
async fn pausing_and_resuming_the_keepalive_stop_and_start_it() {
  let stub = Stub::start().await;

  stub.answer("POST /worker", 200, json!({ "ok": true }));
  stub.client().pause_keepalive("worker-2").await.unwrap();
  stub.client().resume_keepalive("worker-2").await.unwrap();

  let bodies: Vec<Value> = stub.calls().into_iter().filter_map(|call| call.body).collect();

  assert_eq!(
    bodies,
    [
      json!({ "action": "update", "name": "worker-2", "keepaliveStopped": true }),
      json!({ "action": "update", "name": "worker-2", "keepaliveStopped": false }),
    ]
  );
}

#[tokio::test]
async fn a_worker_update_refused_is_handed_on() {
  let stub = Stub::start().await;

  stub.answer("POST /worker", 400, json!({ "error": "No provider answers.", "code": "bad-request" }));

  match stub.client().unpin_worker("worker-2").await {
    Err(AgencyError::Refused { call, status, .. }) => assert_eq!((call, status), ("unpin_worker", 400)),
    other => panic!("not handed on as a refusal: {other:?}"),
  }
}

#[tokio::test]
async fn say_to_worker_posts_the_words() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /workers/{worker}/words", 204);
  stub.client().say_to_worker("worker-2", "look at the logs").await.unwrap();
  stub.took(
    "POST /workers/{worker}/words",
    "/workers/worker-2/words",
    Some(json!({ "text": "look at the logs" })),
  );
}

// Routes, tiers and pins.

#[tokio::test]
async fn providers_lists_each_with_its_lines() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /providers",
    200,
    json!([{ "name": "provider-a", "enabled": true, "registered": false, "coworker": true, "lines": ["job"] }]),
  );

  let providers = stub.client().providers().await.unwrap();

  assert_eq!(
    providers,
    [Provider {
      name: "provider-a".into(),
      enabled: true,
      registered: false,
      coworker: true,
      lines: vec!["job".into()],
    }]
  );
  stub.took("GET /providers", "/providers", None);
}

#[tokio::test]
async fn place_cache_reads_the_lifetime_by_model_and_upstream() {
  let stub = Stub::start().await;

  stub.answer("GET /providers/{provider}/cache", 200, json!({ "ttlMs": 240_000 }));

  let client = stub.client();

  assert_eq!(client.place_cache("provider-a", "model-1", Some("up-1")).await.unwrap(), Some(240_000));
  stub.took(
    "GET /providers/{provider}/cache",
    "/providers/provider-a/cache?model=model-1&upstream=up-1",
    None,
  );

  stub.answer("GET /providers/{provider}/cache", 200, json!({ "ttlMs": null }));

  assert_eq!(client.place_cache("provider-a", "model-1", None).await.unwrap(), None);
}

#[tokio::test]
async fn pool_roles_lists_each_role_with_its_default_tier() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /pool/roles",
    200,
    json!([{ "name": "scout", "description": "Finds things.", "enabled": true, "pooled": true, "tier": "Light" }]),
  );

  assert_eq!(
    stub.client().pool_roles().await.unwrap(),
    [PoolRole {
      name: "scout".into(),
      description: "Finds things.".into(),
      enabled: true,
      pooled: true,
      tier: "Light".into(),
    }]
  );
  stub.took("GET /pool/roles", "/pool/roles", None);
}

#[tokio::test]
async fn pool_room_reads_the_tier_s_bucket_off_the_pool_s_board() {
  let stub = Stub::start().await;
  let tile = |state: &str| json!({ "state": state, "work": null });

  stub.answer(
    "GET /coworker-pool",
    200,
    json!({
      "readAt": 1,
      "buckets": [
        { "provider": "provider-b", "tier": "Light", "capacity": 9, "tiles": [], "waiting": [] },
        {
          "provider": "provider-a",
          "tier": "Light",
          "capacity": 4,
          "tiles": [tile("working"), tile("held"), tile("held"), tile("idle")],
          "waiting": [{ "kind": "cowork" }]
        }
      ],
      "workers": []
    }),
  );

  assert_eq!(
    stub.client().pool_room("provider-a", "Light").await.unwrap(),
    PoolRoom {
      capacity: 4,
      active: 1,
      reserved: 2,
      waiting: 1,
    }
  );
  stub.took("GET /coworker-pool", "/coworker-pool", None);
  assert_eq!(
    stub.client().pool_room("provider-a", "Heavy").await.unwrap(),
    PoolRoom {
      capacity: 0,
      active: 0,
      reserved: 0,
      waiting: 0,
    }
  );
}

#[tokio::test]
async fn retire_place_deletes_the_place_by_its_pool_and_number() {
  let stub = Stub::start().await;

  stub.answer_empty("DELETE /places/branch-slots/{pool}/{slot}", 204);
  stub.client().retire_place("pool-1", 3).await.unwrap();
  stub.took("DELETE /places/branch-slots/{pool}/{slot}", "/places/branch-slots/pool-1/3", None);
}

#[tokio::test]
async fn prefix_digest_posts_the_directory_and_reads_the_digest() {
  let stub = Stub::start().await;

  stub.answer("POST /places/prefix-digest", 200, json!({ "digest": "abc123" }));

  let digest = stub
    .client()
    .prefix_digest("/data/slots/pool-1/1", "model-1", Some("high"))
    .await
    .unwrap();

  assert_eq!(digest, "abc123");
  stub.took(
    "POST /places/prefix-digest",
    "/places/prefix-digest",
    Some(json!({ "cwd": "/data/slots/pool-1/1", "model": "model-1", "effort": "high" })),
  );
}

#[tokio::test]
async fn tiers_lists_each_tier() {
  let stub = Stub::start().await;

  stub.answer("GET /tiers", 200, json!([{ "name": "top", "coworker": false }]));

  let tiers = stub.client().tiers().await.unwrap();

  assert_eq!(
    tiers,
    [Tier {
      name: "top".into(),
      coworker: false,
    }]
  );
  stub.took("GET /tiers", "/tiers", None);
}

#[tokio::test]
async fn resolve_route_posts_the_ask_and_reads_the_mapping() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /routes/resolve",
    200,
    json!({
      "provider": "provider-a",
      "tier": "top",
      "model": "model-a",
      "effort": "high",
      "answerTokens": 32_000
    }),
  );

  let asked = RouteAsk {
    work_key: "job-worker".into(),
    provider: None,
    tier: "top".into(),
  };
  let mapping = stub.client().resolve_route(&asked).await.unwrap();

  assert_eq!(
    mapping,
    Some(Mapping {
      provider: "provider-a".into(),
      tier: "top".into(),
      model: "model-a".into(),
      effort: Some("high".into()),
      answer_tokens: Some(32_000),
    })
  );
  stub.took(
    "POST /routes/resolve",
    "/routes/resolve",
    Some(json!({ "workKey": "job-worker", "provider": null, "tier": "top" })),
  );
}

#[tokio::test]
async fn check_pin_posts_the_value_and_reads_the_choice() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /pins/check",
    200,
    json!({ "kind": "pinned", "provider": "provider-a", "model": "model-a" }),
  );

  let choice = stub.client().check_pin("provider-a::model-a").await.unwrap();

  assert_eq!(
    choice,
    PinChoice::Pinned(Pin {
      provider: "provider-a".into(),
      model: Some("model-a".into()),
    })
  );
  stub.took("POST /pins/check", "/pins/check", Some(json!({ "value": "provider-a::model-a" })));
}

#[tokio::test]
async fn check_pin_reads_auto() {
  let stub = Stub::start().await;

  stub.answer("POST /pins/check", 200, json!({ "kind": "auto" }));

  assert_eq!(stub.client().check_pin("auto").await.unwrap(), PinChoice::Auto);
}

// The co-worker pool.

#[tokio::test]
async fn enqueue_request_posts_what_agency_resolves_the_request_from() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/enqueue", 200, pool_request_json("waiting", None));

  let request = stub.client().enqueue_request(&pool_asking()).await.unwrap();

  assert_eq!(request, pool_request(PoolState::Waiting, None));
  stub.took(
    "POST /pool/enqueue",
    "/pool/enqueue",
    Some(json!({
      "source": "turn",
      "requestKey": "key-1",
      "owner": "owner-1",
      "role": "builder",
      "selection": "top",
      "tier": "top",
      "provider": "provider-a",
      "yielding": false,
      "placeKey": null
    })),
  );
}

#[tokio::test]
async fn a_branch_s_seat_is_enqueued_as_co_work() {
  let stub = Stub::start().await;
  let asking = PoolAsking {
    turn: false,
    ..pool_asking()
  };

  stub.answer("POST /pool/enqueue", 200, pool_request_json("waiting", None));
  stub.client().enqueue_request(&asking).await.unwrap();

  assert_eq!(stub.calls()[0].body.as_ref().unwrap()["source"], "cowork");
}

#[tokio::test]
async fn pool_request_reads_one_by_its_token() {
  let stub = Stub::start().await;

  stub.answer("GET /pool/requests/{token}", 200, pool_request_json("active", None));

  let request = stub.client().pool_request("t-1").await.unwrap();

  assert_eq!(request, Some(pool_request(PoolState::Active, None)));
  stub.took("GET /pool/requests/{token}", "/pool/requests/t-1", None);
}

#[tokio::test]
async fn open_requests_names_the_owner_only_where_given() {
  let stub = Stub::start().await;

  stub.answer("GET /pool/requests", 200, json!([pool_request_json("waiting", None)]));

  let owned = stub.client().open_requests(Some("owner-1")).await.unwrap();
  let every = stub.client().open_requests(None).await.unwrap();
  let targets: Vec<String> = stub.calls().into_iter().map(|call| call.target).collect();

  assert_eq!(owned, [pool_request(PoolState::Waiting, None)]);
  assert_eq!(every, owned);
  assert_eq!(targets, ["/pool/requests?owner=owner-1", "/pool/requests"]);
}

#[tokio::test]
async fn lease_place_asks_the_request_in_and_reads_it_or_none_while_it_waits() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/acquire", 200, json!({ "request": pool_request_json("active", None) }));

  let request = stub.client().lease_place("t-1").await.unwrap();

  assert_eq!(request.map(|request| request.state), Some(PoolState::Active));
  stub.took("POST /pool/acquire", "/pool/acquire", Some(json!({ "token": "t-1" })));

  stub.answer("POST /pool/acquire", 200, json!({ "request": null }));

  assert_eq!(stub.client().lease_place("t-1").await.unwrap(), None);
}

#[tokio::test]
async fn the_pool_s_states_are_read_as_bmo_job_s() {
  let stub = Stub::start().await;

  for (state, read) in [
    ("reserved", PoolState::Reserved),
    ("released", PoolState::Ended),
    ("cancelled", PoolState::Ended),
  ] {
    stub.answer("GET /pool/requests/{token}", 200, pool_request_json(state, None));

    assert_eq!(stub.client().pool_request("t-1").await.unwrap().unwrap().state, read);
  }
}

#[tokio::test]
async fn bind_child_posts_the_token_and_the_child() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/bind", 200, pool_request_json("active", Some("child-1")));

  let request = stub.client().bind_child("t-1", "child-1").await.unwrap();

  assert_eq!(request, pool_request(PoolState::Active, Some("child-1")));
  stub.took("POST /pool/bind", "/pool/bind", Some(json!({ "token": "t-1", "childId": "child-1" })));
}

#[tokio::test]
async fn finish_request_posts_the_outcome_and_cancel_and_reads_whether_it_was_held() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/finish", 200, json!({ "held": false }));

  let finished = stub.client().finish_request("t-1", "closed", true).await.unwrap();

  assert!(!finished);
  stub.took(
    "POST /pool/finish",
    "/pool/finish",
    Some(json!({ "token": "t-1", "outcome": "closed", "cancel": true })),
  );
}

#[tokio::test]
async fn reserve_seats_posts_what_agency_resolves_the_seats_from() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/reserve", 200, reservation_json());

  let held = stub.client().reserve_seats(&reservation_asking()).await.unwrap();

  assert_eq!(held, reservation());
  stub.took(
    "POST /pool/reserve",
    "/pool/reserve",
    Some(json!({
      "reservationKey": "rk-1",
      "owner": "owner-1",
      "source": "research",
      "role": "builder",
      "selection": "top",
      "tier": "top",
      "provider": "provider-a",
      "count": 3
    })),
  );
}

#[tokio::test]
async fn held_reservations_names_the_owner_where_given() {
  let stub = Stub::start().await;

  stub.answer("GET /pool/reservations", 200, json!({ "reservations": [reservation_json()] }));

  let held = stub.client().held_reservations(Some("owner-1")).await.unwrap();

  assert_eq!(held, [reservation()]);
  stub.took("GET /pool/reservations", "/pool/reservations?owner=owner-1", None);
}

#[tokio::test]
async fn release_reservation_posts_the_token_and_the_reason_as_its_outcome() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/reservations/release", 200, json!({ "held": true }));

  let released = stub.client().release_reservation("r-1", "research ended").await.unwrap();

  assert!(released);
  stub.took(
    "POST /pool/reservations/release",
    "/pool/reservations/release",
    Some(json!({ "token": "r-1", "outcome": "research ended" })),
  );
}

#[tokio::test]
async fn record_place_write_puts_the_write() {
  let stub = Stub::start().await;
  let write = PlaceWrite {
    provider: "provider-a".into(),
    model: "model-a".into(),
    effort: None,
    upstream: Some("upstream-1".into()),
  };

  stub.answer_empty("PUT /pool/places/{place}/write", 204);
  stub.client().record_place_write("place-1", &write).await.unwrap();
  stub.took(
    "PUT /pool/places/{place}/write",
    "/pool/places/place-1/write",
    Some(json!({
      "provider": "provider-a",
      "model": "model-a",
      "effort": null,
      "upstream": "upstream-1"
    })),
  );
}

// Nodes.

#[tokio::test]
async fn nodes_lists_each_node() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /nodes",
    200,
    json!([{
      "name": "node-1",
      "label": "Node one",
      "kind": "ssh",
      "role": null,
      "state": "up",
      "checkedAt": 7_000
    }]),
  );

  let nodes = stub.client().nodes().await.unwrap();

  assert_eq!(
    nodes,
    [Node {
      name: "node-1".into(),
      label: "Node one".into(),
      kind: "ssh".into(),
      role: None,
      state: "up".into(),
      checked_at: Some(7_000),
    }]
  );
  stub.took("GET /nodes", "/nodes", None);
}

#[tokio::test]
async fn ssh_reach_reads_how_ssh_reaches_a_node() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /nodes/{name}/reach",
    200,
    json!({ "host": "10.0.0.2", "port": 22, "user": "runner", "keyPath": null, "shell": "bash" }),
  );

  let reach = stub.client().ssh_reach("node-1").await.unwrap();

  assert_eq!(
    reach,
    Some(SshReach {
      host: "10.0.0.2".into(),
      port: 22,
      user: "runner".into(),
      key_path: None,
      shell: Some("bash".into()),
    })
  );
  stub.took("GET /nodes/{name}/reach", "/nodes/node-1/reach", None);
}

// Claude accounts.

#[tokio::test]
async fn claude_account_held_asks_by_user() {
  let stub = Stub::start().await;

  stub.answer("GET /accounts/claude/held", 200, json!({ "held": true }));

  let held = stub.client().claude_account_held("user-1").await.unwrap();

  assert!(held);
  stub.took("GET /accounts/claude/held", "/accounts/claude/held?user=user-1", None);
}

#[tokio::test]
async fn claude_homes_lists_the_record_homes() {
  let stub = Stub::start().await;

  stub.answer("GET /accounts/claude/homes", 200, json!({ "homes": ["/srv/home", "/srv/login-1"] }));

  assert_eq!(stub.client().claude_homes().await.unwrap(), ["/srv/home", "/srv/login-1"]);
  stub.took("GET /accounts/claude/homes", "/accounts/claude/homes", None);
}

// The deployment's rest.

#[tokio::test]
async fn resting_reads_the_deployment_s_rest() {
  let stub = Stub::start().await;

  stub.answer("GET /rest", 200, json!({ "resting": true, "until": 1_791_430_000, "kind": "limit" }));

  assert!(stub.client().resting().await.unwrap());
  stub.took("GET /rest", "/rest", None);
}

// The machine's room.

#[tokio::test]
async fn machine_room_reads_the_room_by_its_camel_case_keys() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /machine/room",
    200,
    json!({ "held": ["memory"], "jobs": 3, "browserJobs": 1, "browsers": 2 }),
  );

  let room = stub.client().machine_room().await.unwrap();

  assert_eq!(
    room,
    Room {
      held: vec!["memory".into()],
      jobs: 3,
      browser_jobs: 1,
      browsers: 2,
    }
  );
  stub.took("GET /machine/room", "/machine/room", None);
}

#[tokio::test]
async fn tell_launch_held_posts_the_notice_and_takes_its_ok() {
  let stub = Stub::start().await;
  let notice = LaunchHeld {
    at: 8_000,
    held: vec!["memory".into()],
  };

  stub.answer("POST /machine/launch-held", 200, json!({ "ok": true }));
  stub.client().tell_launch_held(&notice).await.unwrap();
  stub.took(
    "POST /machine/launch-held",
    "/machine/launch-held",
    Some(json!({ "at": 8_000, "held": ["memory"] })),
  );
}

#[tokio::test]
async fn room_returns_hears_only_the_room_returning_until_the_stream_ends() {
  let stub = Stub::start().await;

  stub.answer_events(
    "GET /events",
    concat!(
      ": the line is open\n\n",
      "event: machine-room-returned\ndata: {\"at\":8000,\"held\":[\"memory\"]}\n\n",
      "event: nodes-moved\ndata: {}\n\n",
      "event: machine-room-returned\ndata: {\"at\":9000,\"held\":[]}\n\n",
    ),
  );

  let heard: Vec<RoomReturned> = stub
    .client()
    .room_returns()
    .await
    .unwrap()
    .map(|returned| returned.unwrap())
    .collect()
    .await;

  assert_eq!(
    heard,
    [
      RoomReturned {
        at: 8_000,
        held: vec!["memory".into()],
      },
      RoomReturned {
        at: 9_000,
        held: vec![],
      },
    ]
  );
  stub.took("GET /events", "/events?kinds=machine-room-returned", None);
}

#[tokio::test]
async fn room_returns_hands_on_a_refused_stream() {
  let stub = Stub::start().await;

  stub.answer("GET /events", 503, json!({ "error": "Not now.", "code": "not-now" }));

  let Err(failed) = stub.client().room_returns().await else {
    panic!("a refused stream is a failure");
  };

  assert!(
    matches!(&failed, AgencyError::Refused { call: "room_returns", status: 503, code, .. } if code == "not-now"),
    "{failed}"
  );
}

// Places.

#[tokio::test]
async fn workbench_reads_the_root_and_scratches() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /places/workbench/{worker}",
    200,
    json!({ "root": "/data/bench/worker-2", "scratches": ["/data/bench/worker-2/item-5"] }),
  );

  let bench = stub.client().workbench("worker-2").await.unwrap();

  assert_eq!(
    bench,
    Workbench {
      root: PathBuf::from("/data/bench/worker-2"),
      scratches: vec![PathBuf::from("/data/bench/worker-2/item-5")],
    }
  );
  stub.took("GET /places/workbench/{worker}", "/places/workbench/worker-2", None);
}

#[tokio::test]
async fn branch_slots_reads_the_root_and_slots() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /places/branch-slots/{pool}",
    200,
    json!({ "root": "/data/slots", "slots": [{ "slot": 1, "dir": "/data/slots/pool-1/1" }] }),
  );

  let slots = stub.client().branch_slots("pool-1").await.unwrap();

  assert_eq!(
    slots,
    BranchSlots {
      root: PathBuf::from("/data/slots"),
      slots: vec![Slot {
        slot: 1,
        dir: PathBuf::from("/data/slots/pool-1/1"),
      }],
    }
  );
  stub.took("GET /places/branch-slots/{pool}", "/places/branch-slots/pool-1", None);
}

// How answers and failures are read.

#[tokio::test]
async fn a_read_of_one_thing_that_does_not_exist_is_none() {
  let stub = Stub::start().await;
  let held = json!({ "error": "Nothing is kept there.", "code": "nothing-held" });

  stub.answer(
    "GET /session/state",
    404,
    json!({ "error": "No such session.", "code": "unknown-session" }),
  );

  for route in [
    "GET /executions/{id}",
    "GET /runs/{id}",
    "GET /log/jobs/{job}/last",
    "POST /routes/resolve",
    "GET /pool/requests/{token}",
    "GET /nodes/{name}/reach",
  ] {
    stub.answer(route, 404, held.clone());
  }

  let client = stub.client();
  let asked = RouteAsk {
    work_key: "job-worker".into(),
    provider: Some("provider-a".into()),
    tier: "top".into(),
  };

  assert_eq!(client.session_state(&session_id()).await.unwrap(), None);
  assert_eq!(client.execution("e-9").await.unwrap(), None);
  assert_eq!(client.run(99).await.unwrap(), None);
  assert_eq!(client.job_last_line(99).await.unwrap(), None);
  assert_eq!(client.resolve_route(&asked).await.unwrap(), None);
  assert_eq!(client.pool_request("t-9").await.unwrap(), None);
  assert_eq!(client.ssh_reach("node-9").await.unwrap(), None);
  assert_eq!(stub.calls().len(), 7);
}

#[tokio::test]
async fn a_404_with_any_other_code_is_a_refusal_not_nothing_found() {
  let stub = Stub::start().await;

  stub.answer("GET /executions/{id}", 404, json!({ "error": "not found", "code": "not-found" }));

  let unserved = stub.client().execution("e-9").await.unwrap_err();

  assert!(
    matches!(&unserved, AgencyError::Refused { call: "execution", status: 404, code, .. } if code == "not-found"),
    "{unserved}"
  );
}

#[tokio::test]
async fn a_refusal_is_handed_on_with_its_status_code_and_sentence() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /session/turn",
    409,
    json!({ "error": "s-1 is in a turn", "code": "session-busy" }),
  );
  stub.answer(
    "POST /session/fold",
    404,
    json!({ "error": "no worker is named agent", "code": "unknown-session" }),
  );

  let turn = Turn {
    session: session_id(),
    prompt: "go on".into(),
    context: TurnContext::default(),
  };
  let busy = stub.client().send_turn(&turn).await.unwrap_err();
  let gone = stub.client().fold_session(&session_id()).await.unwrap_err();

  assert!(
    matches!(
      &busy,
      AgencyError::Refused { call: "send_turn", status: 409, code, error }
        if code == "session-busy" && error == "s-1 is in a turn"
    ),
    "{busy}"
  );
  assert!(
    matches!(
      &gone,
      AgencyError::Refused { call: "fold_session", status: 404, code, .. } if code == "unknown-session"
    ),
    "{gone}"
  );
}

#[tokio::test]
async fn an_answer_that_does_not_read_is_malformed() {
  let stub = Stub::start().await;

  stub.answer("GET /tiers", 200, json!({ "tiers": [] }));
  stub.answer("GET /nodes", 502, json!("bad gateway"));

  let unread = stub.client().tiers().await.unwrap_err();
  let unsaid = stub.client().nodes().await.unwrap_err();

  assert!(
    matches!(
      unread,
      AgencyError::Malformed {
        call: "tiers",
        status: 200,
        ..
      }
    ),
    "{unread}"
  );
  assert!(
    matches!(
      unsaid,
      AgencyError::Malformed {
        call: "nodes",
        status: 502,
        ..
      }
    ),
    "{unsaid}"
  );
}

#[tokio::test]
async fn a_call_that_reaches_nothing_is_unreachable() {
  let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let address = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();

  drop(listener);

  let failed = AgencyClient::new(address, TOKEN).unwrap().sessions().await.unwrap_err();

  assert!(
    matches!(
      failed,
      AgencyError::Unreachable {
        call: "sessions",
        ..
      }
    ),
    "{failed}"
  );
}

#[test]
fn an_address_that_is_not_http_is_refused() {
  for address in ["https://agency.invalid/", "mailto:agency@agency.invalid", "file:///agency"] {
    let refused = AgencyClient::new(Url::parse(address).unwrap(), TOKEN).unwrap_err();

    assert!(matches!(refused, AgencyError::NotAnAddress { .. }), "{address}: {refused}");
  }
}

#[tokio::test]
async fn a_path_segment_is_percent_encoded() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /workers/{worker}/words", 204);
  stub.client().say_to_worker("a b/c?d", "hello").await.unwrap();
  stub.took(
    "POST /workers/{worker}/words",
    "/workers/a%20b%2Fc%3Fd/words",
    Some(json!({ "text": "hello" })),
  );
}
