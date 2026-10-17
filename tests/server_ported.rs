//! The routes over the ported verbs, each through the real server (docs/api.md, "Routes"; docs/verbs.md,
//! "The server's verbs"): for every route, one request its verb carries out and one it refuses, the verb
//! reaching the rows of a scratch database and the stand-ins of the units it calls.

mod support;

use std::path::Path;

use bmo_job::verbs::Built;
use serde_json::{Value, json};
use support::server::{Served, code};

/// The owner as bmo-receiver names him, and his chat as the settings give it.
const OWNER_SENDER: i64 = 5;
const OWNER_CHAT: i64 = 100;

/// A server on the ported verbs whose stand-ins answer every call a verb makes with nothing in the way.
async fn served() -> Served<Built> {
  let served = Served::<Built>::ported().await;
  let bench = served.scratch.path().join("bench");

  for (route, body) in [
    ("GET /executions", json!([])),
    ("GET /providers", json!([])),
    ("GET /tiers", json!([])),
    ("GET /pool/roles", json!([])),
    ("GET /pool/requests", json!([])),
    ("GET /machine/room", json!({ "held": [], "jobs": 0, "browser_jobs": 0, "browsers": 0 })),
    ("DELETE /executions/{id}/cancel", json!({ "dropped": [] })),
    ("GET /places/workbench/{worker}", json!({ "root": bench, "scratches": [] })),
    ("GET /agents/{agent}/rest", json!({ "resting": false })),
    (
      "GET /nodes",
      json!([{ "name": "n1", "label": "Node One", "kind": "ssh", "role": null, "state": "ok", "checked_at": null }]),
    ),
  ] {
    served.agency.answer(route, 200, body);
  }

  for route in [
    "POST /executions/{id}/cancel",
    "POST /workers/{worker}/bench",
    "POST /workers/{worker}/words",
  ] {
    served.agency.answer_empty(route, 204);
  }

  served
    .receiver
    .answer("GET /owner", 200, json!({ "sender": OWNER_SENDER, "chat": OWNER_CHAT }));
  served.receiver.answer("POST /instructions", 201, json!({ "id": 1 }));
  served.receiver.answer(
    "GET /rooms/{chat}",
    200,
    json!({ "chat": OWNER_CHAT, "owner_room": true, "registered": true, "work_chat": OWNER_CHAT }),
  );
  served.testbed.answer("pending-list", "[]", "", 0);
  served
}

async fn item(served: &Served<Built>, id: i64, approved: bool) {
  served
    .seed(
      r#"INSERT INTO "todos" ("id", "title", "instruction", "requester", "chat_id", "approved", "priority", "status", "created_at")
         VALUES ($1, 'Fix the reader', 'What to do.', 'Owner', $2, $3, $1, 'waiting', 1)"#,
      vec![id.into(), OWNER_CHAT.into(), i64::from(approved).into()],
    )
    .await;
}

async fn worker(served: &Served<Built>, name: &str, cwd: &Path) {
  served
    .seed(
      r#"INSERT INTO "workers" ("name", "label", "cwd", "builtin", "support_count", "created_at") VALUES ($1, $1, $2, 0, 0, 1)"#,
      vec![name.into(), cwd.display().to_string().into()],
    )
    .await;
}

async fn question(served: &Served<Built>) {
  served
    .seed(
      r#"INSERT INTO "job_questions" ("todo_id", "chat_id", "question", "options", "recommended", "why", "asked_at", "due_at")
         VALUES (1, $1, 'Which one?', $2, 0, 'Both stand.', 1, 99999999999999)"#,
      vec![OWNER_CHAT.into(), "this\nthat".into()],
    )
    .await;
}

fn refused(answer: &(u16, Value)) -> (u16, &str) {
  (answer.0, code(&answer.1))
}

#[tokio::test]
async fn a_job_s_end_is_told_and_a_unit_that_fails_refuses_it() {
  let served = served().await;

  served
    .seed(
      r#"INSERT INTO "jobs" ("id", "chat_id", "unit", "launched_at", "finished_at", "prompt", "worker", "outcome", "ok")
         VALUES (7, $1, 'bmo-task-7', 1, 2, 'Do it.', 'worker', 'done', 1)"#,
      vec![OWNER_CHAT.into()],
    )
    .await;

  assert_eq!(
    served.post("/handoff", json!({ "outcome": "done", "job": 7 })).await,
    (200, json!({ "told": "told" }))
  );
  assert_eq!(served.post("/job-report", json!({ "job": 7 })).await, (204, Value::Null));

  served
    .receiver
    .answer("POST /instructions", 500, json!({ "error": "down", "code": "down" }));

  assert_eq!(
    refused(&served.post("/handoff", json!({ "outcome": "done", "job": 7 })).await),
    (502, "receiver")
  );
}

#[tokio::test]
async fn the_queue_s_standing_is_told_and_routes_that_cannot_be_read_refuse_it() {
  let served = served().await;
  let asked = json!({ "moment": "work_order", "repeat": true });

  assert_eq!(
    served.post("/queue/tell", asked.clone()).await,
    (200, json!({ "told": "nothing_waiting" }))
  );

  served.agency.answer("GET /providers", 500, json!({ "error": "down", "code": "down" }));

  assert_eq!(refused(&served.post("/queue/tell", asked).await), (502, "agency"));
}

#[tokio::test]
async fn a_runner_is_launched_and_one_nobody_knows_is_refused() {
  let served = served().await;

  assert_eq!(
    served.post("/launch", json!({ "agent": "search", "text": "find it" })).await,
    (204, Value::Null)
  );
  assert_eq!(
    refused(&served.post("/launch", json!({ "agent": "nobody", "text": "find it" })).await),
    (400, "runner_unknown")
  );
}

#[tokio::test]
async fn the_pool_board_shows_every_worker_and_a_pool_that_fails_refuses_it() {
  let served = served().await;
  let (status, body) = served.get("/coworker-pool").await;

  assert_eq!(status, 200, "{body}");
  assert_eq!(body["buckets"], json!([]));
  assert_eq!(body["workers"][0]["name"], "worker");
  assert_eq!(body["workers"][0]["tile"], json!({ "state": "idle", "work": null }));

  served
    .agency
    .answer("GET /pool/requests", 500, json!({ "error": "down", "code": "down" }));

  assert_eq!(refused(&served.get("/coworker-pool").await), (502, "agency"));
}

#[tokio::test]
async fn a_question_is_settled_once_and_one_that_does_not_exist_is_refused() {
  let served = served().await;

  question(&served).await;

  assert_eq!(
    served.post("/question", json!({ "id": 1, "chosen": 1 })).await,
    (200, json!({ "code": "question_settled" }))
  );
  assert_eq!(
    served.post("/question", json!({ "id": 1, "answer": "this one" })).await.1["code"],
    "question_settled_already"
  );
  assert_eq!(
    refused(&served.post("/question", json!({ "id": 9, "chosen": 0 })).await),
    (404, "question_absent")
  );
}

#[tokio::test]
async fn a_question_s_clock_is_stopped_and_one_that_does_not_exist_is_refused() {
  let served = served().await;

  question(&served).await;

  let (status, body) = served.post("/question-pause", json!({ "id": 1, "paused": true })).await;

  assert_eq!((status, body["code"].as_str()), (200, Some("question_paused")), "{body}");
  assert!(body["paused_left"].as_i64().is_some_and(|left| left > 0), "{body}");
  assert_eq!(
    refused(&served.post("/question-pause", json!({ "id": 9, "paused": true })).await),
    (404, "question_absent")
  );
}

#[tokio::test]
async fn a_unit_s_stop_is_confirmed_once_it_is_gone_and_one_that_ended_is_refused() {
  let served = served().await;

  served
    .seed(
      r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "worker") VALUES ($1, 'bmo-task-3', 1, 'Do it.', 'worker')"#,
      vec![OWNER_CHAT.into()],
    )
    .await;

  assert_eq!(
    served.post("/run/stop", json!({ "unit": "bmo-task-3", "why": "enough" })).await,
    (200, json!({ "code": "stop_confirmed", "target": "unit:bmo-task-3", "runs": 1 }))
  );
  assert_eq!(
    refused(&served.post("/run/stop", json!({ "unit": "bmo-task-9" })).await),
    (404, "stop_target_ended")
  );
  assert_eq!(
    served.post("/run/stop", json!({ "all": true })).await,
    (200, json!({ "code": "nothing_running" }))
  );

  // An execution still open once its cancel was asked is asked to stop, not confirmed gone.
  let open = json!({
    "id": "ex-1", "parent": null, "root": null, "line": null, "shared_pane": false, "started_at": 1,
    "ended_at": null, "outcome": null, "cancelled": false, "details": {}
  });

  served.agency.answer("GET /executions", 200, json!([open]));
  served.agency.answer("GET /executions/{id}", 200, open);

  assert_eq!(
    served.post("/run/stop", json!({ "all": true })).await,
    (
      200,
      json!({
        "code": "all_asked", "asked": 1, "confirmed": 0,
        "targets": [{ "target": "execution:ex-1", "state": { "kind": "stopping" } }]
      })
    )
  );
}

#[tokio::test]
async fn a_job_is_launched_in_a_free_checkout_and_a_worker_nobody_added_is_refused() {
  let served = served().await;
  let (status, body) = served
    .post(
      "/task",
      json!({ "chat": OWNER_CHAT, "prompt": "Look at the logs.", "purpose": "a look at the logs", "started_by": "owner", "cowork": false }),
    )
    .await;

  assert_eq!((status, body["code"].as_str()), (200, Some("job_launched")), "{body}");
  assert_eq!(body["worker"], "worker");
  assert!(body["unit"].as_str().is_some_and(|unit| unit.starts_with("bmo-task-")), "{body}");

  let asked = json!({ "chat": OWNER_CHAT, "prompt": "x", "purpose": "y", "started_by": "owner", "worker": "nobody", "cowork": false });

  assert_eq!(refused(&served.post("/task", asked).await), (404, "no_worker"));
}

#[tokio::test]
async fn an_item_is_approved_and_one_not_held_is_not_released() {
  let served = served().await;

  item(&served, 3, false).await;

  assert_eq!(
    served.post("/todo", json!({ "action": "approve", "id": 3, "by": "Owner" })).await,
    (200, json!({ "moved": true }))
  );
  assert_eq!(
    refused(&served.post("/todo", json!({ "action": "release", "id": 3 })).await),
    (409, "not_held")
  );
}

#[tokio::test]
async fn an_item_is_stopped_with_its_reason_and_an_order_read_before_the_queue_moved_is_refused() {
  let served = served().await;

  item(&served, 3, true).await;
  item(&served, 4, true).await;

  assert_eq!(
    served.post("/todo", json!({ "action": "stop", "id": 3, "why": "not now" })).await,
    (200, json!({ "moved": true, "stopped": 0 }))
  );
  assert_eq!(
    refused(&served.post("/todo", json!({ "action": "reorder", "id": 4, "order": [4, 9] })).await),
    (409, "reorder_stale")
  );
}

#[tokio::test]
async fn a_draft_s_text_is_saved_and_a_draft_is_not_resumed() {
  let served = served().await;

  item(&served, 5, true).await;

  let (status, body) = served
    .post(
      "/work-order",
      json!({ "id": 5, "action": "save", "intent": "Read it well.", "instruction": "Read the file." }),
    )
    .await;

  assert_eq!((status, body["status"].as_str()), (200, Some("draft")), "{body}");
  assert_eq!(body["instruction"], "Read the file.");
  assert_eq!(
    refused(&served.post("/work-order", json!({ "id": 5, "action": "resume" })).await),
    (409, "resume_not_held")
  );
}

#[tokio::test]
async fn a_worker_is_added_in_a_folder_of_its_own_and_the_built_in_one_is_not_removed() {
  let served = served().await;
  let (status, body) = served.post("/worker", json!({ "action": "add", "name": "bench" })).await;
  let cwd = served.scratch.path().join("homes").join("bench");

  assert_eq!(
    (status, body),
    (200, json!({ "name": "bench", "label": "bench", "cwd": cwd.display().to_string() }))
  );
  assert!(cwd.is_dir());
  assert_eq!(
    refused(&served.post("/worker", json!({ "action": "remove", "name": "worker" })).await),
    (409, "builtin_not_removed")
  );
}

#[tokio::test]
async fn a_checkout_is_added_on_a_node_and_a_node_nobody_lists_is_refused() {
  let served = served().await;
  let base = served.scratch.path().join("source");

  worker(&served, "bench", &base).await;

  let asked = json!({ "action": "add_checkout", "name": "bench", "node": "n1", "path": "~/bench" });

  assert_eq!(
    served.post("/worker/node", asked).await,
    (200, json!({ "worker": "bench-n1", "cwd": "~/bench" }))
  );

  let asked = json!({ "action": "add_checkout", "name": "bench", "node": "n2" });

  assert_eq!(refused(&served.post("/worker/node", asked).await), (404, "no_node"));
}

#[tokio::test]
async fn a_word_goes_to_the_worker_s_session_and_a_worker_nobody_added_is_refused() {
  let served = served().await;

  assert_eq!(
    served.post("/worker-word", json!({ "text": "Look at the logs." })).await,
    (204, Value::Null)
  );
  assert!(served.agency.calls().iter().any(|call| call.target == "/workers/worker/words"
    && call.body == Some(json!({ "text": "Look at the logs." }))));
  assert_eq!(
    refused(&served.post("/worker-word", json!({ "text": "Hello.", "worker": "nobody" })).await),
    (404, "no_worker")
  );
}
