//! The queue's items changed from the console (docs/api.md, "The TODO"): each action read with what it
//! takes and handed to the TODO's verb.

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use job_base::Refusal;
use job_codes::{ControlRefusal, FieldForm, TodoRefusal};

use super::{Answered, State, body};
use crate::server::Server;
use crate::server::request::{Body, Fields, refused, wrong};
use crate::verbs::{TodoAction, Verbs};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/todo", post(change::<V>))
}

// The action a body asks for, with what it takes.
fn action(fields: &Fields<'_>, asked: &str) -> Result<TodoAction, Refusal> {
  Ok(match asked {
    "approve" | "unapprove" => {
      let note = fields.words_or_none("note")?;
      let by = fields.words_or_none("by")?;

      if asked == "approve" {
        TodoAction::Approve { note, by }
      } else {
        TodoAction::Unapprove { note, by }
      }
    }
    "move" => TodoAction::Move {
      up: match fields.text_or_none("direction").ok().flatten().as_deref() {
        Some("up") => true,
        Some("down") => false,
        _ => return Err(wrong("direction", FieldForm::Direction)),
      },
    },
    "top" => TodoAction::Top,
    "bottom" => TodoAction::Bottom,
    "reorder" => TodoAction::Reorder {
      order: fields.numbers("order")?,
    },
    "drop" => TodoAction::Drop,
    "done" => TodoAction::Done,
    "resolve" => TodoAction::Resolve {
      why: fields.words_or_none("why")?.ok_or_else(|| refused(TodoRefusal::ResolveNeedsWhy))?,
    },
    "raise" => TodoAction::Raise {
      title: fields.words("title")?,
      said: fields.words("said")?,
      sources: fields.numbers_or_none("sources")?.unwrap_or_default(),
    },
    "append" => TodoAction::Append {
      text: fields.words_or_none("text")?.ok_or_else(|| refused(TodoRefusal::AppendEmpty))?,
    },
    "unappend" => TodoAction::Unappend {
      note: fields.whole("note")?,
    },
    "hold" => TodoAction::Hold {
      why: fields.words_or_none("why")?,
    },
    "release" => TodoAction::Release,
    "stop" => TodoAction::Stop {
      why: fields.words_or_none("why")?.ok_or_else(|| refused(TodoRefusal::StopNeedsWhy))?,
    },
    "worker" => TodoAction::Worker {
      worker: fields.words_or_none("worker")?,
    },
    "cowork" => TodoAction::Cowork {
      on: fields.boolean("cowork").map_err(|_| refused(TodoRefusal::CoworkNeedsSwitch))?,
    },
    _ => return Err(refused(ControlRefusal::ActionWrong)),
  })
}

async fn change<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  let asked_action = fields.words("action")?;
  let id = fields.whole("id")?;
  let action = action(&fields, &asked_action)?;

  body(state.verbs.change_todo(id, action).await?)
}
