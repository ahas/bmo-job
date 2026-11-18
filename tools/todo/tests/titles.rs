//! The titles the worker's readings show (docs/tools.md, 「The todo tool」): a Latin title as written, any other as
//! the gate keeps its translation, and one with none kept, or with no gate answering in time, as written and
//! marked.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use axum::extract::{Query, State};
use axum::http::HeaderMap;
use axum::routing::get;
use axum::{Json, Router};
use serde_json::{Value, json};
use todo_tool::titles::{GATE_WAIT, Kept, LANGUAGE, Titles, UNTRANSLATED, digest};
use tool_call::Service;

// A title in another script holding commas, which its digest names whole, and its English.
const GREEK: &str = "Ουρά, εργασίες, τίτλοι";
const ENGLISH: &str = "Queue, jobs, titles";
// A title in another script the gate keeps nothing for.
const UNKEPT: &str = "Κανένας τίτλος";
const LATIN: &str = "Repair the queue";

// What the stand-in gate was asked: each call's query.
type Asked = Arc<Mutex<Vec<BTreeMap<String, String>>>>;

// A gate keeping `kept`, each an original and its English, answering its kept translations by digest; or never
// answering at all.
async fn gate(kept: &[(&str, &str)], answers: bool) -> (Service, Asked) {
  let kept: BTreeMap<String, String> = kept
    .iter()
    .map(|(original, english)| (digest(original), (*english).to_owned()))
    .collect();
  let asked: Asked = Arc::default();
  let route = get(
    move |State(asked): State<Asked>, headers: HeaderMap, Query(query): Query<BTreeMap<String, String>>| {
      let kept = kept.clone();

      async move {
        assert_eq!(headers["authorization"], "Bearer gate-token");
        asked.lock().unwrap().push(query.clone());

        if !answers {
          std::future::pending::<()>().await;
        }

        let items: Vec<Value> = query["digests"]
          .split(',')
          .filter_map(|digest| kept.get(digest).map(|text| json!({ "digest": digest, "text": text })))
          .collect();

        Json(json!({ "items": items, "more": false }))
      }
    },
  );
  let app = Router::new().route("/translations/kept", route).with_state(asked.clone());
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
  let url = format!("http://{}", listener.local_addr().unwrap());

  tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

  (
    Service {
      url,
      token: "gate-token".to_owned(),
    },
    asked,
  )
}

fn marked(title: &str) -> String {
  format!("{title} {UNTRANSLATED}")
}

#[tokio::test]
async fn a_title_in_another_script_shows_its_kept_translation_and_one_with_none_kept_is_marked() {
  let (service, asked) = gate(&[(GREEK, ENGLISH)], true).await;
  let kept = Kept::read(Some(&service), &[GREEK, UNKEPT, LATIN]).await;

  assert_eq!(kept.shown(GREEK), ENGLISH);
  assert_eq!(kept.shown(UNKEPT), marked(UNKEPT));
  assert_eq!(kept.shown(LATIN), LATIN, "a Latin title is never marked");

  let asked = asked.lock().unwrap();
  let mut digests: Vec<String> = asked[0]["digests"].split(',').map(str::to_owned).collect();
  let mut foreign = vec![digest(GREEK), digest(UNKEPT)];

  digests.sort();
  foreign.sort();
  assert_eq!(asked.len(), 1, "one call carries every title");
  assert_eq!(asked[0]["language"], LANGUAGE);
  assert_eq!(digests, foreign, "only the titles in another script are asked");
}

#[tokio::test]
async fn latin_titles_alone_ask_nothing_of_the_gate() {
  let (service, asked) = gate(&[], true).await;
  let kept = Kept::read(Some(&service), &[LATIN]).await;

  assert_eq!(kept.shown(LATIN), LATIN);
  assert!(asked.lock().unwrap().is_empty());
}

#[tokio::test]
async fn with_no_gate_named_or_none_reached_a_title_in_another_script_is_marked() {
  let unreached = Service {
    url: "http://127.0.0.1:9".to_owned(),
    token: "gate-token".to_owned(),
  };

  assert_eq!(Kept::read(None, &[GREEK]).await.shown(GREEK), marked(GREEK));
  assert_eq!(Kept::read(Some(&unreached), &[GREEK]).await.shown(GREEK), marked(GREEK));
}

// The bound is the tool's own; the paused clock moves past it the moment nothing else is left to run.
#[tokio::test(start_paused = true)]
async fn a_gate_that_does_not_answer_within_the_bound_leaves_the_title_marked() {
  let (service, _) = gate(&[(GREEK, ENGLISH)], false).await;
  let started = tokio::time::Instant::now();
  let kept = Kept::read(Some(&service), &[GREEK]).await;

  assert_eq!(kept.shown(GREEK), marked(GREEK));
  assert!(started.elapsed() >= GATE_WAIT, "it waited the bound out");
}
