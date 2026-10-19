//! A research run's turn carried on a session thrown away (docs/research-runs.md, "The run"): a question its
//! program stops on left standing for a person, and a turn stopped from outside keeping what bmo-agency read it had
//! written.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use bmo_job::verbs::research::Told;
use bmo_job::verbs::research::run::{Carried, Carry, carry_turn};
use job_agency::{Mapping, WorkTurns};
use serde_json::{Value, json};

use crate::support::desk::Desk;

/// The id bmo-agency knows the run's session thrown away by.
const SESSION: &str = "search@apart:1";

/// How often the turn and its session's state are read on the stand-in, which answers at once.
const READ_EVERY: Duration = Duration::from_millis(5);

// A desk whose work turns watch their session's state at the stand-in's pace, the session thrown away started.
async fn desk() -> Desk {
  let mut desk = Desk::new().await;
  let agency = desk.stand.ground.agency.clone();

  desk.stand.ground.turns = WorkTurns::new(agency).watching_every(READ_EVERY);
  desk.agency.answer(
    "POST /session/start",
    200,
    json!({
      "id": SESSION,
      "agent": "search",
      "scope": { "kind": "apart", "provider": "claude-code", "model": "small", "cwd": "/w", "todo": null, "role": null }
    }),
  );
  desk
}

fn state(question: Value) -> Value {
  json!({ "session": SESSION, "provider": "claude-code", "model": "small", "place": "/w", "generation": 1,
          "context": null, "run": "r-1", "question": question })
}

fn end(outcome: &str, failure: Value) -> Value {
  json!({
    "delivery": "taken",
    "run": "r-1",
    "outcome": outcome,
    "answer": "TITLE: done",
    "reply": { "mode": "claude-code", "model": "small", "text": "TITLE: done" },
    "failure": failure,
    "spans": [],
    "compacted": false
  })
}

// The research turn carried to its end, its run closing at `closes_at` where it names a moment.
async fn carried(desk: &Desk, closes_at: Option<i64>) -> Carried {
  let mapping = Mapping {
    provider: "claude-code".into(),
    tier: "Simple".into(),
    model: "small".into(),
    effort: None,
    answer_tokens: None,
  };
  let told = Told::default();
  let carry = Carry {
    agent: "search",
    role: None,
    mapping: &mapping,
    cwd: "/w",
    name: "bmo-search-1",
    title: "search run".into(),
    chat: 100,
    unit: None,
    told: &told,
    poll: READ_EVERY,
  };

  carry_turn(&desk.stand, &carry, "Find it.", &AtomicBool::new(false), closes_at)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_question_a_research_turn_stops_on_is_left_standing_for_a_person_once() {
  let desk = desk().await;
  let asked = json!({ "id": "q-1", "kind": "unknown", "screen": "Pick one" });

  desk.agency.answer("GET /session/state", 200, state(asked));
  desk.agency.answer("POST /session/answer", 200, json!({ "ok": true }));
  desk.agency.answer("POST /session/turn", 200, end("answered", Value::Null));

  let released = desk.agency.hold("POST /session/turn");
  let (carried, ()) = tokio::join!(carried(&desk, None), async {
    desk.agency.taken("POST /session/answer").await;
    released.notify_one();
  });
  let answered: Vec<Option<Value>> = desk
    .agency
    .calls()
    .into_iter()
    .filter(|call| call.route == "POST /session/answer")
    .map(|call| call.body)
    .collect();

  assert!(matches!(carried, Carried::Answered(_)), "{carried:?}");
  // The state goes on showing the question until the turn ends, and it is left standing only once.
  assert_eq!(
    answered,
    [Some(json!({ "session": SESSION, "question": "q-1", "answer": { "kind": "leave" } }))]
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_research_turn_stopped_from_outside_keeps_what_bmo_agency_read_it_had_written() {
  let desk = desk().await;
  let written = "TITLE: partial\n\nWhat it found so far.";

  desk.agency.answer("GET /session/state", 200, state(Value::Null));
  desk.agency.answer("POST /session/interrupt", 200, json!({ "ok": true }));
  desk.agency.answer(
    "GET /session/output",
    200,
    json!({ "run": "r-1", "running": false, "text": written }),
  );
  desk
    .agency
    .answer("POST /session/turn", 200, end("stopped", json!({ "kind": "stopped" })));

  let released = desk.agency.hold("POST /session/turn");
  // The run closed before its turn was first read, so the turn is interrupted at once.
  let (carried, ()) = tokio::join!(carried(&desk, Some(0)), async {
    desk.agency.taken("POST /session/interrupt").await;
    released.notify_one();
  });

  assert_eq!(carried, Carried::Stopped(written.to_owned()));
}
