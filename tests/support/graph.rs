//! The stand-in for bmo-graph: the route of every call the service makes there (docs/api.md, "bmo-graph"),
//! served by the shared stand-in.

use std::ops::Deref;

use axum::Router;
use axum::routing::get;
use job_graph::GraphClient;

use super::stub::{self, Shared, take};

/// The stand-in for bmo-graph, serving until it is dropped.
pub struct Stub(stub::Stub);

/// Every route of the calls, each taken by `take`.
fn routes() -> Router<Shared> {
  Router::new().route("/graph/named", get(take))
}

impl Stub {
  /// Starts the stand-in on a free loopback port.
  pub async fn start() -> Self {
    Self(stub::Stub::start(routes()).await)
  }

  /// The port's client, calling the stand-in.
  pub fn client(&self) -> GraphClient {
    GraphClient::new(self.address()).expect("the stand-in's address is an http address")
  }
}

impl Deref for Stub {
  type Target = stub::Stub;

  fn deref(&self) -> &stub::Stub {
    &self.0
  }
}
