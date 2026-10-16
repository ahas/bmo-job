//! The service's port to bmo-graph (docs/api.md, "bmo-graph"): every call it makes there, declared by the
//! trait `Graph` and made over HTTP by `GraphClient`. The shapes the calls carry stand in one module to an
//! area.

pub mod client;
pub mod named;

use std::future::Future;

pub use client::GraphClient;
pub use named::Named;

/// Why a call on bmo-graph failed. Each names the call it was.
#[derive(Debug, thiserror::Error)]
pub enum GraphError {
  /// The address given is no `http:` address a path can be added to.
  #[error("{address} is not an http address bmo-graph can be reached at")]
  NotAnAddress { address: String },
  /// The call did not reach bmo-graph, or no answer came back.
  #[error("{call}: bmo-graph did not answer: {error}")]
  Unreachable { call: &'static str, error: String },
  /// bmo-graph refused the call, with its status, code and sentence as it gave them.
  #[error("{call}: bmo-graph refused with {status} {code}: {error}")]
  Refused {
    call: &'static str,
    status: u16,
    code: String,
    error: String,
  },
  /// bmo-graph answered with a body that does not read as the call's answer or as a refusal.
  #[error("{call}: bmo-graph answered {status} with a body that does not read: {error}")]
  Malformed {
    call: &'static str,
    status: u16,
    error: String,
  },
}

/// What a call on bmo-graph comes back with.
pub type Answer<T> = Result<T, GraphError>;

/// Every call the service makes on bmo-graph.
pub trait Graph: Send + Sync {
  /// Where the graph stands in the checkout at `checkout`, an absolute path, and every file its nodes
  /// name.
  fn named(&self, checkout: &str) -> impl Future<Output = Answer<Named>> + Send;
}
