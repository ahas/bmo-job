//! The two things that hold the queue still (docs/api.md, "The queue's holds"): the owner's pause, thrown
//! and let go by him, and a deployment's hold, which the deploy that takes it lets go, or the owner on
//! purpose. Both are rows of the settings table, so whichever reads them next reads the same switch.

use sea_orm::ConnectionTrait;
use serde_json::{Map, Value};

use super::settings;
use crate::Refusal;

const QUEUE_PAUSED_KEY: &str = "queue_paused";
const DEPLOY_HOLD_KEY: &str = "deploy_hold";
const DEPLOY_WAITS_ON_KEY: &str = "deploy_waits_on";

/// Throws the owner's pause or lets it go.
pub async fn set_queue_paused(db: &impl ConnectionTrait, paused: bool) -> Result<(), Refusal> {
  if paused {
    settings::set(db, QUEUE_PAUSED_KEY, "1").await
  } else {
    settings::forget(db, QUEUE_PAUSED_KEY).await
  }
}

/// Whether the owner's own pause stands.
pub async fn queue_paused(db: &impl ConnectionTrait) -> Result<bool, Refusal> {
  Ok(settings::get(db, QUEUE_PAUSED_KEY).await?.as_deref() == Some("1"))
}

/// Whether anything holds the queue still: the owner's pause, or a deployment's hold.
pub async fn queue_held(db: &impl ConnectionTrait) -> Result<bool, Refusal> {
  Ok(queue_paused(db).await? || deploy_held(db).await?)
}

/// Whether a deployment's hold stands. A row that will not read is still a hold.
pub async fn deploy_held(db: &impl ConnectionTrait) -> Result<bool, Refusal> {
  Ok(settings::get(db, DEPLOY_HOLD_KEY).await?.is_some())
}

/// A deployment's hold as its deploy wrote it, where one stands; one that does not read as an object is
/// read as an empty one.
pub async fn deploy_hold(db: &impl ConnectionTrait) -> Result<Option<Map<String, Value>>, Refusal> {
  Ok(settings::get(db, DEPLOY_HOLD_KEY).await?.map(|text| match serde_json::from_str(&text) {
    Ok(Value::Object(hold)) => hold,
    _ => Map::new(),
  }))
}

/// The jobs a deployment holding the queue waits to see end; a row that does not read waits on none.
pub async fn deploy_waits_on(db: &impl ConnectionTrait) -> Result<Vec<i64>, Refusal> {
  let text = settings::get(db, DEPLOY_WAITS_ON_KEY).await?;

  Ok(text.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default())
}

/// Takes the deployment's hold, or writes what it now holds for over the one standing, with the jobs it
/// waits on; none takes that row away.
pub async fn hold_for_deploy(
  db: &impl ConnectionTrait,
  hold: &Map<String, Value>,
  waits_on: &[i64],
) -> Result<(), Refusal> {
  settings::set(db, DEPLOY_HOLD_KEY, &Value::Object(hold.clone()).to_string()).await?;

  if waits_on.is_empty() {
    settings::forget(db, DEPLOY_WAITS_ON_KEY).await
  } else {
    settings::set(db, DEPLOY_WAITS_ON_KEY, &serde_json::json!(waits_on).to_string()).await
  }
}

/// Lets a deployment's hold go, with the jobs it was waiting on; the owner's pause stands on.
pub async fn release_deploy_hold(db: &impl ConnectionTrait) -> Result<(), Refusal> {
  settings::forget(db, DEPLOY_HOLD_KEY).await?;
  settings::forget(db, DEPLOY_WAITS_ON_KEY).await
}
