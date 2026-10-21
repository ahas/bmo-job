//! The HTTP server: every route of docs/api.md over the store, behind the bearer token.

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
use subtle::ConstantTimeEq;
use tokio::sync::watch;

use crate::refusal::Refusal;
use crate::store::Store;

/// What every route stands on: the store, the token every request carries, and the signal that the service
/// is stopping, on which every open stream ends.
#[derive(Debug, Clone)]
pub struct App {
  pub store: Store,
  token: Arc<str>,
  stopping: Arc<watch::Sender<bool>>,
}

impl App {
  pub fn new(store: Store, token: &str) -> Self {
    Self {
      store,
      token: Arc::from(token),
      stopping: Arc::new(watch::channel(false).0),
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

/// The server's routes, each behind the token.
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

/// Lets a request through only with the service's token.
async fn bearer(State(app): State<App>, request: Request, next: Next) -> Result<Response, Refusal> {
  let given = request
    .headers()
    .get(AUTHORIZATION)
    .and_then(|value| value.to_str().ok())
    .and_then(|value| value.strip_prefix("Bearer "))
    .unwrap_or_default();
  if !bool::from(given.as_bytes().ct_eq(app.token.as_bytes())) {
    return Err(Refusal::new(Code::Unauthorized, "The bearer token is missing or wrong."));
  }
  Ok(next.run(request).await)
}
