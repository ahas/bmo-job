//! The routes the server answers itself, each through the real server over a scratch database (docs/api.md,
//! "Routes"): health, the queue's holds, the work a worker session holds, the workers and branch agents, a
//! room's holdings, the readings, the talk about an item and the event stream, and what any route refuses.

mod support;

use futures_util::StreamExt;
use serde_json::{Value, json};
use support::server::{Served, code};

/// One item, as a reading needs it, with its title and its room.
async fn item(served: &Served, id: i64, title: &str, chat: i64) {
  served
    .seed(
      r#"INSERT INTO "todos" ("id", "title", "instruction", "requester", "chat_id", "approved", "priority", "status", "created_at")
         VALUES ($1, $2, 'What to do.', 'Owner', $3, 1, $1, 'waiting', 1000)"#,
      vec![id.into(), title.into(), chat.into()],
    )
    .await;
}

#[tokio::test]
async fn health_names_the_version_and_the_start() {
  let served = Served::start().await;
  let (status, body) = served.get("/health").await;

  assert_eq!(status, 200);
  assert_eq!(body["version"], env!("CARGO_PKG_VERSION"));
  assert_eq!(body["started_at"], served.server.started_at);
}

#[tokio::test]
async fn a_route_none_answers_and_a_body_that_is_not_json_are_refused() {
  let served = Served::start().await;
  let (status, body) = served.get("/nowhere").await;

  assert_eq!((status, code(&body)), (404, "route_absent"));

  let (status, body) = served.delete("/todo").await;

  assert_eq!((status, code(&body)), (404, "route_absent"));

  let (status, body) = served.answered(served.http.post(served.url("/todo")).body("not json")).await;

  assert_eq!((status, code(&body)), (400, "body_not_json"));
}

#[tokio::test]
async fn the_owners_pause_and_a_deployments_hold_are_read_taken_and_let_go() {
  let served = Served::start().await;

  assert_eq!(
    served.get("/queue/holds").await,
    (200, json!({ "paused": false, "deploy_hold": null, "deploy_waits_on": [] }))
  );
  assert_eq!(
    served.post("/queue-pause", json!({ "paused": true })).await,
    (200, json!({ "paused": true, "deploy_held": false, "told": null }))
  );
  assert_eq!(
    served
      .put(
        "/queue/deploy-hold",
        json!({ "hold": { "since": 5, "commits": ["abc"] }, "waits_on": [4, 6] })
      )
      .await,
    (204, Value::Null)
  );
  assert_eq!(
    served.get("/queue/holds").await,
    (
      200,
      json!({ "paused": true, "deploy_hold": { "since": 5, "commits": ["abc"] }, "deploy_waits_on": [4, 6] })
    )
  );
  assert_eq!(
    served.post("/deploy-hold-release", json!({})).await,
    (200, json!({ "released": true, "told": "nothing_waiting" }))
  );
  assert_eq!(
    served.delete("/queue/deploy-hold").await,
    (200, json!({ "released": false, "told": null }))
  );
  served.put("/queue/deploy-hold", json!({ "hold": {} })).await;
  assert_eq!(
    served.delete("/queue/deploy-hold").await,
    (200, json!({ "released": true, "told": "nothing_waiting" }))
  );
  assert_eq!(
    served.post("/queue-pause", json!({ "paused": false })).await,
    (200, json!({ "paused": false, "deploy_held": false, "told": "nothing_waiting" }))
  );

  let (status, body) = served.put("/queue/deploy-hold", json!({ "hold": "now" })).await;

  assert_eq!((status, code(&body)), (400, "field_wrong"));

  let resumed = r#"Moment { moment: "resumed", repeat: true, rides_ms: None }"#.to_owned();

  assert_eq!(
    served.verbs().took(),
    [
      ("tell_queue", resumed.clone()),
      ("tell_queue", resumed.clone()),
      ("tell_queue", resumed),
    ]
  );
}

#[tokio::test]
async fn a_sessions_work_is_recorded_read_and_forgotten_by_a_fold() {
  let served = Served::start().await;

  assert_eq!(served.get("/sessions/s-1/work").await, (200, json!({ "item": null, "results": [] })));
  assert_eq!(served.put("/sessions/s-1/item", json!({ "item": 12 })).await, (204, Value::Null));
  assert_eq!(served.put("/sessions/s-1/results/build-1", json!({ "version": "v1" })).await.0, 204);
  assert_eq!(served.put("/sessions/s-1/results/build-1", json!({ "version": "v2" })).await.0, 204);

  let (_, work) = served.get("/sessions/s-1/work").await;

  assert_eq!(work["item"], 12);
  assert_eq!(work["results"].as_array().map(Vec::len), Some(1));
  assert_eq!(work["results"][0]["version"], "v2");
  assert_eq!(served.delete("/sessions/s-1/results").await, (204, Value::Null));
  assert_eq!(served.get("/sessions/s-1/work").await.1["results"], json!([]));
  assert_eq!(served.put("/sessions/s-1/results/build-1", json!({})).await.0, 400);
}

#[tokio::test]
async fn workers_are_read_as_their_table_keeps_them() {
  let served = Served::start().await;

  served
    .seed(
      r#"INSERT INTO "workers" ("name", "label", "cwd", "builtin", "support_count", "created_at")
         VALUES ('worker', 'Worker', '/w', 1, 2, 10), ('bench', NULL, '/b', 0, 0, 20)"#,
      vec![],
    )
    .await;

  let (status, workers) = served.get("/workers").await;

  assert_eq!(status, 200);
  assert_eq!(
    workers
      .as_array()
      .map(|all| all.iter().map(|one| one["name"].clone()).collect::<Vec<_>>()),
    Some(vec![json!("bench"), json!("worker")])
  );

  let (_, worker) = served.get("/workers/worker").await;

  assert_eq!(
    (worker["builtin"].clone(), worker["support_count"].clone(), worker["created_at"].clone()),
    (json!(true), json!(2), json!(10))
  );

  let (status, body) = served.get("/workers/nobody").await;

  assert_eq!((status, code(&body)), (404, "no_worker"));
}

#[tokio::test]
async fn a_rooms_holdings_name_its_open_items_and_waiting_questions() {
  let served = Served::start().await;

  assert_eq!(served.get("/rooms/-100/holdings").await, (200, json!([])));
  item(&served, 1, "Open", -100).await;
  item(&served, 2, "Also open", -100).await;
  served
    .seed(
      r#"INSERT INTO "job_questions" ("todo_id", "chat_id", "question", "options", "recommended", "why", "asked_at", "due_at")
         VALUES (1, -100, 'Which?', '["a","b"]', 0, 'Yours to say.', 1, 2)"#,
      vec![],
    )
    .await;

  assert_eq!(
    served.get("/rooms/-100/holdings").await,
    (
      200,
      json!([{ "code": "room_open_items", "count": 2 }, { "code": "room_waiting_questions", "count": 1 }])
    )
  );
}

#[tokio::test]
async fn a_rooms_open_questions_are_the_unsettled_ones_still_waiting_the_one_asked_last_first() {
  let served = Served::start().await;
  let far = 99_999_999_999_999_i64;

  for (chat, options, due, settled, talks) in [
    (-100, "a\nb", far, None, 1),
    (-100, "c\nd\ne", far, None, 0),
    (-100, "f\ng", far, Some(5), 1),
    (-100, "h\ni", 2, None, 1),
    (-200, "j\nk", far, None, 1),
  ] {
    served
      .seed(
        r#"INSERT INTO "job_questions" ("todo_id", "chat_id", "question", "options", "recommended", "why", "asked_at", "due_at", "settled_at", "talks")
           VALUES (1, $1, 'Which?', $2, 0, 'Yours to say.', 1, $3, $4, $5)"#,
        vec![chat.into(), options.into(), due.into(), settled.into(), talks.into()],
      )
      .await;
  }

  assert_eq!(
    served.get("/questions/open?chat=-100").await,
    (
      200,
      json!([
        { "id": 2, "options": ["c", "d", "e"], "talks": false },
        { "id": 1, "options": ["a", "b"], "talks": true },
      ])
    )
  );
  assert_eq!(served.get("/questions/open?chat=-300").await, (200, json!([])));

  let (status, body) = served.get("/questions/open").await;

  assert_eq!((status, code(&body)), (400, "field_wrong"));
}

#[tokio::test]
async fn the_open_items_are_those_not_finished_merged_or_held_oldest_first_with_their_workers() {
  let served = Served::start().await;

  item(&served, 3, "Waiting", -100).await;
  item(&served, 1, "Given out", -100).await;
  item(&served, 2, "Finished", -100).await;
  item(&served, 4, "Merged", -100).await;
  item(&served, 5, "Held", -100).await;
  item(&served, 6, "Not approved", -100).await;

  for change in [
    r#"UPDATE "todos" SET "worker" = 'worker-a', "status" = 'running' WHERE "id" = 1"#,
    r#"UPDATE "todos" SET "done" = 1 WHERE "id" = 2"#,
    r#"UPDATE "todos" SET "merged_into" = 3 WHERE "id" = 4"#,
    r#"UPDATE "todos" SET "held_at" = 5 WHERE "id" = 5"#,
    r#"UPDATE "todos" SET "approved" = 0 WHERE "id" = 6"#,
  ] {
    served.seed(change, vec![]).await;
  }

  assert_eq!(
    served.get("/items?open=1").await,
    (
      200,
      json!({ "items": [
        { "id": 1, "approved": true, "status": "running", "title": "Given out", "worker": "worker-a" },
        { "id": 3, "approved": true, "status": "waiting", "title": "Waiting", "worker": null },
        { "id": 6, "approved": false, "status": "waiting", "title": "Not approved", "worker": null },
      ] })
    )
  );

  for asked in ["/items", "/items?open=0"] {
    let (status, body) = served.get(asked).await;

    assert_eq!((status, code(&body)), (400, "field_wrong"), "{asked}");
  }
}

#[tokio::test]
async fn the_readings_read_the_queue_an_item_a_rooms_jobs_and_a_work_order() {
  let served = Served::start().await;

  served.agency.answer(
    "GET /providers",
    200,
    json!([{ "name": "claude-code", "enabled": true, "registered": true }]),
  );
  served.gate.answer("POST /translations/kept/read", 200, json!({ "kept": [] }));
  item(&served, 3, "Make the thing", -100).await;
  item(&served, 4, "Another thing", -200).await;
  served
    .seed(
      r#"INSERT INTO "jobs" ("id", "chat_id", "unit", "launched_at", "prompt", "todo_id", "worker")
         VALUES (8, -100, 'job-8', 1000, 'Make it.', 3, 'worker')"#,
      vec![],
    )
    .await;

  let order = json!({
    "todoId": 3, "intent": "Why.", "instruction": "What.", "status": "active", "phase": "implementation",
    "stage": 0, "stages": [], "history": [], "control": null, "worker": "worker", "cwd": "/w", "chatId": -100,
    "model": "auto", "error": null, "updatedAt": 5,
    "tasks": [{ "id": "build-0", "state": "done", "role": "builder" }, { "id": "build-1", "state": "pending", "role": "builder" }],
  });

  served
    .seed(
      r#"INSERT INTO "work_orders" ("todo_id", "body") VALUES (3, $1)"#,
      vec![order.to_string().into()],
    )
    .await;

  let (status, listed) = served.get("/todos?chat=-100").await;

  assert_eq!(status, 200);
  assert_eq!(
    listed
      .as_array()
      .map(|items| items.iter().map(|one| one["id"].clone()).collect::<Vec<_>>()),
    Some(vec![json!(3)])
  );
  assert_eq!(served.get("/todos").await.1.as_array().map(Vec::len), Some(2));

  let (status, shown) = served.get("/todos/3").await;

  assert_eq!((status, shown["line"]["title"].clone()), (200, json!("Make the thing")));
  assert_eq!(served.get("/todos/Another%20thing").await.1["line"]["id"], 4);

  let (status, lines) = served.get("/todos/worker-lines?all").await;

  assert_eq!(status, 200);
  assert!(
    lines.as_array().is_some_and(|lines| lines
      .iter()
      .any(|line| line.as_str().is_some_and(|line| line.contains("Make the thing")))),
    "{lines}"
  );

  let (status, lines) = served.get("/todos/3/worker-lines").await;

  assert_eq!(status, 200);
  assert!(lines.as_array().is_some_and(|lines| !lines.is_empty()));

  let (status, jobs) = served.get("/rooms/-100/jobs?limit=5").await;

  assert_eq!((status, jobs.as_array().map(Vec::len)), (200, Some(1)));

  let (status, read) = served.get("/work-orders/3").await;

  assert_eq!((status, read["order"]["status"].clone()), (200, json!("active")));
  assert!(read["lines"].as_array().is_some_and(|lines| !lines.is_empty()));
  assert_eq!(
    served.get("/work-orders/3/current").await,
    (
      200,
      json!([{ "place": { "kind": "build", "at": 1 }, "task": { "id": "build-1", "state": "pending", "role": "builder" } }])
    )
  );
  assert_eq!(
    served.get("/work-orders/3/tasks/build-0").await.1["place"],
    json!({ "kind": "build", "at": 0 })
  );
  assert_eq!(served.get("/work-orders/4").await.1["order"]["status"], "draft");

  let missing = [
    ("/todos/77", "item_absent"),
    ("/work-orders/77", "item_absent"),
    ("/work-orders/3/tasks/build-9", "task_absent"),
  ];

  for (path, wanted) in missing {
    let (status, body) = served.get(path).await;

    assert_eq!((status, code(&body)), (404, wanted), "{path}");
  }

  served.agency.answer("GET /providers", 503, json!({ "error": "Down.", "code": "down" }));

  let (status, body) = served.get("/todos").await;

  assert_eq!((status, code(&body)), (502, "agency"));
}

#[tokio::test]
async fn a_workers_job_line_its_prompt_and_the_jobs_that_have_just_run_are_read() {
  let served = Served::start().await;

  item(&served, 3, "Make the thing", -100).await;
  served
    .seed(
      r#"INSERT INTO "jobs" ("id", "chat_id", "unit", "launched_at", "finished_at", "ok", "outcome", "model", "prompt",
                             "prompt_english", "prompt_korean", "todo_id", "purpose", "worker")
         VALUES (1, -100, 'job-1', 1000, 5000, 1, 'done', 'auto', 'Make it.', 'Make it.', 'The kept reading.', 3,
                 'Unused', 'builder'),
                (2, -200, 'job-2', 6000, NULL, NULL, NULL, NULL, 'Look around.', 'Look around.', NULL, NULL,
                 ' Look around ', 'builder'),
                (3, -100, 'job-3', 7000, 9000, 0, 'failed', NULL, 'Again.', NULL, NULL, 3, NULL, 'other')"#,
      vec![],
    )
    .await;
  served
    .seed(
      r#"INSERT INTO "job_spans" ("job_id", "todo_id", "kind", "name", "seconds", "times", "tokens", "input_tokens",
                                  "output_tokens", "cache_read_tokens", "cache_write_tokens")
         VALUES (1, 3, 'phase', 'run', 3.0, 1, NULL, NULL, NULL, NULL, NULL),
                (1, 3, 'tool', 'Bash', 2.0, 4, NULL, NULL, NULL, NULL, NULL),
                (1, 3, 'tool', 'Read', 0.5, 2, NULL, NULL, NULL, NULL, NULL),
                (1, 3, 'usage', 'model-small', 0.0, 1, 100, 10, 20, 60, 10),
                (1, 3, 'usage', 'model-large', 0.0, 1, 500, 50, 100, 300, 50)"#,
      vec![],
    )
    .await;

  let line = |id: i64, agent: &str, kind: &str| json!({ "id": id, "agent": agent, "run": "job-2", "at": id, "kind": kind, "text": "", "jobId": 2 });

  served.agency.answer(
    "GET /run-log",
    200,
    json!({ "lines": [line(1, "builder", "start"), line(2, "builder", "tool"), line(3, "other", "tool"), line(4, "builder", "tool")] }),
  );

  // The job the screen was opened at, where it is the worker's, with what its spans counted.
  assert_eq!(
    served.get("/workers/builder/job?run=job-1").await,
    (
      200,
      json!({ "job": {
        "run": "job-1", "item": 3, "title": "Make the thing", "purpose": "Unused", "launched_at": 1000,
        "finished_at": 5000, "tools": 6,
        "models": [
          { "model": "model-large", "input_tokens": 50, "output_tokens": 100, "cache_read_tokens": 300, "cache_write_tokens": 50 },
          { "model": "model-small", "input_tokens": 10, "output_tokens": 20, "cache_read_tokens": 60, "cache_write_tokens": 10 },
        ],
      } })
    )
  );

  // Otherwise the job it runs, its tool calls counted in its run's log and no model named.
  let running = json!({ "job": {
    "run": "job-2", "item": null, "title": null, "purpose": "Look around", "launched_at": 6000,
    "finished_at": null, "tools": 2, "models": [],
  } });

  assert_eq!(served.get("/workers/builder/job").await, (200, running.clone()));
  served.agency.took("GET /run-log", "/run-log?run=job-2&after=0", None);
  assert_eq!(served.get("/workers/builder/job?run=job-3").await, (200, running));

  // Otherwise its last job, and nothing for a worker with none.
  assert_eq!(served.get("/workers/other/job").await.1["job"]["run"], "job-3");
  assert_eq!(served.get("/workers/nobody/job").await, (200, json!({ "job": null })));

  assert_eq!(
    served.get("/workers/builder/job/prompt?run=job-1").await,
    (200, json!({ "prompt": { "text": "The kept reading.", "english": "Make it." } }))
  );
  assert_eq!(
    served.get("/workers/builder/job/prompt").await,
    (200, json!({ "prompt": { "text": "Look around.", "english": null } }))
  );
  assert_eq!(served.get("/workers/nobody/job/prompt").await, (200, json!({ "prompt": null })));

  assert_eq!(
    served.get("/jobs/recent?limit=2").await,
    (
      200,
      json!([
        { "id": 3, "unit": "job-3", "chat": -100, "model": null, "launched_at": 7000, "finished_at": 9000,
          "outcome": "failed", "ok": false, "label": { "kind": "item", "id": 3, "attempt": 2 }, "purpose": null,
          "worker": "other" },
        { "id": 2, "unit": "job-2", "chat": -200, "model": null, "launched_at": 6000, "finished_at": null,
          "outcome": null, "ok": null, "label": { "kind": "itemless" }, "purpose": "Look around",
          "worker": "builder" },
      ])
    )
  );
  assert_eq!(served.get("/jobs/recent").await.1.as_array().map(Vec::len), Some(3));

  served.agency.answer("GET /run-log", 503, json!({ "error": "Down.", "code": "down" }));

  let (status, body) = served.get("/workers/builder/job").await;

  assert_eq!((status, code(&body)), (502, "agency"));
}

// A field of a server-sent event's frame, by its name.
fn field<'a>(frame: &'a str, name: &str) -> Option<&'a str> {
  frame
    .lines()
    .find_map(|line| line.strip_prefix(name)?.strip_prefix(':').map(str::trim_start))
}

/// The data of the next `job` events on a stream, as many as asked.
async fn next_events<B: AsRef<[u8]>>(
  stream: &mut (impl futures_util::Stream<Item = reqwest::Result<B>> + Unpin),
  held: &mut String,
  wanted: usize,
) -> Vec<Value> {
  let mut events = vec![];

  loop {
    while let Some(end) = held.find("\n\n") {
      let frame: String = held.drain(..end + 2).collect();

      if field(&frame, "event") == Some("job")
        && let Some(data) = field(&frame, "data")
      {
        events.push(serde_json::from_str(data).expect("an event's data is JSON"));

        if events.len() == wanted {
          return events;
        }
      }
    }

    let chunk = stream.next().await.expect("the stream goes on").expect("the stream reads");

    held.push_str(std::str::from_utf8(chunk.as_ref()).expect("the stream is text"));
  }
}

#[tokio::test]
async fn the_event_stream_opens_with_its_run_and_carries_the_news_as_it_comes() {
  let served = Served::start().await;

  served.post("/queue-pause", json!({ "paused": true })).await;

  let response = served.http.get(served.url("/events")).send().await.expect("the stream opens");

  assert_eq!(response.headers()["content-type"], "text/event-stream");

  let mut stream = response.bytes_stream();
  let mut held = String::new();

  while !held.contains("\n\n") {
    let chunk = stream.next().await.expect("the stream goes on").expect("the stream reads");

    held.push_str(std::str::from_utf8(chunk.as_ref()).expect("the stream is text"));
  }

  let opening = held[..held.find("\n\n").unwrap()].to_owned();

  assert_eq!(field(&opening, "event"), Some("run"));
  assert_eq!(
    field(&opening, "data"),
    Some(json!({ "run": served.server.events.run() }).to_string().as_str())
  );

  let replayed = next_events(&mut stream, &mut held, 1).await;

  assert_eq!(
    (replayed[0]["kind"].clone(), replayed[0]["id"].clone()),
    (json!("queue_moved"), json!(1))
  );

  served.put("/queue/deploy-hold", json!({ "hold": {} })).await;

  let coming = next_events(&mut stream, &mut held, 1).await;

  assert_eq!(coming[0]["kind"], "queue_moved");
  assert_eq!(coming[0]["detail"], "deploy");
  assert_eq!(coming[0]["id"], 2);

  // A cursor of this run replays only what came after it.
  let run = served.server.events.run();
  let response = served.http.get(served.url(&format!("/events?after=1&run={run}"))).send().await.unwrap();
  let mut stream = response.bytes_stream();
  let mut held = String::new();
  let replayed = next_events(&mut stream, &mut held, 1).await;

  assert_eq!(replayed[0]["id"], 2);
}

#[tokio::test]
async fn a_turn_of_an_items_talk_is_kept_handed_on_and_its_answer_kept() {
  let served = Served::start().await;

  item(&served, 5, "Talked about", -100).await;
  served.receiver.answer("POST /talks", 200, json!({ "answer": "Here is what I think." }));

  let response = served.http.get(served.url("/events")).send().await.unwrap();
  let mut stream = response.bytes_stream();
  let mut held = String::new();

  assert_eq!(
    served.get("/todo-talk?id=5").await,
    (200, json!({ "answering": false, "failure": null }))
  );
  assert_eq!(
    served.post("/todo-talk", json!({ "id": 5, "text": "What now?" })).await,
    (202, Value::Null)
  );

  let moved = next_events(&mut stream, &mut held, 2).await;

  assert_eq!(
    moved.iter().map(|event| event["kind"].clone()).collect::<Vec<_>>(),
    [json!("talk_moved"), json!("talk_moved")]
  );
  assert_eq!(moved[1]["failure"], Value::Null);
  assert_eq!(served.get("/todo-talk?id=5").await.1, json!({ "answering": false, "failure": null }));

  let said = job_base::all(
    served.db(),
    r#"SELECT "speaker", "text" FROM "todo_talk" WHERE "todo_id" = 5 ORDER BY "id""#,
    vec![],
  )
  .await
  .unwrap()
  .iter()
  .map(|row| {
    (
      row.try_get::<String>("", "speaker").unwrap(),
      row.try_get::<String>("", "text").unwrap(),
    )
  })
  .collect::<Vec<_>>();

  assert_eq!(
    said,
    [
      ("owner".to_owned(), "What now?".to_owned()),
      ("agent".to_owned(), "Here is what I think.".to_owned())
    ]
  );
  assert_eq!(served.receiver.calls()[0].body.as_ref().unwrap()["topic"]["title"], "Talked about");

  // A turn that comes back with nothing is kept as why.
  served
    .receiver
    .answer("POST /talks", 429, json!({ "error": "Limit.", "code": "limit" }));
  served.post("/todo-talk", json!({ "id": 5, "text": "Again?" })).await;

  let moved = next_events(&mut stream, &mut held, 2).await;

  assert_eq!(moved[1]["failure"], "limit");
  assert_eq!(served.get("/todo-talk?id=5").await.1, json!({ "answering": false, "failure": "limit" }));

  let refusals = [
    (json!({ "id": 77, "text": "Hm?" }), 404, "talk_item_absent"),
    (json!({ "id": 5 }), 400, "field_wrong"),
  ];

  for (asked, status, wanted) in refusals {
    let (got, body) = served.post("/todo-talk", asked.clone()).await;

    assert_eq!((got, code(&body)), (status, wanted), "{asked}");
  }

  assert_eq!(served.get("/todo-talk").await.0, 400);
}
