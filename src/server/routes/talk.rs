//! The talk about an item (docs/api.md, "The talk about an item"): where it stands, and a turn of it,
//! answered at once while the agent's answer comes later.

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::Query;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use job_codes::FieldForm;

use super::{Answered, State, body};
use crate::server::Server;
use crate::server::request::{Body, Fields, wrong};
use crate::verbs::Verbs;

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/todo-talk", get(standing::<V>).post(turn::<V>))
}

async fn standing<V: Verbs>(state: State<V>, Query(query): Query<HashMap<String, String>>) -> Answered {
  let id = query
    .get("id")
    .and_then(|id| id.trim().parse::<i64>().ok())
    .ok_or_else(|| wrong("id", FieldForm::WholeNumber))?;

  body(state.talking.talks.state(id))
}

async fn turn<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  let id = fields.whole("id")?;
  let said = fields.words("text")?;

  state.talking.take_turn(id, &said).await?;

  Ok(StatusCode::ACCEPTED.into_response())
}
