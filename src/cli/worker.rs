//! `worker` (docs/cli.md, "`worker`"): the workers as the server keeps them, each verb one request to its
//! routes (docs/api.md, "Workers"), what it answers printed as its JSON.

use std::time::Duration;

use clap::{Args, Subcommand};
use serde_json::{Map, Value, json};

use crate::verbs::stand::{Answer, ServerRefused, Stand, VerbError};

// A change to a worker settles its bench and support rows before it answers.
const WORKER_WAIT: Duration = Duration::from_secs(60);

/// What `worker` takes.
#[derive(Subcommand, Debug)]
pub enum WorkerVerb {
  /// Every worker.
  List,
  /// One worker, by its name.
  Show { name: String },
  /// A worker added.
  Add {
    name: String,
    #[command(flatten)]
    fields: WorkerFields,
  },
  /// A worker changed.
  Update {
    name: String,
    #[command(flatten)]
    fields: WorkerFields,
    #[arg(long)]
    keepalive_stopped: Option<bool>,
    /// `auto`, a provider or `provider::model`.
    #[arg(long)]
    pinned_provider: Option<String>,
    #[arg(long)]
    support_count: Option<i64>,
    #[arg(long)]
    support_keepalive: Option<i64>,
  },
  /// A worker taken away.
  Remove { name: String },
  /// Where a worker's checkouts stand on the nodes.
  #[command(subcommand)]
  Node(NodeVerb),
}

/// What a worker is written with; an empty preamble or dev command clears it.
#[derive(Args, Debug, Default)]
pub struct WorkerFields {
  #[arg(long, allow_hyphen_values = true)]
  pub label: Option<String>,
  #[arg(long)]
  pub path: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub preamble: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub dev_command: Option<String>,
}

/// What `worker node` takes.
#[derive(Subcommand, Debug)]
pub enum NodeVerb {
  /// A base's checkout on a node added.
  AddCheckout {
    name: String,
    #[arg(long)]
    node: String,
    #[arg(long)]
    path: Option<String>,
  },
  /// A base's checkout on a node taken off.
  RemoveCheckout {
    name: String,
    #[arg(long)]
    node: String,
  },
  /// A worker of its own placed on a node, or brought back to this machine where no node is named.
  Place {
    name: String,
    #[arg(long)]
    node: Option<String>,
    #[arg(long)]
    path: Option<String>,
  },
}

fn refused(why: ServerRefused) -> VerbError {
  VerbError::Refused(format!("worker: {why}"))
}

// A request body: the fields given, each under its key, the ones left out absent.
fn body(pairs: Vec<(&str, Option<Value>)>) -> Value {
  Value::Object(
    pairs
      .into_iter()
      .filter_map(|(key, value)| value.map(|value| (key.to_owned(), value)))
      .collect::<Map<String, Value>>(),
  )
}

fn fields(name: String, action: &str, fields: WorkerFields) -> Vec<(&'static str, Option<Value>)> {
  vec![
    ("action", Some(json!(action))),
    ("name", Some(json!(name))),
    ("label", fields.label.map(Value::from)),
    ("path", fields.path.map(Value::from)),
    ("preamble", fields.preamble.map(Value::from)),
    ("dev_command", fields.dev_command.map(Value::from)),
  ]
}

// What the server answered, as lines: a list one member a line, anything else its JSON, nothing for none.
fn lines(answer: Value) -> Answer {
  match answer {
    Value::Null => Answer::default(),
    Value::Array(members) => members.iter().map(Value::to_string).collect::<Vec<_>>().into(),
    other => vec![other.to_string()].into(),
  }
}

/// One `worker` verb, asked of the server.
pub async fn worker(stand: &Stand, verb: WorkerVerb) -> Result<Answer, VerbError> {
  let (path, sent) = match verb {
    WorkerVerb::List => return stand.read_server("workers", WORKER_WAIT).await.map(lines).map_err(refused),
    WorkerVerb::Show { name } => {
      let path =
        format!("workers/{}", url::form_urlencoded::byte_serialize(name.as_bytes()).collect::<String>());

      return stand.read_server(&path, WORKER_WAIT).await.map(lines).map_err(refused);
    }
    WorkerVerb::Add {
      name,
      fields: given,
    } => ("worker", body(fields(name, "add", given))),
    WorkerVerb::Update {
      name,
      fields: given,
      keepalive_stopped,
      pinned_provider,
      support_count,
      support_keepalive,
    } => {
      let mut pairs = fields(name, "update", given);

      pairs.extend([
        ("keepalive_stopped", keepalive_stopped.map(Value::from)),
        ("pinned_provider", pinned_provider.map(Value::from)),
        ("support_count", support_count.map(Value::from)),
        ("support_keepalive", support_keepalive.map(Value::from)),
      ]);
      ("worker", body(pairs))
    }
    WorkerVerb::Remove { name } => ("worker", json!({ "action": "remove", "name": name })),
    WorkerVerb::Node(NodeVerb::AddCheckout { name, node, path }) => (
      "worker/node",
      body(vec![
        ("action", Some(json!("add_checkout"))),
        ("name", Some(json!(name))),
        ("node", Some(json!(node))),
        ("path", path.map(Value::from)),
      ]),
    ),
    WorkerVerb::Node(NodeVerb::RemoveCheckout { name, node }) => {
      ("worker/node", json!({ "action": "remove_checkout", "name": name, "node": node }))
    }
    WorkerVerb::Node(NodeVerb::Place { name, node, path }) => (
      "worker/node",
      body(vec![
        ("action", Some(json!("place"))),
        ("name", Some(json!(name))),
        ("node", node.map(Value::from)),
        ("path", path.map(Value::from)),
      ]),
    ),
  };

  stand.ask_server(path, &sent, WORKER_WAIT).await.map(lines).map_err(refused)
}
