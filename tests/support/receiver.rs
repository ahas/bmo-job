//! The stand-in for bmo-receiver: the route of every call the service makes there (docs/api.md,
//! "bmo-receiver"), served by the shared stand-in.

use std::ops::Deref;

use axum::Router;
use axum::routing::{get, post, put};
use job_receiver::ReceiverClient;

use super::stub::{self, Shared, take};

/// The stand-in for bmo-receiver, serving until it is dropped.
pub struct Stub(stub::Stub);

/// Every route of the calls, each taken by `take`.
fn routes() -> Router<Shared> {
  Router::new()
    .route("/instructions", post(take))
    .route("/instructions/{id}", get(take).delete(take))
    .route("/rooms/{chat}", get(take))
    .route("/rooms/{chat}/notices", post(take))
    .route("/rooms/{chat}/answerers", get(take))
    .route("/rooms/{chat}/messages/{message}", get(take))
    .route("/owner", get(take))
    .route("/talks", post(take))
    .route("/room-holders/{unit}", put(take))
}

impl Stub {
  /// Starts the stand-in on a free loopback port.
  pub async fn start() -> Self {
    Self(stub::Stub::start(routes()).await)
  }

  /// The port's client, calling the stand-in.
  pub fn client(&self) -> ReceiverClient {
    ReceiverClient::new(self.address()).expect("the stand-in's address is an http address")
  }
}

impl Deref for Stub {
  type Target = stub::Stub;

  fn deref(&self) -> &stub::Stub {
    &self.0
  }
}
