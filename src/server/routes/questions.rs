//! A room's open questions, and a job's question answered, talked over, and its clock stopped and started
//! again (docs/api.md, "Questions").

use std::collections::HashMap;
use std::sync::Arc;

use axum::Router;
use axum::extract::Query;
use axum::routing::{get, post};
use job_codes::{FieldForm, JobAskRefusal};
use serde::Serialize;
use serde_json::Value;

use super::{Answered, State, body};
use crate::now_ms;
use crate::queue::questions;
use crate::server::Server;
use crate::server::request::{Body, Fields, refused, wrong};
use crate::verbs::{Settling, Verbs};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new()
    .route("/questions/open", get(open::<V>))
    .route("/question", post(answer::<V>))
    .route("/question-pause", post(pause::<V>))
    .route("/question-talk", post(talk::<V>))
}

/// An open question as bmo-receiver reads the owner's words against it.
#[derive(Serialize)]
struct OpenQuestion {
  id: i64,
  options: Vec<String>,
  talks: bool,
}

async fn open<V: Verbs>(state: State<V>, Query(query): Query<HashMap<String, String>>) -> Answered {
  let chat = query
    .get("chat")
    .and_then(|chat| chat.trim().parse::<i64>().ok())
    .ok_or_else(|| wrong("chat", FieldForm::WholeNumber))?;
  let open = questions::open_in(&state.db, chat, now_ms()).await?;

  body(
    open
      .into_iter()
      .map(|question| OpenQuestion {
        id: question.id,
        options: question.options,
        talks: question.talks,
      })
      .collect::<Vec<_>>(),
  )
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

async fn talk<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  let id = fields.whole_or_none("id")?.ok_or_else(|| refused(JobAskRefusal::QuestionTalkNumber))?;
  let say = fields.words_or_none("say")?.ok_or_else(|| refused(JobAskRefusal::QuestionTalkSay))?;

  body(state.verbs.talk_question(id, say).await?)
}
