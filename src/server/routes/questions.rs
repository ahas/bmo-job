//! A job's question answered from the console, and its clock stopped and started again (docs/api.md,
//! "Questions").

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use job_codes::JobAskRefusal;
use serde_json::Value;

use super::{Answered, State, body};
use crate::server::Server;
use crate::server::request::{Body, Fields, refused};
use crate::verbs::{Settling, Verbs};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new()
    .route("/question", post(answer::<V>))
    .route("/question-pause", post(pause::<V>))
}

// What the body settles the question with: an option by its number, or words with something in them,
// never both.
fn settling(asked: &Value) -> Option<Settling> {
  let chosen = asked.get("chosen").filter(|chosen| !chosen.is_null());
  let words = asked.get("answer").filter(|words| !words.is_null());

  match (chosen, words) {
    (Some(chosen), None) => chosen.as_u64().and_then(|at| u32::try_from(at).ok()).map(Settling::Chosen),
    (None, Some(words)) => words
      .as_str()
      .map(str::trim)
      .filter(|words| !words.is_empty())
      .map(|words| Settling::Words(words.to_owned())),
    _ => None,
  }
}

async fn answer<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let id = Fields(&asked).whole("id")?;
  let settling = settling(&asked).ok_or_else(|| refused(JobAskRefusal::QuestionAnswerWrong))?;

  body(state.verbs.answer_question(id, settling).await?)
}

async fn pause<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  let id = fields.whole("id")?;
  let paused = fields.boolean("paused")?;

  body(state.verbs.pause_question(id, paused).await?)
}
