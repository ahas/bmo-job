//! The service's port to bmo-gate (docs/api.md, "bmo-gate"): every call it makes there, declared by the
//! trait `Gate` and made over HTTP by `GateClient`. The shapes the calls carry stand in one module to an
//! area.

pub mod client;
pub mod sanitiser;
pub mod translations;

use std::collections::HashMap;
use std::future::Future;

pub use client::GateClient;
pub use sanitiser::{Sanitise, Sanitised};
pub use translations::Reading;

/// Why a call on bmo-gate failed. Each names the call it was.
#[derive(Debug, thiserror::Error)]
pub enum GateError {
  /// The address given is no `http:` address a path can be added to.
  #[error("{address} is not an http address bmo-gate can be reached at")]
  NotAnAddress { address: String },
  /// The call did not reach bmo-gate, or no answer came back.
  #[error("{call}: bmo-gate did not answer: {error}")]
  Unreachable { call: &'static str, error: String },
  /// bmo-gate refused the call, with its status, code and sentence as it gave them.
  #[error("{call}: bmo-gate refused with {status} {code}: {error}")]
  Refused {
    call: &'static str,
    status: u16,
    code: String,
    error: String,
  },
  /// bmo-gate answered with a body that does not read as the call's answer or as a refusal.
  #[error("{call}: bmo-gate answered {status} with a body that does not read: {error}")]
  Malformed {
    call: &'static str,
    status: u16,
    error: String,
  },
}

/// What a call on bmo-gate comes back with.
pub type Answer<T> = Result<T, GateError>;

/// Every call the service makes on bmo-gate.
pub trait Gate: Send + Sync {
  /// The translation kept in `language` of each of `originals` that has one, by its original. Never asks
  /// the translator.
  fn kept(
    &self,
    language: &str,
    originals: &[String],
  ) -> impl Future<Output = Answer<HashMap<String, String>>> + Send;

  /// Each of `originals` read in `language`, in the order given: translated and kept where it was whole,
  /// left in its original where a part of it could not be rendered.
  fn translate(
    &self,
    language: &str,
    originals: &[String],
  ) -> impl Future<Output = Answer<Vec<Reading>>> + Send;

  /// `text` kept as the reading of `original` in `language`, in place of any kept before.
  fn keep(&self, language: &str, original: &str, text: &str) -> impl Future<Output = Answer<()>> + Send;

  /// Words through the sanitiser, as they are to be handed on into a model's context.
  fn sanitise(&self, asked: &Sanitise) -> impl Future<Output = Answer<Sanitised>> + Send;
}
