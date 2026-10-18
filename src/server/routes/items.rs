//! The items still open on the queue (docs/api.md, "The open items").

use std::sync::Arc;

use axum::Router;
use axum::extract::Query;
use axum::routing::get;
use job_codes::FieldForm;
use serde_json::json;

use super::{Answered, State, body};
use crate::queue::open_items;
use crate::server::Server;
use crate::server::request::wrong;
use crate::verbs::Verbs;

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/items", get(listed::<V>))
}

// The one listing the route gives is the open one, so its flag must be set: bare, or with any value but
// `false` or `0`.
async fn listed<V: Verbs>(state: State<V>, Query(query): Query<Vec<(String, String)>>) -> Answered {
  let open = query
    .iter()
    .any(|(key, value)| key == "open" && !matches!(value.as_str(), "false" | "0"));

  if !open {
    return Err(wrong("open", FieldForm::Boolean));
  }

  body(json!({ "items": open_items::listed(&state.db).await? }))
}
