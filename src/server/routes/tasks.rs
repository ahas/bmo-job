//! A job launched on a worker (docs/api.md, "Jobs"). Which worker and checkout take it, and every refusal
//! a launch is owed, are the launch verb's.

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use job_codes::TaskRefusal;

use super::{Answered, State, body};
use crate::server::Server;
use crate::server::request::{Body, Fields, refused};
use crate::verbs::{TaskLaunch, Verbs};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/task", post(task::<V>))
}

async fn task<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  // A launch is of a room and of something to do, and an item named by anything but a whole number names
  // no item a launch can be of.
  let needs = || refused(TaskRefusal::TaskNeedsRoomAndWork);
  let chat = fields.whole_or_none("chat").map_err(|_| needs())?.ok_or_else(needs)?;
  let prompt = fields
    .text_or_none("prompt")
    .map_err(|_| needs())?
    .filter(|prompt| !prompt.trim().is_empty());
  let item = fields.whole_or_none("item").map_err(|_| needs())?;
  let launch = TaskLaunch {
    chat,
    prompt: prompt.ok_or_else(needs)?,
    model: fields.text_or_none("model")?,
    item,
    purpose: fields.words_or_none("purpose")?,
    worker: fields.text_or_none("worker")?,
    cowork: fields.on("cowork"),
    started_by: fields.text_or_none("started_by")?,
  };

  body(state.verbs.launch_task(launch).await?)
}
