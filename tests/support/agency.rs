//! An in-process stand-in for bmo-agency. It serves the route of every call the service makes there
//! (docs/api.md, "bmo-agency") on a loopback port, records each call it takes, and answers each from the
//! fixture set for its route. A route with no fixture answers 500 with the code `no_fixture`.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};

use axum::body::Bytes;
use axum::extract::{MatchedPath, State};
use axum::http::{Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use job_agency::AgencyClient;
use serde_json::{Value, json};
use tokio::net::TcpListener;
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
}

/// What a route answers: a status, and a JSON body where it has one.
#[derive(Clone)]
struct Fixture {
  status: StatusCode,
  body: Option<Value>,
}

#[derive(Default)]
struct Book {
  calls: Vec<Call>,
  fixtures: HashMap<String, Fixture>,
}

type Shared = Arc<Mutex<Book>>;

/// The stand-in, serving until it is dropped.
pub struct Stub {
  book: Shared,
  address: Url,
  server: JoinHandle<()>,
}

/// Records a call and answers it from its route's fixture.
async fn take(
  State(book): State<Shared>,
  matched: MatchedPath,
  method: Method,
  uri: Uri,
  body: Bytes,
) -> Response {
  let route = format!("{method} {}", matched.as_str());
  let target = uri.path_and_query().map_or_else(|| uri.path().to_owned(), ToString::to_string);
  let body = (!body.is_empty()).then(|| serde_json::from_slice(&body).expect("a call's body is JSON"));
  let fixture = {
    let mut book = book.lock().expect("the book is not poisoned");

    book.calls.push(Call {
      route: route.clone(),
      target,
      body,
    });
    book.fixtures.get(&route).cloned()
  };

  match fixture {
    Some(Fixture {
      status,
      body: Some(body),
    }) => (status, Json(body)).into_response(),
    Some(Fixture { status, body: None }) => status.into_response(),
    None => {
      let said = json!({ "error": format!("No fixture is set for {route}."), "code": "no_fixture" });

      (StatusCode::INTERNAL_SERVER_ERROR, Json(said)).into_response()
    }
  }
}

/// Every route of the calls, each taken by `take`.
fn routes() -> Router<Shared> {
  Router::new()
    .route("/sessions", post(take).get(take))
    .route("/sessions/{id}", delete(take))
    .route("/sessions/{id}/turns", post(take))
    .route("/sessions/{id}/state", get(take))
    .route("/sessions/{id}/output", get(take))
    .route("/sessions/{id}/interrupt", post(take))
    .route("/sessions/{id}/fold", post(take))
    .route("/executions", get(take))
    .route("/executions/{id}", get(take))
    .route("/executions/{id}/cancel", post(take).delete(take))
    .route("/runs", post(take))
    .route("/runs/{id}", get(take))
    .route("/runs/{id}/close", post(take))
    .route("/log/starts", get(take))
    .route("/log/jobs/{job}", get(take))
    .route("/log/jobs/{job}/last", get(take))
    .route("/workers/{worker}/runtime", get(take))
    .route("/workers/{worker}/pin", put(take).delete(take))
    .route("/workers/{worker}/keepalive-pause", put(take).delete(take))
    .route("/workers/{worker}/context-tokens", put(take))
    .route("/providers", get(take))
    .route("/tiers", get(take))
    .route("/routes/resolve", post(take))
    .route("/pins/check", post(take))
    .route("/pool/requests", post(take).get(take))
    .route("/pool/requests/{token}", get(take))
    .route("/pool/requests/{token}/lease", post(take))
    .route("/pool/requests/{token}/bind", post(take))
    .route("/pool/requests/{token}/finish", post(take))
    .route("/pool/reservations", post(take).get(take))
    .route("/pool/reservations/{token}/release", post(take))
    .route("/pool/places/{place}/write", put(take))
    .route("/nodes", get(take))
    .route("/nodes/{name}/reach", get(take))
    .route("/nodes/heavy-run", post(take))
    .route("/accounts/claude/held", get(take))
    .route("/machine/room", get(take))
    .route("/machine/launch-held", put(take))
    .route("/places/workbench/{worker}", get(take))
    .route("/places/branch-slots/{pool}", get(take))
}

impl Stub {
  /// Starts the stand-in on a free loopback port.
  pub async fn start() -> Self {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port is free");
    let bound = listener.local_addr().expect("the port is bound");
    let address = Url::parse(&format!("http://{bound}/")).expect("the address reads");
    let book = Shared::default();
    let app = routes().with_state(book.clone());
    let server = tokio::spawn(async move {
      axum::serve(listener, app).await.expect("the stand-in serves");
    });

    Self {
      book,
      address,
      server,
    }
  }

  fn book(&self) -> MutexGuard<'_, Book> {
    self.book.lock().expect("the book is not poisoned")
  }

  /// The port's client, calling the stand-in.
  pub fn client(&self) -> AgencyClient {
    AgencyClient::new(self.address.clone()).expect("the stand-in's address is an http address")
  }

  /// Sets `route` to answer `status` with `body`.
  pub fn answer(&self, route: &str, status: u16, body: Value) {
    self.set(route, status, Some(body));
  }

  /// Sets `route` to answer `status` with no body.
  pub fn answer_empty(&self, route: &str, status: u16) {
    self.set(route, status, None);
  }

  fn set(&self, route: &str, status: u16, body: Option<Value>) {
    let status = StatusCode::from_u16(status).expect("a fixture's status is a status");

    self.book().fixtures.insert(route.to_owned(), Fixture { status, body });
  }

  /// Every call taken so far, in order.
  pub fn calls(&self) -> Vec<Call> {
    self.book().calls.clone()
  }

  /// Asserts the stand-in took exactly one call: on `route`, at `target`, with `body`.
  pub fn took(&self, route: &str, target: &str, body: Option<Value>) {
    let expected = Call {
      route: route.to_owned(),
      target: target.to_owned(),
      body,
    };

    assert_eq!(self.calls(), [expected]);
  }
}

impl Drop for Stub {
  fn drop(&mut self) {
    self.server.abort();
  }
}
