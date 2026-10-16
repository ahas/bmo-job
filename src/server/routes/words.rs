//! A word the owner said to one worker (docs/api.md, "A worker's word"), as JSON or as a form carrying
//! files; no more files than the queue's policy takes.

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use job_codes::{ControlRefusal, FieldForm};

use super::{Answered, State, nothing};
use crate::server::Server;
use crate::server::request::{Fields, Sent, refused, wrong};
use crate::verbs::{Verbs, WorkerWord};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/worker-word", post(say::<V>))
}

async fn say<V: Verbs>(state: State<V>, sent: Sent) -> Answered {
  let most = state.policy.queue.word_files;

  if sent.files.len() > most {
    return Err(refused(ControlRefusal::TooManyFiles {
      most: u32::try_from(most).unwrap_or(u32::MAX),
    }));
  }

  let fields = Fields(&sent.body);
  let text = fields.text_or_none("text")?.unwrap_or_default();

  // A word may be its files alone.
  if text.trim().is_empty() && sent.files.is_empty() {
    return Err(wrong("text", FieldForm::Words));
  }

  let word = WorkerWord {
    worker: fields.words_or_none("worker")?,
    text: text.trim().to_owned(),
    files: sent.files,
  };

  state.verbs.say_to_worker(word).await?;

  nothing()
}
