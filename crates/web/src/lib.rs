//! The service's port to bmo-web (docs/api.md, "bmo-web"): every call it makes there, declared by the
//! trait `Web` and made over HTTP by `WebClient`. The shapes the calls carry stand in one module to an
//! area.

pub mod checks;
pub mod client;

use std::future::Future;

pub use checks::{Checked, ObjectCheck, ObjectKind};
pub use client::WebClient;

/// Why a call on bmo-web failed. Each names the call it was.
#[derive(Debug, thiserror::Error)]
pub enum WebError {
  /// The address given is no `http:` address a path can be added to.
  #[error("{address} is not an http address bmo-web can be reached at")]
  NotAnAddress { address: String },
  /// The call did not reach bmo-web, or no answer came back.
  #[error("{call}: bmo-web did not answer: {error}")]
  Unreachable { call: &'static str, error: String },
  /// bmo-web refused the call, with its status, code and sentence as it gave them.
  #[error("{call}: bmo-web refused with {status} {code}: {error}")]
  Refused {
    call: &'static str,
    status: u16,
    code: String,
    error: String,
  },
  /// bmo-web answered with a body that does not read as the call's answer or as a refusal.
  #[error("{call}: bmo-web answered {status} with a body that does not read: {error}")]
  Malformed {
    call: &'static str,
    status: u16,
    error: String,
  },
}

/// What a call on bmo-web comes back with.
pub type Answer<T> = Result<T, WebError>;

/// Every call the service makes on bmo-web.
pub trait Web: Send + Sync {
  /// Whether the account model is in force.
  fn model_in_force(&self) -> impl Future<Output = Answer<bool>> + Send;

  /// The user a sender is linked to, `None` where it is linked to none.
  fn user_of_sender(&self, sender: i64) -> impl Future<Output = Answer<Option<String>>> + Send;

  /// Whether a user holds a relation on a worker or a repository, with the grant that was asked for.
  fn check_object(&self, check: &ObjectCheck) -> impl Future<Output = Answer<Checked>> + Send;
}
