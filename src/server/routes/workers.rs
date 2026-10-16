//! Workers (docs/api.md, "Workers"): added, changed and removed through the workers' verb, placed on the
//! nodes through it, and read as `workers` keeps them.

use std::sync::Arc;

use axum::Router;
use axum::extract::Path;
use axum::routing::{get, post};
use job_base::Refusal;
use job_codes::{AgentRefusal, ControlRefusal, FieldForm};
use sea_orm::QueryResult;
use serde::Serialize;
use serde_json::Value;

use super::{Answered, State, body, nothing};
use crate::server::Server;
use crate::server::request::{Body, Fields, refused, wrong};
use crate::verbs::{NodeChange, Verbs, WorkerAdd, WorkerUpdate};
use crate::{all, one};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new()
    .route("/worker", post(change::<V>))
    .route("/worker/node", post(node::<V>))
    .route("/workers", get(list::<V>))
    .route("/workers/{name}", get(read::<V>))
}

/// A worker as `workers` keeps it.
#[derive(Serialize)]
struct Worker {
  name: String,
  label: Option<String>,
  cwd: Option<String>,
  icon: Option<String>,
  builtin: bool,
  preamble: Option<String>,
  dev_command: Option<String>,
  support_count: i64,
  support_keepalive: i64,
  support_of: Option<String>,
  support_slot: Option<i64>,
  node: Option<String>,
  role: Option<String>,
  resting_since: Option<i64>,
  created_at: i64,
}

const COLUMNS: &str = r#""name", "label", "cwd", "icon", "builtin", "preamble", "dev_command", "support_count",
  "support_keepalive", "support_of", "support_slot", "node", "role", "resting_since", "created_at""#;

impl Worker {
  fn of(row: &QueryResult) -> Result<Self, Refusal> {
    Ok(Self {
      name: row.try_get("", "name")?,
      label: row.try_get("", "label")?,
      cwd: row.try_get("", "cwd")?,
      icon: row.try_get("", "icon")?,
      builtin: row.try_get::<i64>("", "builtin")? == 1,
      preamble: row.try_get("", "preamble")?,
      dev_command: row.try_get("", "dev_command")?,
      support_count: row.try_get("", "support_count")?,
      support_keepalive: row.try_get("", "support_keepalive")?,
      support_of: row.try_get("", "support_of")?,
      support_slot: row.try_get("", "support_slot")?,
      node: row.try_get("", "node")?,
      role: row.try_get("", "role")?,
      resting_since: row.try_get("", "resting_since")?,
      created_at: row.try_get("", "created_at")?,
    })
  }
}

async fn list<V: Verbs>(state: State<V>) -> Answered {
  let rows = all(&state.db, &format!(r#"SELECT {COLUMNS} FROM "workers" ORDER BY "name""#), vec![]).await?;

  body(rows.iter().map(Worker::of).collect::<Result<Vec<_>, _>>()?)
}

async fn read<V: Verbs>(state: State<V>, Path(name): Path<String>) -> Answered {
  let row = one(
    &state.db,
    &format!(r#"SELECT {COLUMNS} FROM "workers" WHERE "name" = $1"#),
    vec![name.clone().into()],
  )
  .await?
  .ok_or_else(|| refused(AgentRefusal::NoWorker { worker: name }))?;

  body(Worker::of(&row)?)
}

// One of the two workbench texts as the row keeps it: a string emptied is cleared, and anything that is not
// a string is the text left alone.
fn bench_text(asked: &Value, name: &str) -> Option<Option<String>> {
  let text = asked.get(name)?.as_str()?.trim();

  Some((!text.is_empty()).then(|| text.to_owned()))
}

fn update(asked: &Value) -> Result<WorkerUpdate, Refusal> {
  let fields = Fields(asked);

  Ok(WorkerUpdate {
    label: fields.words_or_none("label")?,
    path: fields.words_or_none("path")?,
    preamble: bench_text(asked, "preamble"),
    dev_command: bench_text(asked, "dev_command"),
    downgrade_heal: fields.boolean_or_none("downgrade_heal")?,
    keepalive_stopped: fields.boolean_or_none("keepalive_stopped")?,
    pinned_provider: fields.words_or_none("pinned_provider")?,
    support_count: fields.whole_or_none("support_count")?,
    support_keepalive: fields.whole_or_none("support_keepalive")?,
  })
}

async fn change<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  let name = fields.words("name")?;

  match fields.text_or_none("action").ok().flatten().as_deref() {
    Some("add") => body(
      state
        .verbs
        .add_worker(WorkerAdd {
          name,
          label: fields.words_or_none("label")?,
          path: fields.words_or_none("path")?,
          preamble: bench_text(&asked, "preamble").flatten(),
          dev_command: bench_text(&asked, "dev_command").flatten(),
        })
        .await?,
    ),
    Some("update") => {
      state.verbs.update_worker(name, update(&asked)?).await?;
      nothing()
    }
    Some("remove") => {
      state.verbs.remove_worker(name).await?;
      nothing()
    }
    _ => Err(refused(ControlRefusal::ActionWrong)),
  }
}

async fn node<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let fields = Fields(&asked);
  let name = fields.words("name")?;
  let node = fields.words_or_none("node")?;
  let path = fields.words_or_none("path")?;
  let needs_node = || node.clone().ok_or_else(|| wrong("node", FieldForm::Words));
  let change = match fields.text_or_none("action").ok().flatten().as_deref() {
    Some("add_checkout") => NodeChange::AddCheckout {
      name,
      node: needs_node()?,
      path,
    },
    Some("remove_checkout") => NodeChange::RemoveCheckout {
      name,
      node: needs_node()?,
    },
    Some("place") => NodeChange::Place { name, node, path },
    _ => return Err(refused(ControlRefusal::ActionWrong)),
  };

  match state.verbs.change_node(change).await? {
    Some(checkout) => body(checkout),
    None => nothing(),
  }
}
