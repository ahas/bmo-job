//! The stand-in for bmo-agency: the route of every call the service makes there (docs/api.md,
//! "bmo-agency"), served by the shared stand-in.

use std::ops::Deref;

use axum::Router;
use axum::routing::{delete, get, post, put};
use job_agency::AgencyClient;

use super::stub::{self, Shared, take};

/// The control token the port's client calls the stand-in with.
pub const TOKEN: &str = "agency-token";

/// The stand-in for bmo-agency, serving until it is dropped.
pub struct Stub(stub::Stub);

/// Every route of the calls, each taken by `take`.
fn routes() -> Router<Shared> {
  Router::new()
    .route("/session/start", post(take))
    .route("/session/turn", post(take))
    .route("/session/push", post(take))
    .route("/session/state", get(take))
    .route("/session/interrupt", post(take))
    .route("/session/fold", post(take))
    .route("/session/end", post(take))
    .route("/session/list", get(take))
    .route("/executions", get(take))
    .route("/executions/{id}", get(take))
    .route("/executions/{id}/cancel", delete(take))
    .route("/runs", post(take))
    .route("/runs/{id}", get(take))
    .route("/runs/{id}/close", post(take))
    .route("/runs/stop", post(take))
    .route("/run-log", get(take))
    .route("/log/jobs/{job}", get(take))
    .route("/log/jobs/{job}/last", get(take))
    .route("/turn-records", post(take))
    .route("/agents", get(take))
    .route("/worker", post(take))
    .route("/workers/{worker}/words", post(take))
    .route("/providers", get(take))
    .route("/tiers", get(take))
    .route("/routes/resolve", post(take))
    .route("/pins/check", post(take))
    .route("/pool/enqueue", post(take))
    .route("/pool/requests", get(take))
    .route("/pool/requests/{token}", get(take))
    .route("/pool/acquire", post(take))
    .route("/pool/bind", post(take))
    .route("/pool/finish", post(take))
    .route("/pool/reserve", post(take))
    .route("/pool/reservations", get(take))
    .route("/pool/reservations/release", post(take))
    .route("/pool/places/{place}/write", put(take))
    .route("/nodes", get(take))
    .route("/nodes/{name}/reach", get(take))
    .route("/accounts/claude/held", get(take))
    .route("/accounts/claude/homes", get(take))
    .route("/rest", get(take).post(take))
    .route("/machine/room", get(take))
    .route("/machine/launch-held", post(take))
    .route("/events", get(take))
    .route("/places/workbench/{worker}", get(take))
    .route("/places/branch-slots/{pool}", get(take))
    .route("/places/branch-slots/{pool}/{slot}", delete(take))
    .route("/places/prefix-digest", post(take))
    .route("/providers/{provider}/cache", get(take))
    .route("/pool/roles", get(take))
    .route("/coworker-pool", get(take))
    .route("/workers/{worker}/sessions/reset", post(take))
    .route("/workers/{worker}/sessions/record", post(take))
    .route("/workers/{worker}/sessions/handover", post(take))
    .route("/workers/{worker}/runs", post(take))
    .route("/workers/{worker}/usage", post(take))
    .route("/workers/{worker}/bench", post(take))
    .route("/workers/{worker}/servers/sweep", post(take))
    .route("/workers/{worker}/job-documents", get(take))
    .route("/workers/{worker}/prefix-touched", post(take))
    .route("/providers/{provider}/models/{model}/health", get(take))
    .route("/accounts/claude/{account}/usage-rest", post(take))
    .route("/log/notes", post(take))
    .route("/nodes/{name}/copy", get(take))
}

impl Stub {
  /// Starts the stand-in on a free loopback port.
  pub async fn start() -> Self {
    Self(stub::Stub::start(routes()).await)
  }

  /// The port's client, calling the stand-in.
  pub fn client(&self) -> AgencyClient {
    AgencyClient::new(self.address(), TOKEN).expect("the stand-in's address is an http address")
  }
}

impl Deref for Stub {
  type Target = stub::Stub;

  fn deref(&self) -> &stub::Stub {
    &self.0
  }
}
