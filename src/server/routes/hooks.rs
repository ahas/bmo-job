//! The hooks bmo-agency asks (docs/api.md, "Hooks bmo-agency asks"): it posts `point` and `ask` to the
//! route of each point the service registered, and reads the point's answer under `answer`.

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use job_codes::FieldForm;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::{Answered, State, body};
use crate::queue::jobs;
use crate::queue::worker_words::{self, Told};
use crate::server::Server;
use crate::server::request::{Body, wrong};
use crate::verbs::Verbs;

/// The path every point is asked under, its name standing last.
pub const HOOKS_PATH: &str = "/agency/hooks";

/// One point bmo-agency asks the service at: its name, the ask it takes, and its answer.
trait Point: Send + Sync + 'static {
  const NAME: &'static str;

  type Ask: DeserializeOwned + Send;

  fn answer<V: Verbs>(server: &Server<V>, ask: Self::Ask) -> impl Future<Output = Answered> + Send;
}

/// The items a job of which is open, which bmo-agency's item scratch sweep spares.
struct OpenItems;

impl Point for OpenItems {
  const NAME: &'static str = "open-items";

  type Ask = ();

  async fn answer<V: Verbs>(server: &Server<V>, _: ()) -> Answered {
    body(json!({ "answer": { "items": jobs::items_at_work(&server.db).await? } }))
  }
}

/// The text a worker is told the words the owner said to it in.
struct WorkerWords;

impl Point for WorkerWords {
  const NAME: &'static str = "worker-words";

  type Ask = Told;

  async fn answer<V: Verbs>(server: &Server<V>, told: Told) -> Answered {
    body(json!({ "answer": worker_words::told(&server.policy, &told) }))
  }
}

/// The names of the points the service answers, as it registers them at bmo-agency.
pub const POINTS: [&str; 2] = [OpenItems::NAME, WorkerWords::NAME];

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().merge(point::<V, OpenItems>()).merge(point::<V, WorkerWords>())
}

fn point<V: Verbs, P: Point>() -> Router<Arc<Server<V>>> {
  Router::new().route(&format!("{HOOKS_PATH}/{}", P::NAME), post(asked::<V, P>))
}

async fn asked<V: Verbs, P: Point>(state: State<V>, Body(sent): Body) -> Answered {
  let ask = sent.get("ask").cloned().unwrap_or(Value::Null);
  let ask = serde_json::from_value::<P::Ask>(ask).map_err(|_| wrong("ask", FieldForm::Object))?;

  P::answer(&state, ask).await
}
