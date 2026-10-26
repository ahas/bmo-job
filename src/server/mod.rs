//! The HTTP server: every route of docs/api.md over the store, behind the bearer tokens, each request carrying
//! the caller its token names.

mod extract;
mod port;
mod routes;
mod stream;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::header::AUTHORIZATION;
use axum::middleware::{self, Next};
use axum::response::Response;
use bmo_job_client::Code;
use tokio::sync::watch;

use crate::access::Access;
use crate::refusal::Refusal;
use crate::settings::Unattended;
use crate::store::Store;

/// What every route stands on: the store, the tokens a request may carry and the callers they name, the labels
/// whose raised items open by themselves, and the signal that the service is stopping, on which every open stream
/// ends.
#[derive(Debug, Clone)]
pub struct App {
  pub store: Store,
  access: Arc<Access>,
  unattended: Arc<Unattended>,
  stopping: Arc<watch::Sender<bool>>,
}

impl App {
  pub fn new(store: Store, access: Access) -> Self {
    Self {
      store,
      access: Arc::new(access),
      unattended: Arc::new(Unattended::default()),
      stopping: Arc::new(watch::channel(false).0),
    }
  }

  /// The app whose raised items open by themselves where they carry one of `unattended`'s labels.
  pub fn unattended(self, unattended: Unattended) -> Self {
    Self {
      unattended: Arc::new(unattended),
      ..self
    }
  }

  /// Ends every open stream, so the server's graceful stop is not held by them.
  pub fn stop(&self) {
    self.stopping.send_replace(true);
  }

  fn stopping(&self) -> watch::Receiver<bool> {
    self.stopping.subscribe()
  }
}

/// The server's routes, each behind the tokens.
pub fn router(app: App) -> Router {
  routes::routes()
    .fallback(no_route)
    .method_not_allowed_fallback(no_route)
    .layer(middleware::from_fn_with_state(app.clone(), bearer))
    .with_state(app)
}

async fn no_route() -> Refusal {
  Refusal::new(Code::NoRoute, "No route answers this method and path.")
}

/// Lets a request through only with one of the service's tokens, carrying the caller that token names.
async fn bearer(State(app): State<App>, mut request: Request, next: Next) -> Result<Response, Refusal> {
  let given = request
    .headers()
    .get(AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "))
    .unwrap_or_default();
  let Some(caller) = app.access.caller(given).cloned() else {
    return Err(Refusal::new(Code::Unauthorized, "The bearer token is missing or wrong."));
  };
  request.extensions_mut().insert(caller);
  Ok(next.run(request).await)
}
