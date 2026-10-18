//! The agency port's contract for a job's work turns and the upkeep of a worker's sessions: a work turn is
//! carried on the session contract and read as it ended, and every other call is made on the method, path and
//! body the API document gives it, its answer read as the call's shape (docs/api.md, "Work turns", "Workers'
//! runtime", "Claude accounts", "Nodes").

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use job_agency::{
  Agency, AgencyClient, AgencyError, AsideOutcome, ConfirmAnswer, Handover, JobDocuments, LiveTask,
  LogNote, ModelSpend, Reply, Resume, RunWorkflow, SessionAnswered, SessionKept, TurnAsking, TurnFailure,
  TurnMapping, TurnSeat, TurnState, WentOut, WorkTurn, WorkTurns,
};
use serde_json::{Value, json};
use support::agency::Stub;

fn asking() -> TurnAsking {
  TurnAsking {
    agent: "worker".into(),
    seat: TurnSeat::Branch {
      todo: Some(7),
      role: Some("scout".into()),
    },
    provider: "claude-code".into(),
    model: "model-a".into(),
    mapping: TurnMapping {
      model: "model-a".into(),
      effort: Some("high".into()),
      tier: Some("Fine".into()),
      answer_tokens: None,
    },
    resume: Resume::Session { id: "s-1".into() },
    cwd: "/srv/checkout".into(),
    name: "bmo-work-7-plan-1".into(),
    title: "#7 Fix the gate".into(),
    thread: Some("todo:7".into()),
    job: Some(41),
    item: Some(7),
    chat: 100,
    unit: Some("bmo-work-7-plan-1".into()),
    user: None,
    work_key: "job-worker".into(),
    hears_words: false,
    variables: BTreeMap::from([("BMO_RESEARCH_CHAT".to_owned(), "100".to_owned())]),
  }
}

fn reply() -> Reply {
  Reply {
    mode: "claude-code".into(),
    model: Some("model-a".into()),
    effort: None,
    text: "done".into(),
    session_id: Some("s-2".into()),
    account: Some("a-1".into()),
    context_ratio: Some(0.5),
    context_tokens: Some(1200),
    session_usage: None,
    spend: vec![ModelSpend {
      mode: None,
      model: "model-a".into(),
      tokens: 10,
      input_tokens: 4,
      output_tokens: 3,
      cache_read_tokens: 2,
      cache_write_tokens: 1,
    }],
    live_tasks: vec![LiveTask {
      id: "b1".into(),
      description: "dev server".into(),
    }],
    workflows: vec![RunWorkflow {
      call: "c1".into(),
      name: "review".into(),
      status: Some("completed".into()),
    }],
    upstream: None,
    request_usages: None,
  }
}

/// The id bmo-agency knows the worker's session by.
const SESSION: &str = "worker@worker";

fn worker_asking() -> TurnAsking {
  TurnAsking {
    seat: TurnSeat::Worker,
    ..asking()
  }
}

/// The stand-in answering a worker's session started, its state after the turn, and a stream that tells `ended`
/// of the turn's run, after a turn of another session.
fn carrying(stub: &Stub, ended: Value) {
  let started = |session: &str, run: &str| json!({ "session": session, "run": run, "at": 1 });
  let mut ended = ended;

  ended["session"] = json!(SESSION);
  ended["run"] = json!("turn:worker:2");
  stub.answer(
    "POST /session/start",
    200,
    json!({ "id": SESSION, "agent": "worker", "scope": { "kind": "worker" } }),
  );
  stub.answer(
    "GET /session/state",
    200,
    json!({
      "session": SESSION,
      "provider": "provider-a",
      "model": "model-b",
      "place": "place-1",
      "generation": 1,
      "context": { "carried": 50_000, "window": 200_000 }
    }),
  );
  stub.answer_events(
    "GET /events",
    &format!(
      "event: turn-started\ndata: {}\n\nevent: turn-started\ndata: {}\n\nevent: turn-ended\ndata: {}\n\n",
      started("other@worker", "turn:other:1"),
      started(SESSION, "turn:worker:2"),
      ended,
    ),
  );
}

/// The turn read until it stands in an ending, a bounded wait on its background send.
async fn ending(turns: &WorkTurns<AgencyClient>, turn: &str) -> WorkTurn {
  for _ in 0..500 {
    let read = turns.read(turn).expect("the turn is carried");

    if !matches!(read.state, TurnState::Running { .. }) {
      return read;
    }

    tokio::time::sleep(Duration::from_millis(10)).await;
  }

  panic!("the turn never ended");
}

/// A worker's turn carried to its end on the stand-in, read in its ending.
async fn carried(stub: &Stub) -> WorkTurn {
  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  turns.send(&prepared.turn, "Do the work.").await.unwrap();
  ending(&turns, &prepared.turn).await
}

#[tokio::test]
async fn a_worker_s_turn_is_prepared_on_the_worker_s_session_with_none_to_continue() {
  let stub = Stub::start().await;

  carrying(&stub, json!({ "outcome": "answered", "answer": "done" }));

  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  assert_eq!(prepared.session, None);
  assert!(prepared.turn.starts_with(SESSION), "{}", prepared.turn);
  stub.took(
    "POST /session/start",
    "/session/start",
    Some(json!({ "agent": "worker", "scope": { "kind": "worker" } })),
  );
}

#[tokio::test]
async fn a_branch_s_turn_is_refused_before_anything_is_sent() {
  let stub = Stub::start().await;
  let turns = WorkTurns::new(stub.client());
  let refused = turns.prepare(&asking()).await.unwrap_err();

  assert!(
    matches!(
      refused,
      AgencyError::Uncarried {
        call: "prepare_turn",
        ..
      }
    ),
    "{refused}"
  );
  assert_eq!(stub.calls(), []);
}

#[tokio::test]
async fn an_answered_turn_reads_its_answer_on_what_the_session_stood_on_and_is_then_let_go() {
  let stub = Stub::start().await;

  carrying(&stub, json!({ "outcome": "answered", "answer": "done" }));
  stub.answer("POST /session/turn", 200, json!({ "delivery": "taken" }));

  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  turns.send(&prepared.turn, "Do the work.").await.unwrap();

  let read = ending(&turns, &prepared.turn).await;
  let TurnState::Answered { reply } = read.state else {
    panic!("the turn answered: {:?}", read.state);
  };

  assert_eq!(
    (reply.text.as_str(), reply.mode.as_str(), reply.model.as_deref()),
    ("done", "provider-a", Some("model-b"))
  );
  assert_eq!((reply.context_tokens, reply.context_ratio), (Some(50_000), Some(0.25)));
  assert_eq!((reply.session_id.as_deref(), read.session.as_deref()), (Some(SESSION), Some(SESSION)));
  assert_eq!(
    read.went_out,
    Some(WentOut {
      provider: "provider-a".into(),
      model: "model-b".into(),
      effort: None,
      tier: None,
    })
  );
  assert_eq!(turns.read(&prepared.turn), None);

  let sent = stub.calls().into_iter().find(|call| call.route == "POST /session/turn").unwrap();

  assert_eq!(
    sent.body,
    Some(json!({ "session": SESSION, "prompt": "Do the work.", "job": 41, "item": 7 }))
  );
}

#[tokio::test]
async fn a_turn_that_did_not_answer_reads_by_how_it_ended() {
  let person = TurnState::Failed {
    failure: Some(TurnFailure::Person),
    evidence: None,
  };
  let cases = [
    (json!({ "delivery": "blocked" }), json!({ "outcome": "blocked" }), person.clone()),
    (json!({ "delivery": "taken" }), json!({ "outcome": "blocked" }), person),
    (
      json!({ "delivery": "taken" }),
      json!({ "outcome": "stopped" }),
      TurnState::Failed {
        failure: Some(TurnFailure::Stopped),
        evidence: None,
      },
    ),
    (
      json!({ "delivery": "taken" }),
      json!({ "outcome": "failed", "reason": "it died", "engineError": { "kind": "rate_limit" } }),
      TurnState::Failed {
        failure: None,
        evidence: Some("it died\nengine error: rate_limit".into()),
      },
    ),
  ];

  for (delivery, ended, read) in cases {
    let stub = Stub::start().await;

    carrying(&stub, ended);
    stub.answer("POST /session/turn", 200, delivery);
    assert_eq!(carried(&stub).await.state, read);
  }
}

#[tokio::test]
async fn a_refused_send_fails_the_turn_with_the_refusal_as_its_evidence() {
  let stub = Stub::start().await;

  carrying(&stub, json!({ "outcome": "answered", "answer": "done" }));
  stub.answer(
    "POST /session/turn",
    409,
    json!({ "error": "worker@worker is in a turn", "code": "session-busy" }),
  );

  let TurnState::Failed {
    failure: None,
    evidence: Some(evidence),
  } = carried(&stub).await.state
  else {
    panic!("the turn failed with evidence");
  };

  assert!(evidence.contains("session-busy"), "{evidence}");
}

#[tokio::test]
async fn a_turn_whose_end_the_stream_never_tells_fails_saying_so() {
  let stub = Stub::start().await;

  carrying(&stub, json!({ "outcome": "answered", "answer": "done" }));
  stub.answer_events(
    "GET /events",
    "event: turn-started\ndata: {\"session\":\"other@worker\",\"run\":\"r\",\"at\":1}\n\n",
  );
  stub.answer("POST /session/turn", 200, json!({ "delivery": "taken" }));

  let TurnState::Failed {
    failure: None,
    evidence: Some(evidence),
  } = carried(&stub).await.state
  else {
    panic!("the turn failed with evidence");
  };

  assert!(evidence.contains("event stream ended"), "{evidence}");
}

#[tokio::test]
async fn words_are_pushed_into_the_turn_s_session() {
  let stub = Stub::start().await;

  carrying(&stub, json!({ "outcome": "answered", "answer": "done" }));
  stub.answer("POST /session/push", 200, json!({ "said": "delivered" }));

  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  assert_eq!(
    turns.push(&prepared.turn, "Cover the tests too.").await.unwrap(),
    AsideOutcome::Delivered
  );

  let pushed = stub.calls().into_iter().find(|call| call.route == "POST /session/push").unwrap();

  assert_eq!(pushed.body, Some(json!({ "session": SESSION, "words": "Cover the tests too." })));
}

#[tokio::test]
async fn a_question_s_answer_a_second_prompt_and_an_unknown_turn_are_refused() {
  let stub = Stub::start().await;

  carrying(&stub, json!({ "outcome": "answered", "answer": "done" }));
  stub.answer("POST /session/turn", 200, json!({ "delivery": "taken" }));

  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  turns.send(&prepared.turn, "Do the work.").await.unwrap();

  let again = turns.send(&prepared.turn, "Do the work.").await.unwrap_err();
  let answered = turns.answer(&prepared.turn, "q-1", &ConfirmAnswer::Allow).unwrap_err();
  let pushed = turns.push("t-9", "words").await.unwrap_err();

  assert!(
    matches!(
      again,
      AgencyError::Uncarried {
        call: "send_turn_prompt",
        ..
      }
    ),
    "{again}"
  );
  assert!(
    matches!(
      answered,
      AgencyError::Uncarried {
        call: "answer_question",
        ..
      }
    ),
    "{answered}"
  );
  assert!(
    matches!(
      pushed,
      AgencyError::Uncarried {
        call: "push_aside",
        ..
      }
    ),
    "{pushed}"
  );
  assert_eq!(turns.read("t-9"), None);
  ending(&turns, &prepared.turn).await;
}

#[tokio::test]
async fn carry_out_reset_reads_whether_one_waited() {
  let stub = Stub::start().await;

  stub.answer("POST /workers/{worker}/sessions/reset", 200, json!({ "carried": true }));
  assert!(stub.client().carry_out_reset("worker").await.unwrap());
  stub.took("POST /workers/{worker}/sessions/reset", "/workers/worker/sessions/reset", None);
}

#[tokio::test]
async fn record_session_posts_the_answered_session_and_reads_what_became_of_it() {
  let stub = Stub::start().await;
  let answered = SessionAnswered {
    mode: "claude-code".into(),
    session_id: Some("s-2".into()),
    account: None,
    spend: vec![],
    context_ratio: None,
    context_tokens: Some(1200),
    model: "model-a".into(),
  };

  stub.answer("POST /workers/{worker}/sessions/record", 200, json!({ "kept": "full" }));
  assert_eq!(
    stub.client().record_session("worker", &answered, "/srv/checkout", 41).await.unwrap(),
    SessionKept::Full
  );
  stub.took(
    "POST /workers/{worker}/sessions/record",
    "/workers/worker/sessions/record",
    Some(json!({
      "answered": { "mode": "claude-code", "session_id": "s-2", "account": null, "spend": [], "context_ratio": null, "context_tokens": 1200, "model": "model-a" },
      "cwd": "/srv/checkout",
      "job": 41,
    })),
  );
}

#[tokio::test]
async fn hand_over_session_posts_the_handover() {
  let stub = Stub::start().await;
  let handover = Handover {
    model: "model-a".into(),
    cwd: "/srv/checkout".into(),
    thread: Some("todo:7".into()),
    session: None,
    note: None,
  };

  stub.answer_empty("POST /workers/{worker}/sessions/handover", 204);
  stub.client().hand_over_session("worker", &handover).await.unwrap();
  stub.took(
    "POST /workers/{worker}/sessions/handover",
    "/workers/worker/sessions/handover",
    Some(json!({ "model": "model-a", "cwd": "/srv/checkout", "thread": "todo:7", "session": null, "note": null })),
  );
}

#[tokio::test]
async fn note_run_and_make_bench_post_to_the_worker() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /workers/{worker}/runs", 204);
  stub.client().note_run("worker").await.unwrap();
  stub.took("POST /workers/{worker}/runs", "/workers/worker/runs", None);

  let stub = Stub::start().await;

  stub.answer_empty("POST /workers/{worker}/bench", 204);
  stub.client().make_bench("worker").await.unwrap();
  stub.took("POST /workers/{worker}/bench", "/workers/worker/bench", None);
}

#[tokio::test]
async fn record_usage_posts_the_reply_and_the_run() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /workers/{worker}/usage", 204);
  stub.client().record_usage("worker", &reply(), Some("bmo-job-41")).await.unwrap();
  stub.took(
    "POST /workers/{worker}/usage",
    "/workers/worker/usage",
    Some(json!({ "reply": serde_json::to_value(reply()).unwrap(), "run": "bmo-job-41" })),
  );
}

#[tokio::test]
async fn sweep_servers_reads_what_was_swept() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /workers/{worker}/servers/sweep",
    200,
    json!({ "lines": ["stopped the dev server"] }),
  );
  assert_eq!(stub.client().sweep_servers("worker").await.unwrap(), ["stopped the dev server"]);
  stub.took("POST /workers/{worker}/servers/sweep", "/workers/worker/servers/sweep", None);
}

#[tokio::test]
async fn job_documents_reads_the_graph_the_trailer_and_the_handoff() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /workers/{worker}/job-documents",
    200,
    json!({ "graph": "Ask the graph.", "trailer": "Close the job.", "handoff": null }),
  );
  assert_eq!(
    stub.client().job_documents("worker").await.unwrap(),
    JobDocuments {
      graph: "Ask the graph.".into(),
      trailer: "Close the job.".into(),
      handoff: None,
    }
  );
  stub.took("GET /workers/{worker}/job-documents", "/workers/worker/job-documents", None);
}

#[tokio::test]
async fn prefix_touched_posts_the_files_and_reads_those_touching_it() {
  let stub = Stub::start().await;

  stub.answer("POST /workers/{worker}/prefix-touched", 200, json!({ "touched": ["CLAUDE.md"] }));
  assert_eq!(
    stub
      .client()
      .prefix_touched("worker", &["CLAUDE.md".into(), "src/lib.rs".into()])
      .await
      .unwrap(),
    ["CLAUDE.md"]
  );
  stub.took(
    "POST /workers/{worker}/prefix-touched",
    "/workers/worker/prefix-touched",
    Some(json!({ "files": ["CLAUDE.md", "src/lib.rs"] })),
  );
}

#[tokio::test]
async fn model_health_reads_whether_the_engine_answers() {
  let stub = Stub::start().await;

  stub.answer("GET /providers/{provider}/models/{model}/health", 200, json!({ "answers": true }));
  assert!(stub.client().model_health("gateway", "model-r").await.unwrap());
  stub.took(
    "GET /providers/{provider}/models/{model}/health",
    "/providers/gateway/models/model-r/health",
    None,
  );
}

#[tokio::test]
async fn a_login_is_rested_until_a_moment_and_by_a_job_s_usage() {
  let stub = Stub::start().await;

  stub.answer_empty("PUT /accounts/claude/{account}/rest", 204);
  stub.client().rest_login("a-1", 1_791_430_000_000).await.unwrap();
  stub.took(
    "PUT /accounts/claude/{account}/rest",
    "/accounts/claude/a-1/rest",
    Some(json!({ "until": 1_791_430_000_000i64 })),
  );

  let stub = Stub::start().await;

  stub.answer_empty("POST /accounts/claude/{account}/usage-rest", 204);
  stub.client().usage_rest("a-1", &json!({ "five_hour": 0.9 })).await.unwrap();
  stub.took(
    "POST /accounts/claude/{account}/usage-rest",
    "/accounts/claude/a-1/usage-rest",
    Some(json!({ "usage": { "five_hour": 0.9 } })),
  );
}

#[tokio::test]
async fn write_note_posts_the_note() {
  let stub = Stub::start().await;
  let note = LogNote {
    agent: "worker".into(),
    run: "bmo-work-7-plan-1".into(),
    chat: Some(100),
    job: Some(41),
    text: "claude-code · model-a".into(),
    source: Some("claude-code".into()),
    state: Some("running".into()),
  };

  stub.answer_empty("POST /log/notes", 204);
  stub.client().write_note(&note).await.unwrap();
  stub.took(
    "POST /log/notes",
    "/log/notes",
    Some(json!({ "agent": "worker", "run": "bmo-work-7-plan-1", "chat": 100, "job": 41, "text": "claude-code · model-a", "source": "claude-code", "state": "running" })),
  );
}

#[tokio::test]
async fn node_copy_reads_where_the_copy_stands() {
  let stub = Stub::start().await;

  stub.answer("GET /nodes/{name}/copy", 200, json!({ "path": "/srv/copies/node-a/checkout" }));
  assert_eq!(
    stub.client().node_copy("node-a", "/srv/checkout").await.unwrap(),
    "/srv/copies/node-a/checkout"
  );
  stub.took("GET /nodes/{name}/copy", "/nodes/node-a/copy?checkout=%2Fsrv%2Fcheckout", None);
}
