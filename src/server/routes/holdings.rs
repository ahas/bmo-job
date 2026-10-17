//! What the service holds of a room bmo-receiver is about to remove (docs/api.md, "A room's holdings"):
//! the open items filed from it and the questions of its jobs still waiting, each named only where it holds
//! any.

use std::sync::Arc;

use axum::Router;
use axum::extract::Path;
use axum::routing::get;
use job_codes::RoomHoldings;

use super::{Answered, State, body};
use crate::queue::todo_holdings;
use crate::server::Server;
use crate::verbs::Verbs;

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/rooms/{chat}/holdings", get(holdings::<V>))
}

async fn holdings<V: Verbs>(state: State<V>, Path(chat): Path<i64>) -> Answered {
  let items = todo_holdings::open_items(&state.db, chat).await?;
  let questions = todo_holdings::waiting_questions(&state.db, chat).await?;
  let held = [
    (items > 0).then_some(RoomHoldings::RoomOpenItems { count: items }),
    (questions > 0).then_some(RoomHoldings::RoomWaitingQuestions { count: questions }),
  ];

  body(held.into_iter().flatten().collect::<Vec<_>>())
}
