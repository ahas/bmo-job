//! `serve` (docs/api.md, "Routes"): the database migrated up, the routes served on `[server] listen` over the
//! service's verbs until the process is told to stop, the work-order runner started beside them, and the
//! service declared to bmo-receiver as a holder of rooms' rows.

use std::process::ExitCode;
use std::sync::Arc;

use job_base::Settings;
use job_receiver::Receiver;
use migration::{Migrator, MigratorTrait};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

use crate::events::Events;
use crate::order_run::standing::{Standing, stand};
use crate::server::{self, Server};
use crate::verbs::Built;

/// How many database connections the server holds open at most.
const CONNECTIONS: u32 = 8;

/// The server, until it is told to stop; what stopped it otherwise is said and ends it with a failure.
pub async fn serve(settings: &Settings) -> ExitCode {
  match served(settings).await {
    Ok(()) => ExitCode::SUCCESS,
    Err(failed) => {
      eprintln!("bmo-job: {failed}");
      ExitCode::FAILURE
    }
  }
}

async fn served(settings: &Settings) -> Result<(), Box<dyn std::error::Error>> {
  let standing = server::server_settings(settings)?.clone();
  let db = settings.database.url.connect(CONNECTIONS).await?;

  Migrator::up(&db, None).await?;

  let events = Events::new();
  let verbs = Built::of(db.clone(), settings, events.clone())?;

  stand(Standing {
    queue: verbs.ground.clone(),
    db: db.clone(),
    policy: verbs.policy.clone(),
    events: events.clone(),
  });

  let server = Arc::new(Server::on(db, settings, events, verbs)?);
  let listener = TcpListener::bind(standing.listen).await?;
  let holder = server::holder(standing.address.url().to_string());
  let unit = standing.unit.to_string();

  // Declared once before the routes are served; where bmo-receiver does not take it yet, again until it
  // does.
  if let Err(failed) = server.receiver.declare_holder(&unit, &holder).await {
    eprintln!("bmo-job: the room holdings are not declared yet: {failed}");
    tokio::spawn(server::declare_holder(server.receiver.clone(), unit, holder));
  }

  eprintln!("bmo-job: serving on {}", listener.local_addr()?);
  server::serve(listener, server, stopping()).await?;

  Ok(())
}

// The moment the process is told to stop: an interrupt, or the terminate its unit is stopped with.
async fn stopping() {
  let Ok(mut terminate) = signal(SignalKind::terminate()) else {
    let _ = tokio::signal::ctrl_c().await;
    return;
  };

  tokio::select! {
    _ = tokio::signal::ctrl_c() => {}
    _ = terminate.recv() => {}
  }
}
