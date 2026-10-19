//! The agency port's contract for a job's work turns and the upkeep of a worker's sessions: a work turn is
//! carried on the session contract and read as it ended, and every other call is made on the method, path and
//! body the API document gives it, its answer read as the call's shape (docs/api.md, "Work turns", "Workers'
//! runtime", "Claude accounts", "Nodes").

mod support;

use std::collections::BTreeMap;
use std::time::Duration;

use job_agency::{
  AccountFailure, Agency, AgencyClient, AgencyError, AsideOutcome, ConfirmAnswer, Confirmation, Handover,
  JobDocuments, LiveTask, LogNote, ModelSpend, Question, Reply, Resume, RunEnding, RunWorkflow, TurnAsking,
  TurnFailure, TurnMapping, TurnSeat, TurnState, WentOut, WorkTurn, WorkTurns,
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
      cost: None,
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

/// The harness session the worker's state says a turn on its model would continue.
const CONTINUES: &str = "h-1";

fn worker_asking() -> TurnAsking {
  TurnAsking {
    seat: TurnSeat::Worker,
    resume: Resume::Auto,
    ..asking()
  }
}

/// The stand-in answering a worker's session started, its state naming the session a turn continues, and the
/// turn's send answered with `end` laid over an end that took the words.
fn carrying(stub: &Stub, end: Value) {
  let mut answered =
    json!({ "delivery": "taken", "run": "turn:worker:2", "spans": [], "compacted": false });

  if let (Some(answered), Value::Object(end)) = (answered.as_object_mut(), end) {
    answered.extend(end);
  }

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
      "context": { "carried": 50_000, "window": 200_000 },
      "continues": CONTINUES
    }),
  );
  stub.answer("POST /session/turn", 200, answered);
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
async fn a_worker_s_turn_is_prepared_on_the_worker_s_session_and_names_the_session_it_continues() {
  let stub = Stub::start().await;

  carrying(&stub, json!({ "outcome": "answered", "answer": "done" }));

  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  assert_eq!(prepared.session.as_deref(), Some(CONTINUES));
  assert!(prepared.turn.starts_with(SESSION), "{}", prepared.turn);

  let calls = stub.calls();

  assert_eq!(calls[0].body, Some(json!({ "agent": "worker", "scope": { "kind": "worker" } })));
  assert_eq!(calls[1].route, "GET /session/state");
  assert!(calls[1].target.ends_with("&model=model-a"), "{}", calls[1].target);

  let named = TurnAsking {
    resume: Resume::Session { id: "s-1".into() },
    ..worker_asking()
  };
  let fresh = TurnAsking {
    resume: Resume::Fresh,
    ..worker_asking()
  };

  assert_eq!(turns.prepare(&named).await.unwrap().session.as_deref(), Some("s-1"));
  assert_eq!(turns.prepare(&fresh).await.unwrap().session, None);
  assert_eq!(stub.calls().len(), calls.len() + 2, "only the worker's session is started again");
}

#[tokio::test]
async fn a_branch_s_turn_goes_on_a_session_thrown_away_in_its_folder() {
  let stub = Stub::start().await;
  let scope = json!({
    "kind": "apart",
    "provider": "claude-code",
    "model": "model-a",
    "cwd": "/srv/checkout",
    "todo": 7,
    "role": "scout"
  });

  stub.answer(
    "POST /session/start",
    200,
    json!({ "id": "worker@apart:1", "agent": "worker", "scope": scope }),
  );

  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&asking()).await.unwrap();

  assert_eq!(prepared.session, None);
  stub.took(
    "POST /session/start",
    "/session/start",
    Some(json!({ "agent": "worker", "scope": scope })),
  );
}

#[tokio::test]
async fn an_answered_turn_reads_its_whole_end_and_is_then_let_go() {
  let stub = Stub::start().await;

  carrying(
    &stub,
    json!({
      "outcome": "answered",
      "answer": "done",
      "went_out": { "provider": "claude-code", "model": "model-a", "effort": "high", "tier": "Fine" },
      "reply": serde_json::to_value(Reply { session_id: Some("h-2".into()), ..reply() }).unwrap(),
      "spans": [{ "kind": "model", "name": "model-a", "seconds": 2.0, "times": 1, "tokens": 10,
                  "input_tokens": 4, "output_tokens": 3, "cache_read_tokens": 2, "cache_write_tokens": 1 }],
      "compacted": true
    }),
  );

  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  turns.send(&prepared.turn, "Do the work.").await.unwrap();

  let read = ending(&turns, &prepared.turn).await;
  let TurnState::Answered { reply: answered } = read.state else {
    panic!("the turn answered: {:?}", read.state);
  };

  assert_eq!(
    *answered,
    Reply {
      session_id: Some("h-2".into()),
      ..reply()
    }
  );
  assert_eq!(read.session.as_deref(), Some("h-2"));
  assert_eq!(
    read.went_out,
    Some(WentOut {
      provider: "claude-code".into(),
      model: "model-a".into(),
      effort: Some("high".into()),
      tier: Some("Fine".into()),
    })
  );
  assert_eq!((read.spans.len(), read.compacted), (1, true));
  assert_eq!(turns.read(&prepared.turn), None);

  let sent = stub.calls().into_iter().find(|call| call.route == "POST /session/turn").unwrap();

  assert_eq!(
    sent.body,
    Some(json!({
      "session": SESSION,
      "prompt": "Do the work.",
      "job": 41,
      "item": 7,
      "provider": "claude-code",
      "model": "model-a",
      "mapping": { "model": "model-a", "effort": "high", "tier": "Fine", "answer_tokens": null },
      "resume": { "kind": "session", "id": CONTINUES },
      "cwd": "/srv/checkout",
      "name": "bmo-work-7-plan-1",
      "title": "#7 Fix the gate",
      "thread": "todo:7",
      "chat": 100,
      "unit": "bmo-work-7-plan-1",
      "user": null,
      "work_key": "job-worker",
      "hears_words": false,
      "variables": { "BMO_RESEARCH_CHAT": "100" }
    }))
  );
}

#[tokio::test]
async fn a_turn_that_did_not_answer_reads_by_how_it_ended() {
  let person = TurnState::Failed {
    failure: Some(TurnFailure::Person),
    evidence: None,
  };
  let cases = [
    (json!({ "delivery": "blocked", "outcome": "blocked" }), person.clone()),
    (json!({ "outcome": "blocked", "failure": { "kind": "person" } }), person),
    (
      json!({ "outcome": "stopped" }),
      TurnState::Failed {
        failure: Some(TurnFailure::Stopped),
        evidence: None,
      },
    ),
    (
      json!({ "outcome": "failed", "reason": "it died", "engineError": { "kind": "rate_limit" } }),
      TurnState::Failed {
        failure: None,
        evidence: Some("it died\nengine error: rate_limit".into()),
      },
    ),
  ];

  for (end, read) in cases {
    let stub = Stub::start().await;

    carrying(&stub, end);
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
async fn a_question_s_answer_is_posted_and_a_second_prompt_and_an_unknown_turn_are_refused() {
  let stub = Stub::start().await;

  carrying(&stub, json!({ "outcome": "answered", "answer": "done" }));
  stub.answer("POST /session/answer", 200, json!({ "ok": true }));

  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  turns.send(&prepared.turn, "Do the work.").await.unwrap();

  let refusal = ConfirmAnswer::Refuse {
    words: Some("Not that one.".into()),
  };

  for answer in [ConfirmAnswer::Allow, ConfirmAnswer::Leave, refusal] {
    turns.answer(&prepared.turn, "q-1", &answer).await.unwrap();
  }

  let answered: Vec<Option<Value>> = stub
    .calls()
    .into_iter()
    .filter(|call| call.route == "POST /session/answer")
    .map(|call| call.body)
    .collect();
  let body = |answer: Value| Some(json!({ "session": SESSION, "question": "q-1", "answer": answer }));

  assert_eq!(
    answered,
    [
      body(json!({ "kind": "allow" })),
      body(json!({ "kind": "leave" })),
      body(json!({ "kind": "refuse", "words": "Not that one." })),
    ]
  );

  let again = turns.send(&prepared.turn, "Do the work.").await.unwrap_err();
  let unknown = turns.answer("t-9", "q-1", &ConfirmAnswer::Allow).await.unwrap_err();
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
      unknown,
      AgencyError::Uncarried {
        call: "answer_question",
        ..
      }
    ),
    "{unknown}"
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
async fn every_kind_of_failure_an_end_names_is_read_as_its_kind() {
  let cases = [
    (
      json!({ "kind": "limit", "resets_at": 9, "account": "a-1", "silenced": true }),
      TurnFailure::Limit {
        resets_at: Some(9),
        account: Some("a-1".into()),
        silenced: true,
      },
    ),
    (
      json!({ "kind": "account", "failure": "node_token", "detail": "no token" }),
      TurnFailure::Account {
        failure: AccountFailure::NodeToken,
        detail: "no token".into(),
      },
    ),
    (
      json!({ "kind": "died", "ending": { "kind": "exit", "code": 1 }, "said": "bye", "message": "it exited" }),
      TurnFailure::Died {
        ending: RunEnding::Exit { code: 1 },
        said: Some("bye".into()),
        message: "it exited".into(),
      },
    ),
    (json!({ "kind": "person" }), TurnFailure::Person),
    (
      json!({ "kind": "untrusted", "folder": "/srv/checkout", "config": "/srv/config" }),
      TurnFailure::Untrusted {
        folder: "/srv/checkout".into(),
        config: "/srv/config".into(),
      },
    ),
    (json!({ "kind": "stopped" }), TurnFailure::Stopped),
    (
      json!({ "kind": "transport", "message": "gone" }),
      TurnFailure::Transport {
        message: "gone".into(),
      },
    ),
    (
      json!({ "kind": "refused", "message": "no" }),
      TurnFailure::Refused {
        message: "no".into(),
      },
    ),
    (
      json!({ "kind": "unavailable", "scope": "worker", "mode": "claude-code", "reason": "switched off" }),
      TurnFailure::Unavailable {
        scope: "worker".into(),
        mode: "claude-code".into(),
        reason: "switched off".into(),
      },
    ),
    (
      json!({ "kind": "error", "name": "Thrown", "message": "it broke" }),
      TurnFailure::Error {
        name: "Thrown".into(),
        message: "it broke".into(),
      },
    ),
  ];

  for (failure, kind) in cases {
    let stub = Stub::start().await;

    carrying(&stub, json!({ "outcome": "failed", "reason": "it failed", "failure": failure }));
    assert_eq!(
      carried(&stub).await.state,
      TurnState::Failed {
        failure: Some(kind),
        evidence: None,
      }
    );
  }
}

#[tokio::test]
async fn a_reprepared_turn_names_the_session_it_would_continue_and_goes_again_on_it() {
  let stub = Stub::start().await;
  let end = |outcome: &str, continues: Value| {
    json!({ "delivery": "taken", "run": "r-1", "outcome": outcome, "answer": "done", "spans": [],
            "compacted": false, "continues": continues })
  };

  carrying(&stub, json!({}));
  stub.answer_each(
    "POST /session/turn",
    vec![
      (200, end("reprepared", json!("h-2"))),
      (200, end("reprepared", Value::Null)),
      (200, end("answered", Value::Null)),
    ],
  );

  let turns = WorkTurns::new(stub.client());
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  turns.send(&prepared.turn, "Do the work.").await.unwrap();

  let read = ending(&turns, &prepared.turn).await;

  assert_eq!((read.state, read.session.as_deref()), (TurnState::Reprepared, Some("h-2")));
  turns.send(&prepared.turn, "Do the work again.").await.unwrap();

  let read = ending(&turns, &prepared.turn).await;

  assert_eq!((read.state, read.session), (TurnState::Reprepared, None));
  turns.send(&prepared.turn, "Do the work afresh.").await.unwrap();
  assert!(matches!(ending(&turns, &prepared.turn).await.state, TurnState::Answered { .. }));

  let resumed: Vec<Value> = stub
    .calls()
    .into_iter()
    .filter(|call| call.route == "POST /session/turn")
    .filter_map(|call| call.body.map(|body| body["resume"].clone()))
    .collect();

  assert_eq!(
    resumed,
    [
      json!({ "kind": "session", "id": CONTINUES }),
      json!({ "kind": "session", "id": "h-2" }),
      json!({ "kind": "fresh" }),
    ]
  );
}

/// The turn read, a bounded while, until it stands as `stands` says.
async fn standing(
  turns: &WorkTurns<AgencyClient>,
  turn: &str,
  stands: impl Fn(&WorkTurn) -> bool,
) -> WorkTurn {
  for _ in 0..500 {
    let read = turns.read(turn).expect("the turn is carried");

    if stands(&read) {
      return read;
    }

    tokio::time::sleep(Duration::from_millis(5)).await;
  }

  panic!("the turn never stood as asked");
}

#[tokio::test]
async fn a_question_the_session_s_state_shows_is_read_while_the_turn_goes_and_gone_once_withdrawn() {
  let stub = Stub::start().await;
  let state = |question: Value| {
    json!({ "session": SESSION, "provider": "claude-code", "model": "model-a", "place": "place-1",
            "generation": 1, "context": null, "run": "r-1", "question": question, "continues": CONTINUES })
  };
  let asked = json!({ "id": "q-1", "kind": "permission", "text": "rm -rf build", "amendable": true });

  carrying(&stub, json!({ "outcome": "answered", "answer": "done" }));
  stub.answer_each(
    "GET /session/state",
    vec![
      (200, state(Value::Null)),
      (200, state(asked.clone())),
      (200, state(asked)),
      (200, state(Value::Null)),
    ],
  );

  let released = stub.hold("POST /session/turn");
  let turns = WorkTurns::new(stub.client()).watching_every(Duration::from_millis(5));
  let prepared = turns.prepare(&worker_asking()).await.unwrap();

  turns.send(&prepared.turn, "Do the work.").await.unwrap();

  let shown = standing(&turns, &prepared.turn, |read| {
    matches!(read.state, TurnState::Running { question: Some(_) })
  })
  .await;

  assert_eq!(
    shown.state,
    TurnState::Running {
      question: Some(Question {
        id: "q-1".into(),
        kind: Confirmation::Permission {
          text: "rm -rf build".into(),
          amendable: true,
        },
      }),
    }
  );
  standing(&turns, &prepared.turn, |read| read.state == TurnState::Running { question: None }).await;
  released.notify_one();
  assert!(matches!(ending(&turns, &prepared.turn).await.state, TurnState::Answered { .. }));
}

#[tokio::test]
async fn what_a_worker_s_turn_would_continue_is_read_off_its_state_for_the_model() {
  let stub = Stub::start().await;

  carrying(&stub, json!({}));

  let turns = WorkTurns::new(stub.client());

  assert_eq!(turns.continues("worker", "model-a").await.unwrap().as_deref(), Some(CONTINUES));

  let calls = stub.calls();

  assert_eq!(calls[0].body, Some(json!({ "agent": "worker", "scope": { "kind": "worker" } })));
  assert!(calls[1].target.ends_with("&model=model-a"), "{}", calls[1].target);
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

  stub.answer("POST /rest", 200, json!({ "silenced": false }));
  stub.client().rest_login("a-1", 1_791_430_000_000).await.unwrap();
  stub.took(
    "POST /rest",
    "/rest",
    Some(json!({ "until": 1_791_430_000, "kind": "limit", "account": "a-1" })),
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
