//! The service's health: its version and when it started (docs/api.md, "Health").

use std::sync::Arc;

use axum::Router;
use axum::routing::get;
use serde_json::json;

use super::{Answered, State, body};
use crate::server::Server;
use crate::verbs::Verbs;

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/health", get(health::<V>))
}

async fn health<V: Verbs>(state: State<V>) -> Answered {
  body(json!({ "version": env!("CARGO_PKG_VERSION"), "started_at": state.started_at }))
}
