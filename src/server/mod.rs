//! The server (docs/api.md, "Routes"): every route the service answers, each reading its request into a
//! typed request and answering what its verb or its reading answers. A route the service does not serve
//! is refused `route_absent`.

mod request;
mod routes;
mod units;

use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::response::{IntoResponse, Response};
use job_agency::AgencyClient;
use job_base::{Refusal, Settings};
use job_codes::{ControlRefusal, JobError};
use job_gate::GateClient;
use job_policy::{Names, Policy};
use job_receiver::{Effect, Holder, Holding, Receiver, ReceiverClient};
use queue_rows::wording::{BannedTerm, set_aside_terms};
use sea_orm::DatabaseConnection;
use tokio::net::TcpListener;

use crate::events::Events;
use crate::now_ms;
use crate::talk::{Talking, Talks};
use crate::verbs::Verbs;

/// How often the service declares itself a holder again until bmo-receiver takes it.
const DECLARE_AGAIN: Duration = Duration::from_secs(60);

/// Everything a route stands on.
pub struct Server<V> {
  pub db: DatabaseConnection,
  pub policy: Arc<Policy>,
  pub names: Names,
  pub events: Events,
  pub talking: Talking,
  pub agency: AgencyClient,
  pub gate: GateClient,
  pub receiver: ReceiverClient,
  pub verbs: V,
  /// The terms the owner has set aside, read once out of the wording document the settings name.
  pub banned: Vec<BannedTerm>,
  /// The moment the server was made, in epoch milliseconds.
  pub started_at: i64,
}

/// Why the server could not be made out of its settings.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
  #[error("the settings have no [server] section, which the server requires")]
  NoServer,
  #[error("the settings have no [units] section, which the server requires")]
  NoUnits,
  #[error("{0}")]
  Policy(#[from] job_policy::PolicyError),
  #[error("{0}")]
  Agency(#[from] job_agency::AgencyError),
  #[error("{0}")]
  Gate(#[from] job_gate::GateError),
  #[error("{0}")]
  Receiver(#[from] job_receiver::ReceiverError),
}

impl<V: Verbs> Server<V> {
  /// The server over a database the migrations have run on, with the units its settings name and the
  /// verbs it is built on, on an event stream of its own.
  pub fn new(db: DatabaseConnection, settings: &Settings, verbs: V) -> Result<Self, StartError> {
    Self::on(db, settings, Events::new(), verbs)
  }

  /// The server on the event stream given, which the verbs it is built on publish their news on.
  pub fn on(
    db: DatabaseConnection,
    settings: &Settings,
    events: Events,
    verbs: V,
  ) -> Result<Self, StartError> {
    let units = settings.units.as_ref().ok_or(StartError::NoUnits)?;
    let receiver = ReceiverClient::new(units.receiver.url().clone())?;

    Ok(Self {
      talking: Talking {
        db: db.clone(),
        receiver: receiver.clone(),
        events: events.clone(),
        talks: Arc::new(Talks::default()),
      },
      policy: Arc::new(Policy::load(settings)?),
      names: Names::of(&settings.deployment),
      agency: AgencyClient::new(units.agency.url().clone())?,
      gate: GateClient::new(units.gate.url().clone())?,
      receiver,
      events,
      db,
      verbs,
      banned: set_aside_terms(settings),
      started_at: now_ms(),
    })
  }
}

/// Every route, over the server they stand on.
pub fn router<V: Verbs>(server: Arc<Server<V>>) -> Router {
  routes::all::<V>()
    .fallback(absent)
    .method_not_allowed_fallback(absent)
    .with_state(server)
}

async fn absent() -> Response {
  Refusal::from(JobError::from(ControlRefusal::RouteAbsent)).into_response()
}

/// The routes served on `listener` until `stopping` ends.
pub async fn serve<V: Verbs>(
  listener: TcpListener,
  server: Arc<Server<V>>,
  stopping: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
  axum::serve(listener, router(server)).with_graceful_shutdown(stopping).await
}

/// The service as a holder of rows naming a room (docs/api.md, "A room's holdings"), reached at `address`.
pub fn holder(address: String) -> Holder {
  Holder {
    address,
    holdings: [("items", Effect::Kept), ("questions", Effect::Kept)]
      .into_iter()
      .chain([("open_items", Effect::Warns), ("waiting_questions", Effect::Warns)])
      .map(|(name, effect)| Holding {
        name: name.to_owned(),
        effect,
      })
      .collect(),
  }
}

/// The service declared to bmo-receiver as a holder under its unit's name, and declared again every
/// minute until bmo-receiver takes it where it did not.
pub async fn declare_holder(receiver: ReceiverClient, unit: String, holder: Holder) {
  loop {
    match receiver.declare_holder(&unit, &holder).await {
      Ok(()) => return,
      Err(failed) => eprintln!("bmo-job: the room holdings are not declared yet: {failed}"),
    }

    tokio::time::sleep(DECLARE_AGAIN).await;
  }
}

/// Where the settings stand the server.
pub fn server_settings(settings: &Settings) -> Result<&job_base::ServerSettings, StartError> {
  settings.server.as_ref().ok_or(StartError::NoServer)
}
