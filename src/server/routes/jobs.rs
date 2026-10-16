//! A job's end and the report it left waiting, and the queue's standing told at a verb's moment (docs/api.md,
//! "A job's end").

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use serde_json::json;

use super::{Answered, State, body, nothing};
use crate::server::Server;
use crate::server::request::{Body, Fields};
use crate::verbs::{JobEnded, Moment, Verbs};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new()
    .route("/handoff", post(handoff::<V>))
    .route("/job-report", post(report::<V>))
    .route("/queue/tell", post(tell::<V>))
}

async fn handoff<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  let ended = JobEnded {
    outcome: fields.words("outcome")?,
    job: fields.whole_or_none("job")?,
  };

  body(json!({ "told": state.verbs.job_ended(ended).await? }))
}

async fn report<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let job = Fields(&asked).whole("job")?;

  state.verbs.deliver_report(job).await?;

  nothing()
}

async fn tell<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  let moment = Moment {
    moment: fields.words("moment")?,
    repeat: fields.on("repeat"),
    rides_ms: fields.whole_or_none("rides_ms")?,
  };

  body(json!({ "told": state.verbs.tell_queue(moment).await? }))
}
