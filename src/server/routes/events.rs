//! The event stream (docs/api.md, "The event stream"): the run, the events after the screen's cursor, and
//! everything as it comes, as server-sent events with a comment between them so the line never stands
//! idle; and the queue told moved by a process of the service's own that changed it.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use axum::Router;
use axum::extract::Query;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use futures_util::stream::{self, Stream, StreamExt};
use serde_json::{Value, json};
use tokio_stream::wrappers::BroadcastStream;

use super::{Answered, State, nothing};
use crate::server::Server;
use crate::server::request::{Body, Fields};
use crate::verbs::Verbs;

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/events", get(listen::<V>)).route("/queue/moved", post(moved::<V>))
}

async fn moved<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let detail = Fields(&asked).words_or_none("detail")?;

  state.events.queue_moved(detail.as_deref());
  nothing()
}

// One event of the stream as it goes out: its id, its name and its data.
fn event(news: &Value) -> Event {
  Event::default().id(news["id"].to_string()).event("job").data(news.to_string())
}

fn frames(
  opening: Vec<Event>,
  coming: BroadcastStream<Value>,
) -> impl Stream<Item = Result<Event, Infallible>> {
  // A screen that fell behind the stream misses what it lagged past and reads the database again.
  let coming = coming.filter_map(|news| std::future::ready(news.ok().map(|news| Ok(event(&news)))));

  stream::iter(opening.into_iter().map(Ok)).chain(coming)
}

async fn listen<V: Verbs>(state: State<V>, Query(query): Query<HashMap<String, String>>) -> Response {
  let after = query.get("after").and_then(|after| after.trim().parse().ok()).unwrap_or(0);
  let listening = state.events.listen(after, query.get("run").map(String::as_str));
  let mut opening = vec![Event::default().event("run").data(json!({ "run": listening.run }).to_string())];

  opening.extend(listening.replay.iter().map(event));

  Sse::new(frames(opening, BroadcastStream::new(listening.coming)))
    .keep_alive(KeepAlive::default())
    .into_response()
}
