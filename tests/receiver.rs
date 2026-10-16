//! The receiver port's contract: every call is made on the method, path and body the API document gives
//! it, and its answer is read as the call's shape (docs/api.md, "bmo-receiver").

mod support;

use job_codes::chat_health::RunDeath;
use job_codes::job_ended::JobEnding;
use job_codes::{Fact, FailureFact, JobFact};
use job_receiver::{
  Answerer, Answerers, DocumentTopic, Effect, Holder, Holding, Instruction, InstructionState, ItemTopic,
  Message, Notice, Owner, Receiver, ReceiverClient, ReceiverError, Room, Said, Talk, Topic, Turn,
};
use serde_json::{Value, json};
use support::receiver::Stub;
use url::Url;

fn job_ended() -> Fact {
  Fact::Job(JobFact::JobEnded {
    reason: JobEnding::Done,
    job_id: 7,
    checkout: "main".into(),
  })
}

fn job_ended_json() -> Value {
  json!({ "code": "job_ended", "reason": "done", "job_id": 7, "checkout": "main" })
}

fn instruction(said: Said) -> Instruction {
  Instruction {
    agent: None,
    chat: Some(-100),
    said,
    kind: Some("job:7".into()),
    sort: "job".into(),
    summary: None,
    expires_at: Some(9_000),
    events: vec!["job_ended".into()],
    rides_until: None,
    via: Some("road-1".into()),
    run: Some(12),
  }
}

fn item_talk() -> Talk {
  Talk {
    topic: Topic::Item(ItemTopic {
      id: 3,
      title: "An item".into(),
      done: false,
      merged_into: None,
      held: true,
      hold: Some("pause".into()),
      stopping: false,
      approved: true,
      status: "open".into(),
    }),
    turns: vec![
      Turn {
        owner: true,
        text: "Where is it?".into(),
      },
      Turn {
        owner: false,
        text: "Held.".into(),
      },
    ],
    said: "Why?".into(),
  }
}

// Instructions.

#[tokio::test]
async fn queue_instruction_posts_a_fact_with_its_delivery() {
  let stub = Stub::start().await;

  stub.answer("POST /instructions", 201, json!({ "id": 41 }));

  let id = stub.client().queue_instruction(&instruction(Said::Fact(job_ended()))).await.unwrap();

  assert_eq!(id, 41);
  stub.took(
    "POST /instructions",
    "/instructions",
    Some(json!({
      "agent": null,
      "chat": -100,
      "said": { "fact": job_ended_json() },
      "kind": "job:7",
      "sort": "job",
      "summary": null,
      "expires_at": 9_000,
      "events": ["job_ended"],
      "rides_until": null,
      "via": "road-1",
      "run": 12
    })),
  );
}

#[tokio::test]
async fn queue_instruction_posts_an_order_in_its_own_words() {
  let stub = Stub::start().await;
  let order = Instruction {
    chat: None,
    kind: Some("overtime:5".into()),
    sort: "overtime".into(),
    summary: Some("Run 5 runs over".into()),
    events: vec![],
    via: None,
    run: None,
    ..instruction(Said::Order("Decide whether run 5 goes on.".into()))
  };

  stub.answer("POST /instructions", 201, json!({ "id": 42 }));

  assert_eq!(stub.client().queue_instruction(&order).await.unwrap(), 42);
  stub.took(
    "POST /instructions",
    "/instructions",
    Some(json!({
      "agent": null,
      "chat": null,
      "said": { "order": "Decide whether run 5 goes on." },
      "kind": "overtime:5",
      "sort": "overtime",
      "summary": "Run 5 runs over",
      "expires_at": 9_000,
      "events": [],
      "rides_until": null,
      "via": null,
      "run": null
    })),
  );
}

#[tokio::test]
async fn a_failed_runs_last_action_travels_inside_its_fact() {
  let stub = Stub::start().await;
  let failed = Fact::Failure(FailureFact::RunFailed {
    room: None,
    cause: RunDeath::Exit { exit_code: 1 },
    said: None,
    last_tool: Some("Bash".into()),
    tool_detail: Some("cargo test".into()),
    run_log: "/runs/5.log".into(),
  });
  let order = Instruction {
    sort: "run_failure".into(),
    ..instruction(Said::Fact(failed))
  };

  stub.answer("POST /instructions", 201, json!({ "id": 43 }));
  stub.client().queue_instruction(&order).await.unwrap();

  let body = stub.calls()[0].body.clone().unwrap();

  assert_eq!(
    body["said"]["fact"],
    json!({
      "code": "run_failed",
      "room": null,
      "cause": { "kind": "exit", "exit_code": 1 },
      "said": null,
      "last_tool": "Bash",
      "tool_detail": "cargo test",
      "run_log": "/runs/5.log"
    })
  );
}

#[tokio::test]
async fn instruction_state_reads_how_far_it_has_gone_or_none() {
  let stub = Stub::start().await;

  for (state, read) in [
    ("pending", InstructionState::Pending),
    ("handed", InstructionState::Handed),
    ("delivered", InstructionState::Delivered),
  ] {
    stub.answer("GET /instructions/{id}", 200, json!({ "state": state }));

    assert_eq!(stub.client().instruction_state(41).await.unwrap(), Some(read));
  }

  assert!(stub.calls().iter().all(|call| call.target == "/instructions/41"));

  stub.answer(
    "GET /instructions/{id}",
    404,
    json!({ "error": "No such instruction.", "code": "unknown" }),
  );

  assert_eq!(stub.client().instruction_state(41).await.unwrap(), None);
}

#[tokio::test]
async fn withdraw_instruction_deletes_it_and_reads_whether_it_went() {
  let stub = Stub::start().await;

  stub.answer("DELETE /instructions/{id}", 200, json!({ "withdrawn": true }));

  assert!(stub.client().withdraw_instruction(41).await.unwrap());
  stub.took("DELETE /instructions/{id}", "/instructions/41", None);

  stub.answer("DELETE /instructions/{id}", 200, json!({ "withdrawn": false }));

  assert!(!stub.client().withdraw_instruction(41).await.unwrap());
}

// Voiced notices.

#[tokio::test]
async fn voice_notice_posts_the_fact_to_the_room() {
  let stub = Stub::start().await;
  let notice = Notice {
    fact: job_ended(),
    intent: "Tell the room its job ended.".into(),
    follow_up: None,
    optional: true,
    kind: Some("job:7".into()),
    expires_at: None,
    events: vec![],
    rides_until: Some(8_000),
  };

  stub.answer("POST /rooms/{chat}/notices", 201, json!({ "id": 44 }));

  assert_eq!(stub.client().voice_notice(-100, &notice).await.unwrap(), 44);
  stub.took(
    "POST /rooms/{chat}/notices",
    "/rooms/-100/notices",
    Some(json!({
      "fact": job_ended_json(),
      "intent": "Tell the room its job ended.",
      "follow_up": null,
      "optional": true,
      "kind": "job:7",
      "expires_at": null,
      "events": [],
      "rides_until": 8_000
    })),
  );
}

// Rooms.

#[tokio::test]
async fn owner_reads_the_owner_as_a_sender_and_their_chat() {
  let stub = Stub::start().await;

  stub.answer("GET /owner", 200, json!({ "sender": 501, "chat": 501 }));

  assert_eq!(
    stub.client().owner().await.unwrap(),
    Owner {
      sender: 501,
      chat: 501
    }
  );
  stub.took("GET /owner", "/owner", None);
}

#[tokio::test]
async fn room_reads_what_a_room_is_to_the_deployment() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /rooms/{chat}",
    200,
    json!({ "chat": -100, "owner_room": false, "registered": false, "work_chat": 501 }),
  );

  assert_eq!(
    stub.client().room(-100).await.unwrap(),
    Room {
      chat: -100,
      owner_room: false,
      registered: false,
      work_chat: 501,
    }
  );
  stub.took("GET /rooms/{chat}", "/rooms/-100", None);
}

#[tokio::test]
async fn answerers_reads_the_default_the_voice_and_each_answerer() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /rooms/{chat}/answerers",
    200,
    json!({
      "default": "agent-a",
      "voice": "agent-b",
      "answerers": [
        { "agent": "agent-b", "companion": false },
        { "agent": "agent-c", "companion": true }
      ]
    }),
  );

  assert_eq!(
    stub.client().answerers(-100).await.unwrap(),
    Answerers {
      default: "agent-a".into(),
      voice: "agent-b".into(),
      answerers: vec![
        Answerer {
          agent: "agent-b".into(),
          companion: false,
        },
        Answerer {
          agent: "agent-c".into(),
          companion: true,
        },
      ],
    }
  );
  stub.took("GET /rooms/{chat}/answerers", "/rooms/-100/answerers", None);
}

// Messages.

#[tokio::test]
async fn message_reads_its_sender_or_none() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /rooms/{chat}/messages/{message}",
    200,
    json!({ "sender": 501, "sender_name": "Owner", "sent_at": 6_000, "text": "File this." }),
  );

  assert_eq!(
    stub.client().message(-100, 77).await.unwrap(),
    Some(Message {
      sender: Some(501),
      sender_name: Some("Owner".into()),
      sent_at: 6_000,
      text: "File this.".into(),
    })
  );
  stub.took("GET /rooms/{chat}/messages/{message}", "/rooms/-100/messages/77", None);

  stub.answer(
    "GET /rooms/{chat}/messages/{message}",
    404,
    json!({ "error": "No such message.", "code": "unknown" }),
  );

  assert_eq!(stub.client().message(-100, 78).await.unwrap(), None);
}

// Talks.

#[tokio::test]
async fn talk_posts_an_item_topic_and_reads_the_answer() {
  let stub = Stub::start().await;

  stub.answer("POST /talks", 200, json!({ "answer": "It waits for you." }));

  assert_eq!(stub.client().talk(&item_talk()).await.unwrap(), Some("It waits for you.".into()));
  stub.took(
    "POST /talks",
    "/talks",
    Some(json!({
      "topic": {
        "kind": "item",
        "id": 3,
        "title": "An item",
        "done": false,
        "merged_into": null,
        "held": true,
        "hold": "pause",
        "stopping": false,
        "approved": true,
        "status": "open"
      },
      "turns": [
        { "owner": true, "text": "Where is it?" },
        { "owner": false, "text": "Held." }
      ],
      "said": "Why?"
    })),
  );
}

#[tokio::test]
async fn talk_posts_a_document_topic_and_reads_no_answer() {
  let stub = Stub::start().await;
  let talk = Talk {
    topic: Topic::Document(DocumentTopic {
      document: "doc-1".into(),
      label: "A document".into(),
      writer: "writer-1".into(),
      file: "docs/a.md".into(),
      version: "abc123".into(),
      heading: Some("A heading".into()),
      passage: None,
      translated: false,
    }),
    turns: vec![],
    said: "Is this right?".into(),
  };

  stub.answer("POST /talks", 200, json!({ "answer": null }));

  assert_eq!(stub.client().talk(&talk).await.unwrap(), None);
  stub.took(
    "POST /talks",
    "/talks",
    Some(json!({
      "topic": {
        "kind": "document",
        "document": "doc-1",
        "label": "A document",
        "writer": "writer-1",
        "file": "docs/a.md",
        "version": "abc123",
        "heading": "A heading",
        "passage": null,
        "translated": false
      },
      "turns": [],
      "said": "Is this right?"
    })),
  );
}

#[tokio::test]
async fn a_talk_that_comes_back_with_nothing_is_refused_with_its_code() {
  let stub = Stub::start().await;

  for (status, code) in [(409, "resting"), (429, "limit"), (503, "outage"), (500, "talk_failed")] {
    stub.answer(
      "POST /talks",
      status,
      json!({ "error": "The talk came back with nothing.", "code": code }),
    );

    let failed = stub.client().talk(&item_talk()).await.unwrap_err();

    assert!(
      matches!(
        &failed,
        ReceiverError::Refused { call: "talk", status: refused, code: read, .. }
          if *refused == status && read == code
      ),
      "{failed}"
    );
  }
}

// Room holdings.

#[tokio::test]
async fn declare_holder_puts_the_address_and_each_holding() {
  let stub = Stub::start().await;
  let holder = Holder {
    address: "http://127.0.0.1:9000/".into(),
    holdings: vec![
      Holding {
        name: "items".into(),
        effect: Effect::Kept,
      },
      Holding {
        name: "open_items".into(),
        effect: Effect::Warns,
      },
    ],
  };

  stub.answer_empty("PUT /room-holders/{unit}", 204);
  stub.client().declare_holder("job-unit", &holder).await.unwrap();
  stub.took(
    "PUT /room-holders/{unit}",
    "/room-holders/job-unit",
    Some(json!({
      "address": "http://127.0.0.1:9000/",
      "holdings": [
        { "name": "items", "effect": "kept" },
        { "name": "open_items", "effect": "warns" }
      ]
    })),
  );
}

// Failures.

#[tokio::test]
async fn a_refusal_is_handed_on_with_its_status_code_and_sentence() {
  let stub = Stub::start().await;

  stub.answer(
    "POST /rooms/{chat}/notices",
    409,
    json!({ "error": "The room has no voice.", "code": "no_voice" }),
  );
  stub.answer(
    "PUT /room-holders/{unit}",
    400,
    json!({ "error": "The address is not one.", "code": "address_wrong" }),
  );

  let notice = Notice {
    fact: job_ended(),
    intent: "Tell the room.".into(),
    follow_up: None,
    optional: false,
    kind: None,
    expires_at: None,
    events: vec![],
    rides_until: None,
  };
  let holder = Holder {
    address: "nowhere".into(),
    holdings: vec![],
  };
  let unvoiced = stub.client().voice_notice(-100, &notice).await.unwrap_err();
  let undeclared = stub.client().declare_holder("job-unit", &holder).await.unwrap_err();

  assert!(
    matches!(
      &unvoiced,
      ReceiverError::Refused { call: "voice_notice", status: 409, code, error }
        if code == "no_voice" && error == "The room has no voice."
    ),
    "{unvoiced}"
  );
  assert!(
    matches!(
      &undeclared,
      ReceiverError::Refused { call: "declare_holder", status: 400, code, .. } if code == "address_wrong"
    ),
    "{undeclared}"
  );
}

#[tokio::test]
async fn a_read_of_a_room_that_is_refused_is_not_nothing_found() {
  let stub = Stub::start().await;

  stub.answer("GET /rooms/{chat}", 404, json!({ "error": "No such room.", "code": "unknown" }));

  let failed = stub.client().room(-100).await.unwrap_err();

  assert!(
    matches!(&failed, ReceiverError::Refused { call: "room", status: 404, code, .. } if code == "unknown"),
    "{failed}"
  );
}

#[tokio::test]
async fn an_answer_that_does_not_read_is_malformed() {
  let stub = Stub::start().await;

  stub.answer("GET /owner", 200, json!({ "owner": 501 }));
  stub.answer("POST /instructions", 502, json!("bad gateway"));

  let unread = stub.client().owner().await.unwrap_err();
  let unsaid = stub
    .client()
    .queue_instruction(&instruction(Said::Fact(job_ended())))
    .await
    .unwrap_err();

  assert!(
    matches!(
      unread,
      ReceiverError::Malformed {
        call: "owner",
        status: 200,
        ..
      }
    ),
    "{unread}"
  );
  assert!(
    matches!(
      unsaid,
      ReceiverError::Malformed {
        call: "queue_instruction",
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

  let failed = ReceiverClient::new(address).unwrap().owner().await.unwrap_err();

  assert!(matches!(failed, ReceiverError::Unreachable { call: "owner", .. }), "{failed}");
}

#[test]
fn an_address_that_is_not_http_is_refused() {
  for address in ["https://receiver.invalid/", "mailto:receiver@receiver.invalid", "file:///receiver"] {
    let refused = ReceiverClient::new(Url::parse(address).unwrap()).unwrap_err();

    assert!(matches!(refused, ReceiverError::NotAnAddress { .. }), "{address}: {refused}");
  }
}

#[tokio::test]
async fn a_path_segment_is_percent_encoded() {
  let stub = Stub::start().await;

  stub.answer_empty("PUT /room-holders/{unit}", 204);

  let holder = Holder {
    address: "http://127.0.0.1:9000/".into(),
    holdings: vec![],
  };

  stub.client().declare_holder("a b/c?d", &holder).await.unwrap();

  assert_eq!(stub.calls()[0].target, "/room-holders/a%20b%2Fc%3Fd");
}
