//! The queue's holds (docs/api.md, "The queue's holds"): the owner's pause and a deployment's hold, read,
//! taken and let go. Pausing tells nothing; resuming and letting a hold go tell the agent how the queue
//! stands.

use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post, put};
use job_codes::QueueTold;
use serde::Serialize;

use super::{Answered, State, body, nothing};
use crate::queue::holds;
use crate::server::Server;
use crate::server::request::{Body, Fields};
use crate::verbs::{Moment, Verbs};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new()
    .route("/queue/holds", get(read::<V>))
    .route("/queue-pause", post(pause::<V>))
    .route("/deploy-hold-release", post(release_by_owner::<V>))
    .route("/queue/deploy-hold", put(take::<V>).delete(release_by_deploy::<V>))
}

#[derive(Serialize)]
struct Holds {
  paused: bool,
  deploy_hold: Option<serde_json::Map<String, serde_json::Value>>,
  deploy_waits_on: Vec<i64>,
}

#[derive(Serialize)]
struct Paused {
  paused: bool,
  deploy_held: bool,
  told: Option<QueueTold>,
}

#[derive(Serialize)]
struct Released {
  released: bool,
  told: Option<QueueTold>,
}

async fn read<V: Verbs>(state: State<V>) -> Answered {
  body(Holds {
    paused: holds::queue_paused(&state.db).await?,
    deploy_hold: holds::deploy_hold(&state.db).await?,
    deploy_waits_on: holds::deploy_waits_on(&state.db).await?,
  })
}

// How the queue stands told once a hold is let go, whoever let it go.
fn resumed() -> Moment {
  Moment {
    moment: "resumed".into(),
    repeat: true,
    rides_ms: None,
  }
}

async fn pause<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let paused = Fields(&asked).boolean("paused")?;

  holds::set_queue_paused(&state.db, paused).await?;
  state.events.queue_moved(None);

  let told = if paused { None } else { Some(state.verbs.tell_queue(resumed()).await?) };

  body(Paused {
    paused,
    deploy_held: holds::deploy_held(&state.db).await?,
    told,
  })
}

// A deployment's hold let go, by the owner on purpose or by the deploy that took it; one that does not
// stand tells nothing.
async fn release<V: Verbs>(state: State<V>, by: &str) -> Answered {
  if !holds::deploy_held(&state.db).await? {
    return body(Released {
      released: false,
      told: None,
    });
  }

  holds::release_deploy_hold(&state.db).await?;
  state.events.queue_moved(Some(by));

  body(Released {
    released: true,
    told: Some(state.verbs.tell_queue(resumed()).await?),
  })
}

async fn release_by_owner<V: Verbs>(state: State<V>) -> Answered {
  release(state, "owner").await
}

async fn release_by_deploy<V: Verbs>(state: State<V>) -> Answered {
  release(state, "deploy").await
}

async fn take<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  let hold = fields.object("hold")?;
  let waits_on = fields.numbers_or_none("waits_on")?.unwrap_or_default();

  holds::hold_for_deploy(&state.db, &hold, &waits_on).await?;
  state.events.queue_moved(Some("deploy"));

  nothing()
}
