//! The agency port's contract: every call is made on the method, path and body the API document gives
//! it, and its answer is read as the call's shape (docs/api.md, "bmo-agency").

mod support;

use std::path::PathBuf;

use job_agency::{
  Agency, AgencyClient, AgencyError, BranchSlots, Context, Delivery, Execution, JobLines, LaunchHeld,
  LogLine, Mapping, Node, Output, Pin, PinChoice, PlaceWrite, PoolAsking, PoolRequest, PoolRole, PoolRoom,
  PoolState, Provider, Reservation, ReservationAsking, ReservationState, Room, RouteAsk, Run, RunClosing,
  RunOpening, Scope, Session, SessionId, SessionState, Slot, SshReach, Start, Tier, ToolCall, ToolResult,
  TurnRecord, TurnRecordKind, WorkMachine, Workbench, WorkerRuntime,
};
use serde_json::{Map, Value, json};
use support::agency::Stub;
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
    "shared_pane": true,
    "started_at": 1_000,
    "ended_at": null,
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

fn pool_asking_json() -> Value {
  json!({
    "request_key": "key-1",
    "owner": "owner-1",
    "source": "research",
    "role": "builder",
    "work_key": "job-worker",
    "provider": "provider-a",
    "model": "model-a",
    "effort": "high",
    "tier": "top",
    "yielding": false,
    "place_key": null,
    "turn": true,
    "reservation": "r-1"
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

fn pool_request_json(state: &str, child: Option<&str>) -> Value {
  let mut request = object(pool_asking_json());

  request.extend(object(json!({
    "token": "t-1",
    "state": state,
    "enqueued_at": 2_000,
    "leased_at": null,
    "child": child,
    "ended_at": null,
    "outcome": null
  })));

  Value::Object(request)
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

fn reservation_asking_json() -> Value {
  json!({
    "reservation_key": "rk-1",
    "owner": "owner-1",
    "source": "research",
    "role": "builder",
    "provider": "provider-a",
    "model": "model-a",
    "effort": null,
    "tier": "top",
    "wanted": 3
  })
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
  let mut reservation = object(reservation_asking_json());

  reservation.extend(object(json!({ "token": "r-1", "state": "held", "created_at": 3_000 })));

  Value::Object(reservation)
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

  stub.answer("POST /sessions", 201, json!({ "id": "s-1", "agent": "agent", "scope": scope }));

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
  stub.took("POST /sessions", "/sessions", Some(json!({ "agent": "agent", "scope": scope })));
}

#[tokio::test]
async fn send_turn_posts_the_prompt_and_reads_the_delivery() {
  let stub = Stub::start().await;

  stub.answer("POST /sessions/{id}/turns", 200, json!({ "delivery": "blocked" }));

  let delivery = stub.client().send_turn(&session_id(), "go on").await.unwrap();

  assert_eq!(delivery, Delivery::Blocked);
  stub.took("POST /sessions/{id}/turns", "/sessions/s-1/turns", Some(json!({ "prompt": "go on" })));
}

#[tokio::test]
async fn session_state_reads_what_the_session_runs_on() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /sessions/{id}/state",
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
  stub.took("GET /sessions/{id}/state", "/sessions/s-1/state", None);
}

#[tokio::test]
async fn session_output_reads_the_latest_turns_text() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /sessions/{id}/output",
    200,
    json!({ "session": "s-1", "generation": 0, "text": "Working on it." }),
  );

  let output = stub.client().session_output(&session_id()).await.unwrap();

  assert_eq!(
    output,
    Some(Output {
      session: session_id(),
      generation: 0,
      text: "Working on it.".into(),
    })
  );
  stub.took("GET /sessions/{id}/output", "/sessions/s-1/output", None);
}

#[tokio::test]
async fn interrupt_session_posts_with_no_body() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /sessions/{id}/interrupt", 204);
  stub.client().interrupt_session(&session_id()).await.unwrap();
  stub.took("POST /sessions/{id}/interrupt", "/sessions/s-1/interrupt", None);
}

#[tokio::test]
async fn fold_session_posts_with_no_body() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /sessions/{id}/fold", 204);
  stub.client().fold_session(&session_id()).await.unwrap();
  stub.took("POST /sessions/{id}/fold", "/sessions/s-1/fold", None);
}

#[tokio::test]
async fn end_session_deletes_it() {
  let stub = Stub::start().await;

  stub.answer_empty("DELETE /sessions/{id}", 204);
  stub.client().end_session(&session_id()).await.unwrap();
  stub.took("DELETE /sessions/{id}", "/sessions/s-1", None);
}

#[tokio::test]
async fn sessions_lists_every_session() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /sessions",
    200,
    json!([
      { "id": "s-1", "agent": "agent", "scope": { "kind": "agent" } },
      { "id": "s-2", "agent": "agent", "scope": { "kind": "worker" } }
    ]),
  );

  let sessions = stub.client().sessions().await.unwrap();
  let scopes: Vec<Scope> = sessions.into_iter().map(|session| session.scope).collect();

  assert_eq!(scopes, [Scope::Agent, Scope::Worker]);
  stub.took("GET /sessions", "/sessions", None);
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
async fn cancel_execution_posts_the_cancel() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /executions/{id}/cancel", 204);
  stub.client().cancel_execution("e-1").await.unwrap();
  stub.took("POST /executions/{id}/cancel", "/executions/e-1/cancel", None);
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
    search_id: Some("search-1".into()),
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
      "search_id": "search-1"
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
async fn run_starts_counts_by_agent_and_run() {
  let stub = Stub::start().await;

  stub.answer("GET /log/starts", 200, json!({ "count": 3 }));

  let count = stub.client().run_starts("agent", "run-17").await.unwrap();

  assert_eq!(count, 3);
  stub.took("GET /log/starts", "/log/starts?agent=agent&run=run-17", None);
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
      "record_id": 41,
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
      "record_id": null,
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
async fn worker_runtime_reads_the_pin_pause_and_context() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /workers/{worker}/runtime",
    200,
    json!({
      "worker": "worker-2",
      "pin": { "provider": "provider-a", "model": null },
      "keepalive_paused_at": 6_000,
      "context_tokens": 40_000
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
  stub.took("GET /workers/{worker}/runtime", "/workers/worker-2/runtime", None);
}

#[tokio::test]
async fn pin_worker_puts_the_pin() {
  let stub = Stub::start().await;
  let pin = Pin {
    provider: "provider-a".into(),
    model: Some("model-a".into()),
  };

  stub.answer_empty("PUT /workers/{worker}/pin", 204);
  stub.client().pin_worker("worker-2", &pin).await.unwrap();
  stub.took(
    "PUT /workers/{worker}/pin",
    "/workers/worker-2/pin",
    Some(json!({ "provider": "provider-a", "model": "model-a" })),
  );
}

#[tokio::test]
async fn unpin_worker_deletes_the_pin() {
  let stub = Stub::start().await;

  stub.answer_empty("DELETE /workers/{worker}/pin", 204);
  stub.client().unpin_worker("worker-2").await.unwrap();
  stub.took("DELETE /workers/{worker}/pin", "/workers/worker-2/pin", None);
}

#[tokio::test]
async fn pause_keepalive_puts_the_time() {
  let stub = Stub::start().await;

  stub.answer_empty("PUT /workers/{worker}/keepalive-pause", 204);
  stub.client().pause_keepalive("worker-2", 6_000).await.unwrap();
  stub.took(
    "PUT /workers/{worker}/keepalive-pause",
    "/workers/worker-2/keepalive-pause",
    Some(json!({ "at": 6_000 })),
  );
}

#[tokio::test]
async fn resume_keepalive_deletes_the_pause() {
  let stub = Stub::start().await;

  stub.answer_empty("DELETE /workers/{worker}/keepalive-pause", 204);
  stub.client().resume_keepalive("worker-2").await.unwrap();
  stub.took("DELETE /workers/{worker}/keepalive-pause", "/workers/worker-2/keepalive-pause", None);
}

#[tokio::test]
async fn record_context_puts_the_tokens() {
  let stub = Stub::start().await;

  stub.answer_empty("PUT /workers/{worker}/context-tokens", 204);
  stub.client().record_context("worker-2", 40_000).await.unwrap();
  stub.took(
    "PUT /workers/{worker}/context-tokens",
    "/workers/worker-2/context-tokens",
    Some(json!({ "tokens": 40_000 })),
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

  stub.answer("GET /providers/{provider}/cache", 200, json!({ "ttl_ms": 240_000 }));

  let client = stub.client();

  assert_eq!(client.place_cache("provider-a", "model-1", Some("up-1")).await.unwrap(), Some(240_000));
  stub.took(
    "GET /providers/{provider}/cache",
    "/providers/provider-a/cache?model=model-1&upstream=up-1",
    None,
  );

  stub.answer("GET /providers/{provider}/cache", 200, json!({ "ttl_ms": null }));

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
async fn pool_room_asks_by_provider_and_tier() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /pool/room",
    200,
    json!({ "capacity": 4, "active": 1, "reserved": 2, "waiting": 0 }),
  );

  assert_eq!(
    stub.client().pool_room("provider-a", "Light").await.unwrap(),
    PoolRoom {
      capacity: 4,
      active: 1,
      reserved: 2,
      waiting: 0,
    }
  );
  stub.took("GET /pool/room", "/pool/room?provider=provider-a&tier=Light", None);
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
      "answer_tokens": 32_000
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
    Some(json!({ "work_key": "job-worker", "provider": null, "tier": "top" })),
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
async fn enqueue_request_posts_the_asking_and_reads_the_request() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/requests", 201, pool_request_json("waiting", None));

  let request = stub.client().enqueue_request(&pool_asking()).await.unwrap();

  assert_eq!(request, pool_request(PoolState::Waiting, None));
  stub.took("POST /pool/requests", "/pool/requests", Some(pool_asking_json()));
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
async fn lease_place_posts_and_reads_the_request() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/requests/{token}/lease", 200, pool_request_json("active", None));

  let request = stub.client().lease_place("t-1").await.unwrap();

  assert_eq!(request.state, PoolState::Active);
  stub.took("POST /pool/requests/{token}/lease", "/pool/requests/t-1/lease", None);
}

#[tokio::test]
async fn bind_child_posts_the_child() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/requests/{token}/bind", 200, pool_request_json("active", Some("child-1")));

  let request = stub.client().bind_child("t-1", "child-1").await.unwrap();

  assert_eq!(request, pool_request(PoolState::Active, Some("child-1")));
  stub.took(
    "POST /pool/requests/{token}/bind",
    "/pool/requests/t-1/bind",
    Some(json!({ "child": "child-1" })),
  );
}

#[tokio::test]
async fn finish_request_posts_the_outcome_and_cancel() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/requests/{token}/finish", 200, json!({ "finished": false }));

  let finished = stub.client().finish_request("t-1", "closed", true).await.unwrap();

  assert!(!finished);
  stub.took(
    "POST /pool/requests/{token}/finish",
    "/pool/requests/t-1/finish",
    Some(json!({ "outcome": "closed", "cancel": true })),
  );
}

#[tokio::test]
async fn reserve_seats_posts_the_asking_and_reads_the_reservation() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/reservations", 201, reservation_json());

  let held = stub.client().reserve_seats(&reservation_asking()).await.unwrap();

  assert_eq!(held, reservation());
  stub.took("POST /pool/reservations", "/pool/reservations", Some(reservation_asking_json()));
}

#[tokio::test]
async fn held_reservations_names_the_owner_where_given() {
  let stub = Stub::start().await;

  stub.answer("GET /pool/reservations", 200, json!([reservation_json()]));

  let held = stub.client().held_reservations(Some("owner-1")).await.unwrap();

  assert_eq!(held, [reservation()]);
  stub.took("GET /pool/reservations", "/pool/reservations?owner=owner-1", None);
}

#[tokio::test]
async fn release_reservation_posts_the_reason() {
  let stub = Stub::start().await;

  stub.answer("POST /pool/reservations/{token}/release", 200, json!({ "released": true }));

  let released = stub.client().release_reservation("r-1", "research ended").await.unwrap();

  assert!(released);
  stub.took(
    "POST /pool/reservations/{token}/release",
    "/pool/reservations/r-1/release",
    Some(json!({ "reason": "research ended" })),
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
      "checked_at": 7_000
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
    json!({ "host": "10.0.0.2", "port": 22, "user": "runner", "key_path": null, "shell": "bash" }),
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

#[tokio::test]
async fn place_heavy_run_reads_the_machine_or_none() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /nodes/heavy-run",
    200,
    json!({ "machine": { "name": "node-1", "label": "Node one", "node": "node-1" } }),
  );

  let placed = stub.client().place_heavy_run().await.unwrap();

  assert_eq!(
    placed,
    Some(WorkMachine {
      name: "node-1".into(),
      label: "Node one".into(),
      node: Some("node-1".into()),
    })
  );
  stub.took("POST /nodes/heavy-run", "/nodes/heavy-run", None);

  stub.answer("POST /nodes/heavy-run", 200, json!({ "machine": null }));

  assert_eq!(stub.client().place_heavy_run().await.unwrap(), None);
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

// An agent's rest.

#[tokio::test]
async fn resting_reads_the_agents_rest() {
  let stub = Stub::start().await;

  stub.answer("GET /agents/{agent}/rest", 200, json!({ "resting": true }));

  assert!(stub.client().resting("agent").await.unwrap());
  stub.took("GET /agents/{agent}/rest", "/agents/agent/rest", None);
}

// The machine's room.

#[tokio::test]
async fn machine_room_reads_the_room_with_its_details_as_they_stand() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /machine/room",
    200,
    json!({
      "held": ["memory"],
      "jobs": 3,
      "browser_jobs": 1,
      "browsers": 2,
      "details": { "testbed": { "busy": true } }
    }),
  );

  let room = stub.client().machine_room().await.unwrap();

  assert_eq!(
    room,
    Room {
      held: vec!["memory".into()],
      jobs: 3,
      browser_jobs: 1,
      browsers: 2,
      details: object(json!({ "testbed": { "busy": true } })),
    }
  );
  stub.took("GET /machine/room", "/machine/room", None);
}

#[tokio::test]
async fn tell_launch_held_puts_the_notice() {
  let stub = Stub::start().await;
  let notice = LaunchHeld {
    at: 8_000,
    held: vec!["memory".into()],
  };

  stub.answer_empty("PUT /machine/launch-held", 204);
  stub.client().tell_launch_held(&notice).await.unwrap();
  stub.took(
    "PUT /machine/launch-held",
    "/machine/launch-held",
    Some(json!({ "at": 8_000, "held": ["memory"] })),
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
  let missing = json!({ "error": "No such thing.", "code": "unknown" });

  for route in [
    "GET /sessions/{id}/state",
    "GET /sessions/{id}/output",
    "GET /executions/{id}",
    "GET /runs/{id}",
    "GET /log/jobs/{job}/last",
    "GET /workers/{worker}/runtime",
    "POST /routes/resolve",
    "GET /pool/requests/{token}",
    "GET /nodes/{name}/reach",
  ] {
    stub.answer(route, 404, missing.clone());
  }

  let client = stub.client();
  let asked = RouteAsk {
    work_key: "job-worker".into(),
    provider: Some("provider-a".into()),
    tier: "top".into(),
  };

  assert_eq!(client.session_state(&session_id()).await.unwrap(), None);
  assert_eq!(client.session_output(&session_id()).await.unwrap(), None);
  assert_eq!(client.execution("e-9").await.unwrap(), None);
  assert_eq!(client.run(99).await.unwrap(), None);
  assert_eq!(client.job_last_line(99).await.unwrap(), None);
  assert_eq!(client.worker_runtime("worker-9").await.unwrap(), None);
  assert_eq!(client.resolve_route(&asked).await.unwrap(), None);
  assert_eq!(client.pool_request("t-9").await.unwrap(), None);
  assert_eq!(client.ssh_reach("node-9").await.unwrap(), None);
  assert_eq!(stub.calls().len(), 9);
}

#[tokio::test]
async fn a_refusal_is_handed_on_with_its_status_code_and_sentence() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /sessions/{id}/turns",
    409,
    json!({ "error": "The session is busy with another turn.", "code": "session_busy" }),
  );
  stub.answer(
    "POST /sessions/{id}/fold",
    404,
    json!({ "error": "No such session.", "code": "unknown" }),
  );

  let busy = stub.client().send_turn(&session_id(), "go on").await.unwrap_err();
  let gone = stub.client().fold_session(&session_id()).await.unwrap_err();

  assert!(
    matches!(
      &busy,
      AgencyError::Refused { call: "send_turn", status: 409, code, error }
        if code == "session_busy" && error == "The session is busy with another turn."
    ),
    "{busy}"
  );
  assert!(
    matches!(&gone, AgencyError::Refused { call: "fold_session", status: 404, code, .. } if code == "unknown"),
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

  let failed = AgencyClient::new(address).unwrap().sessions().await.unwrap_err();

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
    let refused = AgencyClient::new(Url::parse(address).unwrap()).unwrap_err();

    assert!(matches!(refused, AgencyError::NotAnAddress { .. }), "{address}: {refused}");
  }
}

#[tokio::test]
async fn a_path_segment_is_percent_encoded() {
  let stub = Stub::start().await;

  stub.answer_empty("DELETE /workers/{worker}/pin", 204);
  stub.client().unpin_worker("a b/c?d").await.unwrap();
  stub.took("DELETE /workers/{worker}/pin", "/workers/a%20b%2Fc%3Fd/pin", None);
}
