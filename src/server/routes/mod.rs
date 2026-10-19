//! Every route of the server, one module to an area of docs/api.md's "Routes".

mod events;
mod health;
mod holdings;
mod holds;
pub mod hooks;
mod items;
mod jobs;
mod pool;
mod questions;
mod readings;
mod runners;
mod sessions;
mod stops;
mod talk;
mod tasks;
mod todos;
mod words;
mod work_orders;
mod workers;

use std::sync::Arc;

use axum::Router;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use job_base::Refusal;
use serde::Serialize;

use super::Server;
use crate::verbs::Verbs;

/// What every route stands on.
type State<V> = axum::extract::State<Arc<Server<V>>>;

/// What a route answers: a response, or the refusal.
type Answered = Result<Response, Refusal>;

/// An answer with a body.
fn body(answer: impl Serialize) -> Answered {
  Ok(Json(answer).into_response())
}

/// An answer with no body.
fn nothing() -> Answered {
  Ok(StatusCode::NO_CONTENT.into_response())
}

/// Every route, merged.
pub fn all<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new()
    .merge(health::routes())
    .merge(holds::routes())
    .merge(jobs::routes())
    .merge(runners::routes())
    .merge(pool::routes())
    .merge(questions::routes())
    .merge(stops::routes())
    .merge(talk::routes())
    .merge(tasks::routes())
    .merge(todos::routes())
    .merge(work_orders::routes())
    .merge(workers::routes())
    .merge(words::routes())
    .merge(sessions::routes())
    .merge(readings::routes())
    .merge(items::routes())
    .merge(holdings::routes())
    .merge(hooks::routes())
    .merge(events::routes())
}
