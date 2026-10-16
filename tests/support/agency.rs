//! The stand-in for bmo-agency: the route of every call the service makes there (docs/api.md,
//! "bmo-agency"), served by the shared stand-in.

use std::ops::Deref;

use axum::Router;
use axum::routing::{delete, get, post, put};
use job_agency::AgencyClient;

use super::stub::{self, Shared, take};

/// The stand-in for bmo-agency, serving until it is dropped.
pub struct Stub(stub::Stub);

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
    .route("/turn-records", post(take))
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
    Self(stub::Stub::start(routes()).await)
  }

  /// The port's client, calling the stand-in.
  pub fn client(&self) -> AgencyClient {
    AgencyClient::new(self.address()).expect("the stand-in's address is an http address")
  }
}

impl Deref for Stub {
  type Target = stub::Stub;

  fn deref(&self) -> &stub::Stub {
    &self.0
  }
}
