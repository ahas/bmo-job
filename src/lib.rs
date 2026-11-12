//! The job service: a todo kept as middleware (docs/api.md). It stores items, their notes, reports and work
//! orders, queues, leases, questions and subscriptions, serves them over HTTP, tells every change as an event,
//! and delivers the events its subscribers asked for. The service never executes and calls no other unit; a verb
//! of its command that puts words before a person reaches the receiver from its own process (docs/cli.md).

pub mod access;
pub mod again;
pub mod clock;
pub mod close_watch;
pub mod delivery;
pub mod import;
pub mod job_ask;
pub mod job_close;
pub mod outside;
pub mod refusal;
pub mod server;
pub mod settings;
pub mod store;
pub mod todo_cli;

use std::time::Duration;

use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub use access::{Access, Caller};
pub use clock::Clock;
pub use delivery::DeliveryPolicy;
pub use server::{App, router};
pub use settings::{Settings, Unattended};
pub use store::Store;

/// How often the service looks for what has come due.
const SWEEP_EVERY: Duration = Duration::from_secs(1);

/// The service's own work beside its routes: the sweep of what comes due, and webhook delivery.
pub fn spawn_background(store: Store, policy: DeliveryPolicy) -> Vec<JoinHandle<()>> {
  let sweeping = store.clone();
  let sweep = tokio::spawn(async move {
    let mut every = tokio::time::interval(SWEEP_EVERY);
    loop {
      every.tick().await;
      if let Err(refusal) = sweeping.sweep().await {
        eprintln!("bmo-job: the sweep failed: {}", refusal.sentence);
      }
    }
  });
  let delivery = tokio::spawn(delivery::deliver(store, policy));
  vec![sweep, delivery]
}

/// Serves the routes on `listener` until `stop` resolves.
pub async fn serve_on(
  listener: TcpListener,
  app: App,
  stop: impl Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
  axum::serve(listener, router(app)).with_graceful_shutdown(stop).await
}

/// Serves the service by its settings until it is told to stop.
pub async fn serve(settings: Settings) -> Result<(), String> {
  let access = settings.access()?;
  let store = Store::open(&settings.database.url, Clock::System)
    .await
    .map_err(|refusal| refusal.sentence)?;
  let listener = TcpListener::bind(&settings.server.listen)
    .await
    .map_err(|error| format!("{} could not be listened on: {error}", settings.server.listen))?;
  let background = spawn_background(store.clone(), DeliveryPolicy::default());
  let app = App::new(store, access).unattended(settings.unattended.clone());
  let stopping = app.clone();
  let served = serve_on(listener, app, async move {
    stopped().await;
    stopping.stop();
  })
  .await;
  background.iter().for_each(JoinHandle::abort);
  served.map_err(|error| format!("the server failed: {error}"))
}

/// Resolves when the process is told to stop, by an interrupt or a termination signal.
async fn stopped() {
  let interrupt = tokio::signal::ctrl_c();
  let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
    .expect("the termination signal is heard");
  tokio::select! {
    _ = interrupt => {}
    _ = terminate.recv() => {}
  }
}
