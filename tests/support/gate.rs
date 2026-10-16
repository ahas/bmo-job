//! The stand-in for bmo-gate: the route of every call the service makes there (docs/api.md, "bmo-gate"),
//! served by the shared stand-in.

use std::ops::Deref;

use axum::Router;
use axum::routing::{post, put};
use job_gate::GateClient;

use super::stub::{self, Shared, take};

/// The stand-in for bmo-gate, serving until it is dropped.
pub struct Stub(stub::Stub);

/// Every route of the calls, each taken by `take`.
fn routes() -> Router<Shared> {
  Router::new()
    .route("/translations/kept/read", post(take))
    .route("/translations", post(take))
    .route("/translations/kept", put(take))
    .route("/sanitised", post(take))
}

impl Stub {
  /// Starts the stand-in on a free loopback port.
  pub async fn start() -> Self {
    Self(stub::Stub::start(routes()).await)
  }

  /// The port's client, calling the stand-in.
  pub fn client(&self) -> GateClient {
    GateClient::new(self.address()).expect("the stand-in's address is an http address")
  }
}

impl Deref for Stub {
  type Target = stub::Stub;

  fn deref(&self) -> &stub::Stub {
    &self.0
  }
}
