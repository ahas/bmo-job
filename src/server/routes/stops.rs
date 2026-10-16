//! Stopping a run, from the live board or anything else holding the API (docs/api.md, "Stopping runs").
//! A stop names a target, and the kind decides the reach; stopping everything asks every target in turn.

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use job_codes::RunStopRefusal;
use serde_json::Value;

use super::{Answered, State, body};
use crate::server::Server;
use crate::server::request::{Body, Fields, refused};
use crate::verbs::{Stop, Target, TargetKind, Verbs};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/run/stop", post(stop::<V>))
}

// The target a body names: one of a kind and an id, or a unit by its name alone.
fn target(asked: &Value) -> Result<Target, job_base::Refusal> {
  if let Some(named) = asked.get("target").filter(|named| !named.is_null()) {
    return serde_json::from_value::<Target>(named.clone())
      .ok()
      .filter(|target| !target.id.trim().is_empty())
      .ok_or_else(|| {
        refused(RunStopRefusal::StopTargetWrong {
          target: named.to_string(),
        })
      });
  }

  Fields(asked)
    .words_or_none("unit")?
    .map(|unit| Target {
      kind: TargetKind::Unit,
      id: unit,
    })
    .ok_or_else(|| refused(RunStopRefusal::StopTargetMissing))
}

async fn stop<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);

  if fields.on("all") {
    return body(state.verbs.stop_all().await?);
  }

  let stop = Stop {
    target: target(&asked)?,
    why: fields.text_or_none("why")?,
  };

  body(state.verbs.stop_run(stop).await?)
}
