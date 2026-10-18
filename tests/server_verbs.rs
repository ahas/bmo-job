//! The routes that hand a verb its request, each through the real server (docs/api.md, "Routes"): the
//! request read into the typed request the verb is handed, every field refused as the document gives it,
//! and the verb's answer and refusal answered as they stand.

mod support;

use bmo_job::queue::settings;
use bmo_job::server;
use job_base::Refusal;
use job_codes::{JobError, TodoRefusal};
use serde_json::{Value, json};
use support::server::{Served, code};

fn took(served: &Served) -> Vec<(&'static str, String)> {
  served.verbs().took()
}

#[tokio::test]
async fn a_job_that_ended_is_told_and_its_report_delivered() {
  let served = Served::start().await;

  assert_eq!(
    served.post("/handoff", json!({ "outcome": "done", "job": 7 })).await,
    (200, json!({ "told": "told" }))
  );
  assert_eq!(served.post("/job-report", json!({ "job": 7 })).await, (204, Value::Null));

  let (status, body) = served.post("/handoff", json!({ "outcome": " ", "job": 7 })).await;

  assert_eq!((status, code(&body)), (400, "field_wrong"));
  assert_eq!(served.post("/job-report", json!({ "job": "seven" })).await.0, 400);
  assert_eq!(
    took(&served),
    [
      ("job_ended", r#"JobEnded { outcome: "done", job: Some(7) }"#.to_owned()),
      ("deliver_report", "7".to_owned()),
    ]
  );
}

#[tokio::test]
async fn the_queue_is_told_at_a_verbs_moment() {
  let served = Served::start().await;

  assert_eq!(
    served
      .post("/queue/tell", json!({ "moment": "work_order", "repeat": true, "rides_ms": 5000 }))
      .await,
    (200, json!({ "told": "nothing_waiting" }))
  );
  assert_eq!(served.post("/queue/tell", json!({})).await.0, 400);
  assert_eq!(
    took(&served),
    [(
      "tell_queue",
      r#"Moment { moment: "work_order", repeat: true, rides_ms: Some(5000) }"#.to_owned()
    )]
  );
}

#[tokio::test]
async fn a_runner_is_launched_on_words_and_takes_no_file() {
  let served = Served::start().await;

  assert_eq!(
    served
      .post("/launch", json!({ "agent": "search", "text": "find it", "depth": "thorough" }))
      .await,
    (204, Value::Null)
  );

  let form = reqwest::multipart::Form::new()
    .text("agent", "search")
    .text("text", "find it")
    .part("file", reqwest::multipart::Part::bytes(b"x".to_vec()).file_name("a.txt"));
  let (status, body) = served.answered(served.http.post(served.url("/launch")).multipart(form)).await;

  assert_eq!((status, code(&body)), (400, "takes_no_files"));

  let (status, body) = served.post("/launch", json!({ "agent": "search", "text": "x", "depth": 3 })).await;

  assert_eq!((status, code(&body)), (400, "field_wrong"));
  assert_eq!(
    took(&served),
    [(
      "launch_runner",
      r#"RunnerLaunch { agent: "search", text: "find it", depth: Some("thorough") }"#.to_owned()
    )]
  );
}

#[tokio::test]
async fn the_pool_board_is_the_verbs_reading() {
  let served = Served::start().await;

  assert_eq!(
    served.get("/coworker-pool").await,
    (200, json!({ "read_at": 7, "buckets": [], "workers": [] }))
  );
}

#[tokio::test]
async fn a_question_is_answered_by_option_or_words_and_never_both() {
  let served = Served::start().await;

  assert_eq!(
    served.post("/question", json!({ "id": 3, "chosen": 1 })).await,
    (200, json!({ "code": "question_settled" }))
  );
  assert_eq!(served.post("/question", json!({ "id": 3, "answer": " my own " })).await.0, 200);

  for wrong in [
    json!({ "id": 3, "chosen": 1, "answer": "x" }),
    json!({ "id": 3 }),
    json!({ "id": 3, "chosen": -1 }),
  ] {
    let (status, body) = served.post("/question", wrong.clone()).await;

    assert_eq!((status, code(&body)), (400, "question_answer_wrong"), "{wrong}");
  }

  assert_eq!(
    served.post("/question-pause", json!({ "id": 3, "paused": true })).await,
    (200, json!({ "code": "question_paused", "paused_left": 60_000, "due_at": null }))
  );
  assert_eq!(served.post("/question-pause", json!({ "id": 3 })).await.0, 400);
  assert_eq!(
    took(&served),
    [
      ("answer_question", "3 Chosen(1)".to_owned()),
      ("answer_question", r#"3 Words("my own")"#.to_owned()),
      ("pause_question", "3 true".to_owned()),
    ]
  );
}

#[tokio::test]
async fn a_talk_on_a_question_names_it_by_number_and_carries_words_of_its_own() {
  let served = Served::start().await;

  assert_eq!(
    served.post("/question-talk", json!({ "id": 3, "say": " Why that one? " })).await,
    (200, json!({ "code": "question_talked", "paused_left": 60_000, "due_at": 90_000 }))
  );

  for (wrong, refused) in [
    (json!({ "say": "Why?" }), (400, "question_talk_number")),
    (json!({ "id": 3 }), (400, "question_talk_say")),
    (json!({ "id": 3, "say": "  " }), (400, "question_talk_say")),
    (json!({ "id": "three", "say": "Why?" }), (400, "field_wrong")),
  ] {
    let (status, body) = served.post("/question-talk", wrong.clone()).await;

    assert_eq!((status, code(&body)), refused, "{wrong}");
  }

  assert_eq!(took(&served), [("talk_question", r#"3 "Why that one?""#.to_owned())]);
}

#[tokio::test]
async fn the_machine_s_room_returning_forgets_the_held_launch_and_tells_the_queue_again() {
  let served = Served::start().await;
  let returned = "event: machine-room-returned\ndata: {\"at\":8000,\"held\":[\"memory\"]}\n\n";

  served
    .seed(
      r#"INSERT INTO "settings" ("key", "value") VALUES ('machine_launch_held', '{"at":8000,"held":["memory"]}')"#,
      vec![],
    )
    .await;
  served.agency.answer_events("GET /events", returned);

  server::room_returns(&served.server).await.unwrap();

  assert_eq!(
    took(&served),
    [(
      "tell_queue",
      r#"Moment { moment: "machine_room_returned", repeat: true, rides_ms: None }"#.to_owned()
    )]
  );
  assert_eq!(settings::get(&served.server.db, "machine_launch_held").await.unwrap(), None);
  assert_eq!(served.agency.calls()[0].target, "/events?kinds=machine-room-returned");
}

#[tokio::test]
async fn a_stop_names_a_target_or_a_unit_or_everything() {
  let served = Served::start().await;

  assert_eq!(
    served
      .post("/run/stop", json!({ "target": { "kind": "item", "id": "5" }, "why": "enough" }))
      .await,
    (200, json!({ "code": "stop_asked", "target": "item:5", "runs": 1 }))
  );
  assert_eq!(served.post("/run/stop", json!({ "unit": "job-3" })).await.1["target"], "unit:job-3");
  assert_eq!(served.post("/run/stop", json!({ "all": true })).await.1["code"], "all_asked");

  let (status, body) = served.post("/run/stop", json!({ "target": { "kind": "pane", "id": "1" } })).await;

  assert_eq!((status, code(&body)), (400, "stop_target_wrong"));

  let (status, body) = served.post("/run/stop", json!({})).await;

  assert_eq!((status, code(&body)), (400, "stop_target_missing"));
  assert_eq!(
    took(&served),
    [
      (
        "stop_run",
        r#"Stop { target: Target { kind: Item, id: "5" }, why: Some("enough") }"#.to_owned()
      ),
      (
        "stop_run",
        r#"Stop { target: Target { kind: Unit, id: "job-3" }, why: None }"#.to_owned()
      ),
      ("stop_all", String::new()),
    ]
  );
}

#[tokio::test]
async fn a_job_is_launched_on_a_room_and_something_to_do() {
  let served = Served::start().await;
  let (status, body) = served
    .post(
      "/task",
      json!({ "chat": -100, "prompt": "do it", "item": 12, "worker": "bench", "cowork": true }),
    )
    .await;

  assert_eq!(status, 200);
  assert_eq!(
    body,
    json!({
      "code": "job_launched", "job": 41, "unit": "job-41", "worker": "bench", "cwd": "/work", "chat": -100, "notes": []
    })
  );

  for wrong in [
    json!({ "prompt": "do it" }),
    json!({ "chat": 1, "prompt": " " }),
    json!({ "chat": 1, "prompt": "x", "item": "12" }),
  ] {
    let (status, body) = served.post("/task", wrong.clone()).await;

    assert_eq!((status, code(&body)), (400, "task_needs_room_and_work"), "{wrong}");
  }

  assert_eq!(took(&served), [(
    "launch_task",
    r#"TaskLaunch { chat: -100, prompt: "do it", model: None, item: Some(12), purpose: None, worker: Some("bench"), cowork: true, started_by: None }"#.to_owned()
  )]);
}

#[tokio::test]
async fn each_todo_action_is_read_with_what_it_takes() {
  let served = Served::start().await;
  let asks = [
    (
      json!({ "action": "approve", "id": 5, "note": "go", "by": "console" }),
      r#"5 Approve { note: Some("go"), by: Some("console") }"#,
    ),
    (json!({ "action": "move", "id": 5, "direction": "up" }), "5 Move { up: true }"),
    (json!({ "action": "reorder", "id": 5, "order": [5, 4] }), "5 Reorder { order: [5, 4] }"),
    (
      json!({ "action": "raise", "id": 5, "title": "T", "said": "S", "sources": [6] }),
      r#"5 Raise { title: "T", said: "S", sources: [6] }"#,
    ),
    (json!({ "action": "unappend", "id": 5, "note": 8 }), "5 Unappend { note: 8 }"),
    (json!({ "action": "worker", "id": 5 }), "5 Worker { worker: None }"),
    (json!({ "action": "cowork", "id": 5, "cowork": false }), "5 Cowork { on: false }"),
    (json!({ "action": "done", "id": 5 }), "5 Done"),
  ];

  for (asked, _) in &asks {
    // A raise answers the row it filed; every other change answers only that it moved.
    let wanted = if asked["action"] == "raise" {
      json!({ "moved": true, "raised": 99 })
    } else {
      json!({ "moved": true })
    };

    assert_eq!(served.post("/todo", asked.clone()).await, (200, wanted), "{asked}");
  }

  assert_eq!(
    served.post("/todo", json!({ "action": "stop", "id": 5, "why": "now" })).await,
    (200, json!({ "moved": true, "stopped": 2 }))
  );

  let refusals = [
    (json!({ "action": "move", "id": 5, "direction": "sideways" }), "field_wrong"),
    (json!({ "action": "resolve", "id": 5 }), "resolve_needs_why"),
    (json!({ "action": "append", "id": 5, "text": " " }), "append_empty"),
    (json!({ "action": "stop", "id": 5 }), "stop_needs_why"),
    (json!({ "action": "cowork", "id": 5 }), "cowork_needs_switch"),
    (json!({ "action": "sing", "id": 5 }), "action_wrong"),
    (json!({ "action": "top" }), "field_wrong"),
  ];

  for (asked, wanted) in refusals {
    let (status, body) = served.post("/todo", asked.clone()).await;

    assert_eq!((status, code(&body)), (400, wanted), "{asked}");
  }

  let handed: Vec<String> = took(&served).into_iter().map(|(_, handed)| handed).collect();
  let mut wanted: Vec<String> = asks.iter().map(|(_, handed)| (*handed).to_owned()).collect();

  wanted.push(r#"5 Stop { why: "now" }"#.to_owned());
  assert_eq!(handed, wanted);
}

#[tokio::test]
async fn a_verbs_refusal_is_answered_as_it_stands() {
  let served = Served::start().await;
  let refusal: Refusal = JobError::from(TodoRefusal::JobCannotUnhold).into();

  served.verbs().refuse_next(refusal);

  let (status, body) = served.post("/todo", json!({ "action": "release", "id": 5 })).await;

  assert_eq!((status, code(&body)), (403, "job_cannot_unhold"));
}

#[tokio::test]
async fn each_work_order_action_is_read_with_what_it_takes() {
  let served = Served::start().await;
  let asks = [
    (json!({ "id": 9, "action": "resume" }), "9 Resume"),
    (
      json!({ "id": 9, "action": "save", "intent": "why" }),
      r#"9 Save { intent: Some("why"), instruction: None, model: None }"#,
    ),
    (
      json!({ "id": 9, "action": "rerun", "task": "build-1", "model": "auto" }),
      r#"9 Rerun { task: "build-1", instruction: None, model: Some("auto") }"#,
    ),
    (
      json!({ "id": 9, "action": "skip", "task": "build-1", "why": "done elsewhere" }),
      r#"9 Skip { task: "build-1", why: "done elsewhere" }"#,
    ),
    (
      json!({ "id": 9, "action": "move", "task": "build-2", "after": "build-3" }),
      r#"9 Move { task: "build-2", anchor: "build-3", after: true }"#,
    ),
    (
      json!({ "id": 9, "action": "add", "title": "T", "instruction": "I" }),
      r#"9 Add { title: "T", instruction: "I", anchor: None }"#,
    ),
    (
      json!({ "id": 9, "action": "recover", "from": 3, "to": 4 }),
      "9 Recover { from: 3, to: 4 }",
    ),
    (
      json!({ "id": 9, "action": "start", "chat": -1, "cwd": "/w", "worker": "w" }),
      r#"9 Start { chat: -1, cwd: "/w", worker: "w", instruction: None, model: None }"#,
    ),
  ];

  for (asked, _) in &asks {
    assert_eq!(
      served.post("/work-order", asked.clone()).await,
      (200, json!({ "todoId": 9, "status": "active" })),
      "{asked}"
    );
  }

  let refusals = [
    (json!({ "id": "9", "action": "resume" }), "item_number_wrong"),
    (json!({ "id": 9, "action": "skip", "task": "build-1" }), "field_missing"),
    (json!({ "id": 9, "action": "edit", "task": 4 }), "field_not_text"),
    (
      json!({ "id": 9, "action": "move", "task": "a", "before": "b", "after": "c" }),
      "before_and_after",
    ),
    (json!({ "id": 9, "action": "move", "task": "a" }), "before_and_after"),
    (json!({ "id": 9, "action": "recover", "from": 4, "to": 3 }), "job_range_form"),
    (json!({ "id": 9, "action": "start", "cwd": "/w", "worker": "w" }), "chat_wrong"),
    (json!({ "id": 9, "action": "fly" }), "action_wrong"),
  ];

  for (asked, wanted) in refusals {
    let (status, body) = served.post("/work-order", asked.clone()).await;

    assert_eq!((status, code(&body)), (400, wanted), "{asked}");
  }

  let handed: Vec<String> = took(&served).into_iter().map(|(_, handed)| handed).collect();

  assert_eq!(handed, asks.iter().map(|(_, handed)| (*handed).to_owned()).collect::<Vec<_>>());
}

#[tokio::test]
async fn a_worker_is_added_changed_removed_and_placed() {
  let served = Served::start().await;

  assert_eq!(
    served
      .post(
        "/worker",
        json!({ "action": "add", "name": "bench", "path": "/w/bench", "preamble": " " })
      )
      .await,
    (200, json!({ "name": "bench", "label": "bench", "cwd": "/w/bench" }))
  );
  assert_eq!(
    served
      .post("/worker", json!({ "action": "update", "name": "bench", "dev_command": "", "support_count": 2, "pinned_provider": "auto" }))
      .await,
    (204, Value::Null)
  );
  assert_eq!(
    served.post("/worker", json!({ "action": "remove", "name": "bench" })).await,
    (204, Value::Null)
  );
  assert_eq!(
    served
      .post("/worker/node", json!({ "action": "add_checkout", "name": "bench", "node": "far" }))
      .await,
    (200, json!({ "worker": "bench-far", "cwd": "/nodes/far/bench" }))
  );
  assert_eq!(
    served
      .post("/worker/node", json!({ "action": "place", "name": "bench", "path": "/back" }))
      .await,
    (204, Value::Null)
  );

  let refusals = [
    ("/worker", json!({ "action": "add" }), "field_wrong"),
    ("/worker", json!({ "action": "rename", "name": "bench" }), "action_wrong"),
    (
      "/worker",
      json!({ "action": "update", "name": "bench", "support_count": 1.5 }),
      "field_wrong",
    ),
    ("/worker/node", json!({ "action": "remove_checkout", "name": "bench" }), "field_wrong"),
  ];

  for (path, asked, wanted) in refusals {
    let (status, body) = served.post(path, asked.clone()).await;

    assert_eq!((status, code(&body)), (400, wanted), "{asked}");
  }

  assert_eq!(took(&served), [
    ("add_worker", r#"WorkerAdd { name: "bench", label: None, path: Some("/w/bench"), preamble: None, dev_command: None }"#.to_owned()),
    (
      "update_worker",
      r#"bench WorkerUpdate { label: None, path: None, preamble: None, dev_command: Some(None), keepalive_stopped: None, pinned_provider: Some("auto"), support_count: Some(2), support_keepalive: None }"#.to_owned()
    ),
    ("remove_worker", "bench".to_owned()),
    ("change_node", r#"AddCheckout { name: "bench", node: "far", path: None }"#.to_owned()),
    ("change_node", r#"Place { name: "bench", node: None, path: Some("/back") }"#.to_owned()),
  ]);
}

#[tokio::test]
async fn a_word_to_a_worker_comes_as_json_or_as_a_form_with_its_files() {
  let served = Served::start().await;

  assert_eq!(
    served.post("/worker-word", json!({ "text": " look ", "worker": "bench" })).await,
    (204, Value::Null)
  );

  let form = reqwest::multipart::Form::new()
    .text("text", "")
    .part("file", reqwest::multipart::Part::bytes(b"abc".to_vec()).file_name("notes.txt"));

  assert_eq!(
    served.answered(served.http.post(served.url("/worker-word")).multipart(form)).await.0,
    204
  );

  let most = served.server.policy.queue.word_files;
  let mut many = reqwest::multipart::Form::new().text("text", "many");

  for at in 0..=most {
    many = many.part("file", reqwest::multipart::Part::bytes(vec![1]).file_name(format!("{at}.bin")));
  }

  let (status, body) = served.answered(served.http.post(served.url("/worker-word")).multipart(many)).await;

  assert_eq!((status, code(&body)), (400, "too_many_files"));

  let (status, body) = served.post("/worker-word", json!({ "text": "" })).await;

  assert_eq!((status, code(&body)), (400, "field_wrong"));
  assert_eq!(
    took(&served),
    [
      ("say_to_worker", r#"Some("bench") "look" []"#.to_owned()),
      ("say_to_worker", r#"None "" [("notes.txt", 3)]"#.to_owned()),
    ]
  );
}
