//! The co-worker pool's board (docs/api.md, "The co-worker pool board").

use std::sync::Arc;

use axum::Router;
use axum::routing::get;

use super::{Answered, State, body};
use crate::server::Server;
use crate::verbs::Verbs;

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/coworker-pool", get(board::<V>))
}

async fn board<V: Verbs>(state: State<V>) -> Answered {
  body(state.verbs.pool_board().await?)
}
