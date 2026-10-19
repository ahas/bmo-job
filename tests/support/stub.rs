//! An in-process stand-in for another unit. It serves the routes it is given on a loopback port, records
//! each call it takes, and answers each from the fixture set for its route: the next of a sequence set for it,
//! the last standing once reached, and only once the test lets it go where the route is held. A route with no
//! fixture answers 500 with the code `no_fixture`; a path it serves no route on is recorded under its method
//! and path and answered 404 with the code `not-found`, as a unit answers a route it does not serve.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard};

use axum::body::Bytes;
use axum::extract::{MatchedPath, State};
use axum::http::{HeaderMap, Method, StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router};
use serde_json::{Value, json};
use tokio::net::TcpListener;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use url::Url;

/// One call the stand-in took.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
  /// The route it matched, as its method and path pattern, such as `POST /sessions/{id}/turns`.
  pub route: String,
  /// The path and query it was made on, as they were sent.
  pub target: String,
  /// Its body read as JSON; none where it had no body.
  pub body: Option<Value>,
  /// Its `Authorization` header as it was sent; none where it had none.
  pub authorization: Option<String>,
}

/// What a route answers: a status, and a body where it has one.
#[derive(Clone)]
struct Fixture {
  status: StatusCode,
  body: Option<Said>,
}

/// A fixture's body: JSON, or a server-sent event stream written out whole, which ends where the text does.
#[derive(Clone)]
enum Said {
  Json(Value),
  Events(String),
}

#[derive(Default)]
pub struct Book {
  calls: Vec<Call>,
  fixtures: HashMap<String, VecDeque<Fixture>>,
  held: HashMap<String, Arc<Notify>>,
}

/// What every route of a stand-in shares.
pub type Shared = Arc<Mutex<Book>>;

/// The stand-in, serving until it is dropped.
pub struct Stub {
  book: Shared,
  address: Url,
  server: JoinHandle<()>,
}

/// Records a call and answers it from its route's fixture. Every route of a stand-in is taken by it.
pub async fn take(
  State(book): State<Shared>,
  matched: MatchedPath,
  method: Method,
  uri: Uri,
  headers: HeaderMap,
  body: Bytes,
) -> Response {
  let route = format!("{method} {}", matched.as_str());
  let target = uri.path_and_query().map_or_else(|| uri.path().to_owned(), ToString::to_string);
  let body = (!body.is_empty()).then(|| serde_json::from_slice(&body).expect("a call's body is JSON"));
  let authorization = headers
    .get(header::AUTHORIZATION)
    .map(|value| value.to_str().expect("an authorization header is text").to_owned());
  let (fixture, held) = {
    let mut book = book.lock().expect("the book is not poisoned");

    book.calls.push(Call {
      route: route.clone(),
      target,
      body,
      authorization,
    });

    let fixture = match book.fixtures.get_mut(&route) {
      Some(answers) if answers.len() > 1 => answers.pop_front(),
      Some(answers) => answers.front().cloned(),
      None => None,
    };

    (fixture, book.held.get(&route).cloned())
  };

  if let Some(held) = held {
    held.notified().await;
  }

  match fixture {
    Some(Fixture {
      status,
      body: Some(Said::Json(body)),
    }) => (status, Json(body)).into_response(),
    Some(Fixture {
      status,
      body: Some(Said::Events(text)),
    }) => (status, [(header::CONTENT_TYPE, "text/event-stream")], text).into_response(),
    Some(Fixture { status, body: None }) => status.into_response(),
    None => {
      let said = json!({ "error": format!("No fixture is set for {route}."), "code": "no_fixture" });

      (StatusCode::INTERNAL_SERVER_ERROR, Json(said)).into_response()
    }
  }
}

/// Records a call on a path the stand-in serves no route on, and refuses it as a unit refuses a route it does not
/// serve.
async fn unrouted(State(book): State<Shared>, method: Method, uri: Uri, body: Bytes) -> Response {
  let target = uri.path_and_query().map_or_else(|| uri.path().to_owned(), ToString::to_string);

  book.lock().expect("the book is not poisoned").calls.push(Call {
    route: format!("{method} {}", uri.path()),
    target,
    body: serde_json::from_slice(&body).ok(),
    authorization: None,
  });

  let said = json!({ "error": format!("No route serves {method} {}.", uri.path()), "code": "not-found" });

  (StatusCode::NOT_FOUND, Json(said)).into_response()
}

impl Stub {
  /// Starts the stand-in serving `routes` on a free loopback port.
  pub async fn start(routes: Router<Shared>) -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port is free");
    let bound = listener.local_addr().expect("the port is bound");
    let address = Url::parse(&format!("http://{bound}/")).expect("the address reads");
    let book = Shared::default();
    let app = routes.fallback(unrouted).with_state(book.clone());
    let server = tokio::spawn(async move {
      axum::serve(listener, app).await.expect("the stand-in serves");
    });

    Self {
      book,
      address,
      server,
    }
  }

  /// The address the stand-in serves at.
  pub fn address(&self) -> Url {
    self.address.clone()
  }

  fn book(&self) -> MutexGuard<'_, Book> {
    self.book.lock().expect("the book is not poisoned")
  }

  /// Sets `route` to answer `status` with `body`.
  pub fn answer(&self, route: &str, status: u16, body: Value) {
    self.set(route, status, Some(Said::Json(body)));
  }

  /// Sets `route` to answer 200 with a server-sent event stream of `text`, which ends where it does.
  pub fn answer_events(&self, route: &str, text: &str) {
    self.set(route, 200, Some(Said::Events(text.to_owned())));
  }

  /// Sets `route` to answer `status` with no body.
  pub fn answer_empty(&self, route: &str, status: u16) {
    self.set(route, status, None);
  }

  /// Sets `route` to answer each of `answers` in turn, a status with a body, the last for every call after.
  pub fn answer_each(&self, route: &str, answers: Vec<(u16, Value)>) {
    let answers = answers
      .into_iter()
      .map(|(status, body)| Fixture {
        status: StatusCode::from_u16(status).expect("a fixture's status is a status"),
        body: Some(Said::Json(body)),
      })
      .collect();

    self.book().fixtures.insert(route.to_owned(), answers);
  }

  /// Holds every call on `route` until the test lets it go: each `notify_one` on what this answers lets one go.
  pub fn hold(&self, route: &str) -> Arc<Notify> {
    let held = Arc::new(Notify::new());

    self.book().held.insert(route.to_owned(), Arc::clone(&held));
    held
  }

  /// Waits, a bounded while, until a call on `route` has been taken.
  pub async fn taken(&self, route: &str) {
    for _ in 0..1000 {
      if self.calls().iter().any(|call| call.route == route) {
        return;
      }

      tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }

    panic!("no call was taken on {route}: {:?}", self.calls());
  }

  fn set(&self, route: &str, status: u16, body: Option<Said>) {
    let status = StatusCode::from_u16(status).expect("a fixture's status is a status");

    self
      .book()
      .fixtures
      .insert(route.to_owned(), VecDeque::from([Fixture { status, body }]));
  }

  /// Every call taken so far, in order.
  pub fn calls(&self) -> Vec<Call> {
    self.book().calls.clone()
  }

  /// Asserts the stand-in took exactly one call: on `route`, at `target`, with `body`.
  pub fn took(&self, route: &str, target: &str, body: Option<Value>) {
    let took: Vec<(String, String, Option<Value>)> =
      self.calls().into_iter().map(|call| (call.route, call.target, call.body)).collect();

    assert_eq!(took, [(route.to_owned(), target.to_owned(), body)]);
  }
}

impl Drop for Stub {
  fn drop(&mut self) {
    self.server.abort();
  }
}
