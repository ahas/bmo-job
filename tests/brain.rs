//! The brain port's contract: every call is made on the method, path and body the API document gives it,
//! with the brain's token, and its answer is read as the call's shape (docs/api.md, "bmo-brain").

mod support;

use job_brain::{Brain, BrainClient, BrainError, Hold, Holds, Memory, MemoryAdd, Turn, Waited};
use serde_json::json;
use support::brain::{Stub, TOKEN};
use url::Url;

fn followup() -> MemoryAdd {
  MemoryAdd {
    kind: "followup".into(),
    action: "Ask for the account's password.".into(),
    todo: Some(12),
    chat: Some(-1001),
    slug: None,
  }
}

fn no_turn() -> Turn {
  Turn {
    agent: "agent".into(),
    chat_id: None,
    message_id: None,
  }
}

// Brain holds.

#[tokio::test]
async fn hold_posts_the_run_and_reads_the_locks_taken_after_it() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /brain/hold",
    200,
    json!({ "answered": true, "held": true, "taken": ["brain.lock"] }),
  );

  let hold = stub.client().hold("job-run-1").await.unwrap();

  assert_eq!(
    hold,
    Hold {
      held: true,
      taken: vec!["brain.lock".into()],
    }
  );
  stub.took("POST /brain/hold", "/brain/hold", Some(json!({ "run": "job-run-1" })));
}

#[tokio::test]
async fn holds_reads_the_runs_holding_the_brain_and_the_locks_taken() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /brain/hold",
    200,
    json!({ "answered": true, "holders": ["job-run-1", "job-run-2"], "taken": [] }),
  );

  let holds = stub.client().holds().await.unwrap();

  assert_eq!(
    holds,
    Holds {
      holders: vec!["job-run-1".into(), "job-run-2".into()],
      taken: vec![],
    }
  );
  stub.took("GET /brain/hold", "/brain/hold", None);
}

#[tokio::test]
async fn wait_posts_an_empty_body_and_reads_whether_the_locks_came_free() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /brain/wait",
    200,
    json!({ "answered": true, "free": false, "lines": ["still taken after nine minutes: brain.lock"] }),
  );

  let waited = stub.client().wait().await.unwrap();

  assert_eq!(
    waited,
    Waited {
      free: false,
      lines: vec!["still taken after nine minutes: brain.lock".into()],
    }
  );
  stub.took("POST /brain/wait", "/brain/wait", Some(json!({})));
}

// Memories.

#[tokio::test]
async fn memories_reads_the_rows_of_the_agent_in_the_room() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /memories",
    200,
    json!({ "answered": true, "memories": [
      {
        "id": 31, "agent": "agent", "slug": "memory-31", "type": "followup", "chatId": -1001,
        "reach": "owner", "action": "Ask for the account's password.", "todoId": 12, "active": true,
        "setAt": 1_700_000_000, "doneAt": null, "hits": 0,
      },
      {
        "id": 30, "agent": "agent", "slug": "standing", "type": "state", "chatId": -1001,
        "action": "The deploy waits on the owner.", "active": false, "setAt": 1_600_000_000,
      },
    ] }),
  );

  let memories = stub.client().memories("agent", -1001).await.unwrap();

  assert_eq!(
    memories,
    vec![
      Memory {
        id: 31,
        slug: "memory-31".into(),
        kind: "followup".into(),
        chat_id: Some(-1001),
        action: "Ask for the account's password.".into(),
        todo_id: Some(12),
        active: true,
        set_at: 1_700_000_000,
        done_at: None,
      },
      Memory {
        id: 30,
        slug: "standing".into(),
        kind: "state".into(),
        chat_id: Some(-1001),
        action: "The deploy waits on the owner.".into(),
        todo_id: None,
        active: false,
        set_at: 1_600_000_000,
        done_at: None,
      },
    ]
  );
  stub.took("GET /memories", "/memories?agent=agent&chat=-1001", None);
}

#[tokio::test]
async fn an_agent_key_is_encoded_in_the_query() {
  let stub = Stub::start().await;

  stub.answer("GET /memories", 200, json!({ "answered": true, "memories": [] }));

  assert!(stub.client().memories("a b&c=d", 7).await.unwrap().is_empty());
  assert_eq!(stub.calls()[0].target, "/memories?agent=a+b%26c%3Dd&chat=7");
}

#[tokio::test]
async fn add_memory_posts_the_turn_and_the_tools_arguments_and_reads_its_lines() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /memory/add",
    200,
    json!({ "answered": true, "lines": ["memory-31 filed in chat -1001"] }),
  );

  let lines = stub.client().add_memory(&no_turn(), &followup()).await.unwrap();

  assert_eq!(lines, ["memory-31 filed in chat -1001"]);
  stub.took(
    "POST /memory/add",
    "/memory/add",
    Some(json!({
      "turn": { "agent": "agent", "chatId": null, "messageId": null },
      "type": "followup",
      "action": "Ask for the account's password.",
      "todo": 12,
      "chat": -1001,
    })),
  );
}

#[tokio::test]
async fn a_turn_in_a_room_is_sent_with_its_room_and_message() {
  let stub = Stub::start().await;

  stub.answer("POST /memory/add", 200, json!({ "answered": true, "lines": [] }));

  let turn = Turn {
    agent: "agent".into(),
    chat_id: Some(-1001),
    message_id: Some(55),
  };
  let asked = MemoryAdd {
    slug: Some("ask-password".into()),
    todo: None,
    chat: None,
    ..followup()
  };

  stub.client().add_memory(&turn, &asked).await.unwrap();

  assert_eq!(
    stub.calls()[0].body,
    Some(json!({
      "turn": { "agent": "agent", "chatId": -1001, "messageId": 55 },
      "type": "followup",
      "action": "Ask for the account's password.",
      "slug": "ask-password",
    }))
  );
}

// The token.

#[tokio::test]
async fn every_call_carries_the_brains_token() {
  let stub = Stub::start().await;
  let client = stub.client();

  stub.answer("POST /brain/hold", 200, json!({ "answered": true, "held": true, "taken": [] }));
  stub.answer("GET /brain/hold", 200, json!({ "answered": true, "holders": [], "taken": [] }));
  stub.answer("POST /brain/wait", 200, json!({ "answered": true, "free": true, "lines": [] }));
  stub.answer("GET /memories", 200, json!({ "answered": true, "memories": [] }));
  stub.answer("POST /memory/add", 200, json!({ "answered": true, "lines": [] }));

  client.hold("job-run-1").await.unwrap();
  client.holds().await.unwrap();
  client.wait().await.unwrap();
  client.memories("agent", 7).await.unwrap();
  client.add_memory(&no_turn(), &followup()).await.unwrap();

  let calls = stub.calls();

  assert_eq!(calls.len(), 5);
  assert!(
    calls
      .iter()
      .all(|call| call.authorization.as_deref() == Some(&format!("Bearer {TOKEN}"))),
    "{calls:?}"
  );
}

#[test]
fn the_client_shows_its_address_and_never_its_token() {
  let client = BrainClient::new(Url::parse("http://brain.invalid/").unwrap(), "secret-token").unwrap();
  let shown = format!("{client:?}");

  assert!(shown.contains("brain.invalid"), "{shown}");
  assert!(!shown.contains("secret-token"), "{shown}");
}

// Failures.

#[tokio::test]
async fn a_refusal_answered_at_200_is_handed_on_with_its_code_and_sentence() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /memory/add",
    200,
    json!({
      "answered": false,
      "refusal": { "code": "action-missing", "message": "A memory needs an action.", "given": null },
    }),
  );

  let refused = stub.client().add_memory(&no_turn(), &followup()).await.unwrap_err();

  assert!(
    matches!(
      &refused,
      BrainError::Refused { call: "add_memory", status: 200, code, error }
        if code == "action-missing" && error == "A memory needs an action."
    ),
    "{refused}"
  );
}

#[tokio::test]
async fn a_status_that_is_no_success_is_handed_on_with_its_code_and_sentence() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /brain/hold",
    409,
    json!({ "error": "The brain's database is not open yet.", "code": "not-ready" }),
  );
  stub.answer("GET /memories", 401, json!({ "error": "The token is wrong.", "code": "unauthorized" }));

  let unheld = stub.client().hold("job-run-1").await.unwrap_err();
  let unread = stub.client().memories("agent", 7).await.unwrap_err();

  assert!(
    matches!(
      &unheld,
      BrainError::Refused { call: "hold", status: 409, code, error }
        if code == "not-ready" && error == "The brain's database is not open yet."
    ),
    "{unheld}"
  );
  assert!(
    matches!(
      &unread,
      BrainError::Refused { call: "memories", status: 401, code, .. } if code == "unauthorized"
    ),
    "{unread}"
  );
}

#[tokio::test]
async fn an_answer_that_does_not_read_is_malformed() {
  let stub = Stub::start().await;

  // No word on whether it was carried out, a refusal with nothing in it, a field of the wrong shape, and a
  // failing status with no error shape.
  stub.answer("POST /brain/hold", 200, json!({ "held": true, "taken": [] }));
  stub.answer("GET /brain/hold", 200, json!({ "answered": false }));
  stub.answer("POST /brain/wait", 200, json!({ "answered": true, "free": "yes", "lines": [] }));
  stub.answer("GET /memories", 502, json!("bad gateway"));

  let client = stub.client();
  let failures = [
    ("hold", 200, client.hold("job-run-1").await.unwrap_err()),
    ("holds", 200, client.holds().await.unwrap_err()),
    ("wait", 200, client.wait().await.unwrap_err()),
    ("memories", 502, client.memories("agent", 7).await.unwrap_err()),
  ];

  for (expected, expected_status, failed) in failures {
    assert!(
      matches!(
        failed,
        BrainError::Malformed { call, status, .. } if call == expected && status == expected_status
      ),
      "{expected}: {failed}"
    );
  }
}

#[tokio::test]
async fn a_call_that_reaches_nothing_is_unreachable() {
  let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let address = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();

  drop(listener);

  let failed = BrainClient::new(address, TOKEN).unwrap().hold("job-run-1").await.unwrap_err();

  assert!(matches!(failed, BrainError::Unreachable { call: "hold", .. }), "{failed}");
}

#[test]
fn an_address_that_is_not_http_is_refused() {
  for address in ["https://brain.invalid/", "mailto:brain@brain.invalid", "file:///brain"] {
    let refused = BrainClient::new(Url::parse(address).unwrap(), TOKEN).unwrap_err();

    assert!(matches!(refused, BrainError::NotAnAddress { .. }), "{address}: {refused}");
  }
}
