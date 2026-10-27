//! The bmo-receiver port (docs/api.md, "The bmo-receiver port"): a room's holdings, its open questions and
//! items, and the owner's answer or talk on a question, in the shapes bmo-receiver reads.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use bmo_job_client::{ApprovalKind, Code, ErrorBody, NewReply, QuestionState};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use super::App;
use super::extract::{InBody, InPath, InQuery};
use crate::refusal::Refusal;

/// Who the owner's answer and talk are written as: the port speaks for him.
const OWNER: &str = "owner";

/// How a port route refuses: a status, a code and one English sentence.
pub(super) struct PortError {
  status: StatusCode,
  code: &'static str,
  sentence: String,
}

impl PortError {
  fn new(status: StatusCode, code: &'static str, sentence: impl Into<String>) -> Self {
    Self {
      status,
      code,
      sentence: sentence.into(),
    }
  }
}

impl From<Refusal> for PortError {
  fn from(refusal: Refusal) -> Self {
    Self {
      status: StatusCode::from_u16(refusal.code.status()).expect("every code's status is a status"),
      code: refusal.code.as_str(),
      sentence: refusal.sentence,
    }
  }
}

impl IntoResponse for PortError {
  fn into_response(self) -> Response {
    let body = ErrorBody {
      code: self.code.to_owned(),
      error: self.sentence,
    };
    (self.status, Json(body)).into_response()
  }
}

fn absent(id: i64) -> PortError {
  PortError::new(StatusCode::NOT_FOUND, "question_absent", format!("There is no question {id}."))
}

/// What a room holds, each thing named only where it holds any.
#[derive(Serialize)]
struct Holding {
  code: &'static str,
  count: i64,
}

pub(super) async fn holdings(
  State(app): State<App>,
  InPath(chat): InPath<i64>,
) -> Result<Response, PortError> {
  let scope = chat.to_string();
  let items = app.store.items_in_scope(&scope).await?;
  let questions = app.store.questions_waiting_in(&scope).await?;
  let held: Vec<Holding> = [
    (items > 0).then_some(Holding {
      code: "room_open_items",
      count: items,
    }),
    (questions > 0).then_some(Holding {
      code: "room_waiting_questions",
      count: questions,
    }),
  ]
  .into_iter()
  .flatten()
  .collect();
  Ok(Json(held).into_response())
}

/// What a room's open questions are asked of.
#[derive(Deserialize)]
pub(super) struct ChatQuery {
  chat: i64,
}

/// A question as the owner's words are read against it.
#[derive(Serialize)]
struct OpenQuestion {
  id: i64,
  options: Vec<String>,
  recommendation: Option<String>,
  why: Option<String>,
  talks: bool,
}

pub(super) async fn open_questions(
  State(app): State<App>,
  InQuery(query): InQuery<ChatQuery>,
) -> Result<Response, PortError> {
  let open = app.store.open_questions_in(&query.chat.to_string(), app.store.now()).await?;
  let questions: Vec<OpenQuestion> = open
    .into_iter()
    .map(|question| {
      let recommendation = question.recommendation;
      OpenQuestion {
        id: question.id,
        options: question.choices,
        recommendation: recommendation.as_ref().map(|recommendation| recommendation.answer.clone()),
        why: recommendation.map(|recommendation| recommendation.why),
        // Any question may be talked over; the asker hears a word without an answer.
        talks: true,
      }
    })
    .collect();
  Ok(Json(questions).into_response())
}

/// The open items, as the port lists them.
#[derive(Serialize)]
struct OpenItems {
  items: Vec<ItemRow>,
}

/// One open item: its key, whether it is approved, its state, its title and the one assigned to it.
#[derive(Serialize)]
struct ItemRow {
  id: i64,
  approved: bool,
  status: String,
  title: String,
  worker: Option<String>,
}

pub(super) async fn open_items(State(app): State<App>) -> Result<Response, PortError> {
  let items = app.store.open_items().await?;
  let rows: Vec<ItemRow> = items
    .into_iter()
    .map(|item| ItemRow {
      id: item.id,
      approved: item.approvals.iter().any(|approval| approval.kind == ApprovalKind::Approval),
      status: item.state.as_str().to_owned(),
      title: item.title,
      worker: item.assignee_name,
    })
    .collect();
  Ok(Json(OpenItems { items: rows }).into_response())
}

/// A question settled with one of its choices, by its number counted from zero.
#[derive(Deserialize)]
pub(super) struct Chosen {
  id: i64,
  chosen: i64,
}

pub(super) async fn settle(
  State(app): State<App>,
  InBody(chosen): InBody<Chosen>,
) -> Result<Response, PortError> {
  let question = match app.store.question(chosen.id).await {
    Ok(question) => question,
    Err(refusal) if refusal.code == Code::NotFound => return Err(absent(chosen.id)),
    Err(refusal) => return Err(refusal.into()),
  };
  if matches!(question.state, QuestionState::Answered | QuestionState::Withdrawn) {
    return Ok(StatusCode::NO_CONTENT.into_response());
  }
  let at = usize::try_from(chosen.chosen)
    .ok()
    .filter(|at| *at < question.choices.len())
    .ok_or_else(|| {
      PortError::new(
        StatusCode::BAD_REQUEST,
        "question_answer_wrong",
        format!("The question {} has no option {}.", chosen.id, chosen.chosen),
      )
    })?;
  let option = question.choices[at].clone();
  let answer = NewReply {
    choice: Some(option.clone()),
    text: option,
    actor_name: OWNER.to_owned(),
    settles: Some(true),
  };
  match app.store.reply(chosen.id, answer).await {
    // A question another answer settled between the read and the write is settled all the same.
    Ok(_) => Ok(StatusCode::NO_CONTENT.into_response()),
    Err(refusal) if refusal.code == Code::Conflict => Ok(StatusCode::NO_CONTENT.into_response()),
    Err(refusal) => Err(refusal.into()),
  }
}

/// A line of talk over an open question.
#[derive(Deserialize)]
pub(super) struct Talked {
  id: i64,
  say: String,
}

pub(super) async fn talk(
  State(app): State<App>,
  InBody(talked): InBody<Talked>,
) -> Result<Response, PortError> {
  let say = talked.say.trim();
  if say.is_empty() {
    return Err(PortError::new(
      StatusCode::BAD_REQUEST,
      "question_talk_say",
      "The talk has no words of its own.",
    ));
  }
  match app.store.talk(talked.id, say.to_owned(), OWNER.to_owned()).await {
    Ok(_) => Ok(StatusCode::NO_CONTENT.into_response()),
    Err(refusal) if refusal.code == Code::NotFound => Err(absent(talked.id)),
    Err(refusal) if refusal.code == Code::Conflict => Err(PortError::new(
      StatusCode::CONFLICT,
      "question_talk_settled",
      format!("The question {} is settled already and takes no more talk.", talked.id),
    )),
    Err(refusal) => Err(refusal.into()),
  }
}

/// The queue's standing put into the stream as moved, with who moved it where one is named.
#[derive(Deserialize)]
pub(super) struct Moved {
  #[serde(default)]
  detail: Option<String>,
}

pub(super) async fn queue_moved(
  State(app): State<App>,
  InBody(moved): InBody<Moved>,
) -> Result<Response, PortError> {
  let mut data = Map::new();
  if let Some(detail) = moved.detail {
    data.insert("detail".into(), Value::String(detail));
  }
  app.store.publish("queue.moved", data).await?;
  Ok(StatusCode::NO_CONTENT.into_response())
}
