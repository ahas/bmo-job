//! The service's server (docs/api.md, "Routes"): the settings read, the database migrated up, the routes
//! served on `[server] listen` over the service's verbs until the process is told to stop, the work-order
//! runner started beside them, and the service declared to bmo-receiver as a holder of rooms' rows.

use std::process::ExitCode;
use std::sync::Arc;

use bmo_job::events::Events;
use bmo_job::order_run::standing::{Standing, stand};
use bmo_job::server::{self, Server};
use bmo_job::verbs::Built;
use job_base::Settings;
use job_receiver::Receiver;
use migration::{Migrator, MigratorTrait};
use tokio::net::TcpListener;
use tokio::signal::unix::{SignalKind, signal};

/// How many database connections the server holds open at most.
const CONNECTIONS: u32 = 8;

#[tokio::main]
async fn main() -> ExitCode {
  match run().await {
    Ok(()) => ExitCode::SUCCESS,
    Err(failed) => {
      eprintln!("bmo-job: {failed}");
      ExitCode::FAILURE
    }
  }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
  let settings = Settings::load(&Settings::place())?;
  let standing = server::server_settings(&settings)?.clone();
  let db = settings.database.url.connect(CONNECTIONS).await?;

  Migrator::up(&db, None).await?;

  let events = Events::new();
  let verbs = Built::of(db.clone(), &settings, events.clone())?;

  stand(Standing {
    queue: verbs.ground.clone(),
    db: db.clone(),
    policy: verbs.policy.clone(),
    events: events.clone(),
  });

  let server = Arc::new(Server::on(db, &settings, events, verbs)?);
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
