//! A runner started on the owner's own words (docs/api.md, "Runners"). A runner is shown the web and
//! nothing on this machine, so it takes no file.

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use job_codes::{ControlRefusal, FieldForm};

use super::{Answered, State, nothing};
use crate::server::Server;
use crate::server::request::{Fields, Sent, refused, wrong};
use crate::verbs::{RunnerLaunch, Verbs};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/launch", post(launch::<V>))
}

async fn launch<V: Verbs>(state: State<V>, sent: Sent) -> Answered {
  if !sent.files.is_empty() {
    return Err(refused(ControlRefusal::TakesNoFiles));
  }

  let fields = Fields(&sent.body);
  let depth = fields.text_or_none("depth").map_err(|_| wrong("depth", FieldForm::Text))?;
  let launch = RunnerLaunch {
    agent: fields.words("agent")?,
    text: fields.words("text")?,
    depth: depth.filter(|depth| !depth.is_empty()),
  };

  state.verbs.launch_runner(launch).await?;

  nothing()
}
