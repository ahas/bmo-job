//! The agency port's contract for a job's work turns and the upkeep of a worker's sessions: every call is made
//! on the method, path and body the API document gives it, and its answer is read as the call's shape
//! (docs/api.md, "Work turns", "Workers' runtime", "Claude accounts", "Nodes").

mod support;

use std::collections::BTreeMap;

use job_agency::{
  AccountFailure, Agency, AsideOutcome, ConfirmAnswer, Confirmation, Handover, JobDocuments, LiveTask,
  LogNote, ModelSpend, Prepared, Question, Reply, Resume, RunEnding, RunWorkflow, SessionAnswered,
  SessionKept, TurnAsking, TurnFailure, TurnMapping, TurnSeat, TurnSpan, TurnState, WentOut, WorkTurn,
};
use serde_json::json;
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

#[tokio::test]
async fn prepare_turn_posts_the_asking_and_reads_the_session_it_continues() {
  let stub = Stub::start().await;

  stub.answer("POST /work-turns", 201, json!({ "turn": "t-1", "session": "s-1" }));

  let prepared = stub.client().prepare_turn(&asking()).await.unwrap();

  assert_eq!(
    prepared,
    Prepared {
      turn: "t-1".into(),
      session: Some("s-1".into())
    }
  );
  stub.took(
    "POST /work-turns",
    "/work-turns",
    Some(json!({
      "agent": "worker",
      "seat": { "kind": "branch", "todo": 7, "role": "scout" },
      "provider": "claude-code",
      "model": "model-a",
      "mapping": { "model": "model-a", "effort": "high", "tier": "Fine", "answer_tokens": null },
      "resume": { "kind": "session", "id": "s-1" },
      "cwd": "/srv/checkout",
      "name": "bmo-work-7-plan-1",
      "title": "#7 Fix the gate",
      "thread": "todo:7",
      "job": 41,
      "chat": 100,
      "unit": "bmo-work-7-plan-1",
      "user": null,
      "work_key": "job-worker",
      "hears_words": false,
      "variables": { "BMO_RESEARCH_CHAT": "100" },
    })),
  );
}

#[tokio::test]
async fn send_turn_prompt_posts_the_prompt() {
  let stub = Stub::start().await;

  stub.answer_empty("POST /work-turns/{turn}/prompt", 204);
  stub.client().send_turn_prompt("t-1", "Do the work.").await.unwrap();
  stub.took(
    "POST /work-turns/{turn}/prompt",
    "/work-turns/t-1/prompt",
    Some(json!({ "prompt": "Do the work." })),
  );
}

#[tokio::test]
async fn a_running_turn_reads_with_its_question_and_what_it_went_out_on() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /work-turns/{turn}",
    200,
    json!({
      "turn": "t-1",
      "state": "running",
      "question": { "id": "q-1", "kind": "permission", "text": "rm x", "amendable": false },
      "spans": [{ "kind": "tool", "name": "Bash", "seconds": 1.5, "times": 2, "tokens": null,
        "input_tokens": null, "output_tokens": null, "cache_read_tokens": null, "cache_write_tokens": null }],
      "compacted": false,
      "session": "s-1",
      "went_out": { "provider": "claude-code", "model": "model-b", "effort": null, "tier": "Fine" },
    }),
  );

  let turn = stub.client().work_turn("t-1").await.unwrap().unwrap();

  assert_eq!(
    turn,
    WorkTurn {
      turn: "t-1".into(),
      state: TurnState::Running {
        question: Some(Question {
          id: "q-1".into(),
          kind: Confirmation::Permission {
            text: "rm x".into(),
            amendable: false,
          },
        }),
      },
      spans: vec![TurnSpan {
        kind: "tool".into(),
        name: "Bash".into(),
        seconds: 1.5,
        times: 2,
        tokens: None,
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
      }],
      compacted: false,
      session: Some("s-1".into()),
      went_out: Some(WentOut {
        provider: "claude-code".into(),
        model: "model-b".into(),
        effort: None,
        tier: Some("Fine".into()),
      }),
    }
  );
  stub.took("GET /work-turns/{turn}", "/work-turns/t-1", None);
}

#[tokio::test]
async fn an_answered_turn_and_a_failed_one_read_by_their_state() {
  let stub = Stub::start().await;
  let client = stub.client();

  stub.answer(
    "GET /work-turns/{turn}",
    200,
    json!({ "turn": "t-1", "state": "answered", "reply": serde_json::to_value(reply()).unwrap(), "session": "s-2", "went_out": null }),
  );

  assert_eq!(
    client.work_turn("t-1").await.unwrap().unwrap().state,
    TurnState::Answered {
      reply: Box::new(reply())
    }
  );

  stub.answer(
    "GET /work-turns/{turn}",
    200,
    json!({ "turn": "t-1", "state": "failed", "failure": { "kind": "limit", "resets_at": 5, "account": "a-1" }, "session": null, "went_out": null }),
  );

  assert_eq!(
    client.work_turn("t-1").await.unwrap().unwrap().state,
    TurnState::Failed {
      failure: Some(TurnFailure::Limit {
        resets_at: Some(5),
        account: Some("a-1".into()),
        silenced: false,
      }),
      evidence: None,
    }
  );

  stub.answer(
    "GET /work-turns/{turn}",
    200,
    json!({ "turn": "t-1", "state": "failed", "failure": { "kind": "account", "failure": "node_token", "detail": "gone" }, "evidence": null, "session": null, "went_out": null }),
  );

  assert!(matches!(
    client.work_turn("t-1").await.unwrap().unwrap().state,
    TurnState::Failed {
      failure: Some(TurnFailure::Account {
        failure: AccountFailure::NodeToken,
        ..
      }),
      ..
    }
  ));

  stub.answer(
    "GET /work-turns/{turn}",
    200,
    json!({ "turn": "t-1", "state": "failed", "failure": { "kind": "died", "ending": { "kind": "exit", "code": 1 }, "said": "boom", "message": "claude exited" }, "session": null, "went_out": null }),
  );

  assert!(matches!(
    client.work_turn("t-1").await.unwrap().unwrap().state,
    TurnState::Failed {
      failure: Some(TurnFailure::Died {
        ending: RunEnding::Exit { code: 1 },
        ..
      }),
      ..
    }
  ));

  stub.answer(
    "GET /work-turns/{turn}",
    200,
    json!({ "turn": "t-1", "state": "reprepared", "session": "s-9", "went_out": null }),
  );

  let reprepared = client.work_turn("t-1").await.unwrap().unwrap();

  assert_eq!((reprepared.state, reprepared.session.as_deref()), (TurnState::Reprepared, Some("s-9")));
}

#[tokio::test]
async fn a_turn_bmo_agency_does_not_hold_reads_as_none() {
  let stub = Stub::start().await;

  stub.answer("GET /work-turns/{turn}", 404, json!({ "error": "no such turn", "code": "turn_absent" }));
  assert_eq!(stub.client().work_turn("t-9").await.unwrap(), None);
}

#[tokio::test]
async fn push_aside_posts_the_words_and_reads_what_came_of_them() {
  let stub = Stub::start().await;

  stub.answer("POST /work-turns/{turn}/asides", 200, json!({ "outcome": "waiting" }));
  assert_eq!(
    stub.client().push_aside("t-1", "Cover the tests too.").await.unwrap(),
    AsideOutcome::Waiting
  );
  stub.took(
    "POST /work-turns/{turn}/asides",
    "/work-turns/t-1/asides",
    Some(json!({ "text": "Cover the tests too." })),
  );
}

#[tokio::test]
async fn answer_question_puts_the_answer_by_its_kind() {
  let stub = Stub::start().await;

  stub.answer_empty("PUT /work-turns/{turn}/questions/{question}", 204);
  stub
    .client()
    .answer_question(
      "t-1",
      "q-1",
      &ConfirmAnswer::Refuse {
        words: Some("not now".into()),
      },
    )
    .await
    .unwrap();
  stub.took(
    "PUT /work-turns/{turn}/questions/{question}",
    "/work-turns/t-1/questions/q-1",
    Some(json!({ "kind": "refuse", "words": "not now" })),
  );
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
