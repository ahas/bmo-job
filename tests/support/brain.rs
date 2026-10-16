//! The stand-in for bmo-brain: the route of every call the service makes there (docs/api.md, "bmo-brain"),
//! served by the shared stand-in.

use std::ops::Deref;

use axum::Router;
use axum::routing::{get, post};
use job_brain::BrainClient;

use super::stub::{self, Shared, take};

/// The token the port's client calls the stand-in with.
pub const TOKEN: &str = "brain-token";

/// The stand-in for bmo-brain, serving until it is dropped.
pub struct Stub(stub::Stub);

/// Every route of the calls, each taken by `take`.
fn routes() -> Router<Shared> {
  Router::new()
    .route("/brain/hold", post(take).get(take))
    .route("/brain/wait", post(take))
    .route("/memories", get(take))
    .route("/memory/add", post(take))
}

impl Stub {
  /// Starts the stand-in on a free loopback port.
  pub async fn start() -> Self {
    Self(stub::Stub::start(routes()).await)
  }

  /// The port's client, calling the stand-in with `TOKEN`.
  pub fn client(&self) -> BrainClient {
    BrainClient::new(self.address(), TOKEN).expect("the stand-in's address is an http address")
  }
}

impl Deref for Stub {
  type Target = stub::Stub;

  fn deref(&self) -> &stub::Stub {
    &self.0
  }
}
