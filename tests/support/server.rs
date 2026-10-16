//! The server as its request tests run it: on a loopback port over a scratch SQLite database the
//! migrations have run on, beside the stand-ins of the units it calls, and built on verbs that record each
//! call and answer it from a set answer.

use std::sync::{Arc, Mutex, MutexGuard};

use bmo_job::server::{self, Server};
use bmo_job::verbs::{
  Board, Done, JobEnded, Moment, NodeChange, NodeCheckout, OrderAction, PoolBoard, Questions, RunStops,
  RunnerLaunch, Runners, Settling, Stop, TaskLaunch, Tasks, Telling, TodoAction, TodoChanged, Todos,
  WorkOrders, WorkerAdd, WorkerAdded, WorkerUpdate, WorkerWord, WorkerWords, Workers,
};
use job_base::{Refusal, Settings, run};
use job_codes::job_questions::QuestionClock;
use job_codes::run_stop::{StopState, TargetStop};
use job_codes::{QuestionAnswer, QueueTold, RunStopAnswer, TaskAnswer};
use migration::{Migrator, MigratorTrait};
use sea_orm::{DatabaseConnection, Value as Bound};
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use url::Url;

use super::{agency, gate, receiver};

/// One call the verbs took: the verb and what it was handed, as Rust writes it out.
pub type Took = (&'static str, String);

#[derive(Default)]
struct Book {
  took: Vec<Took>,
  refusal: Option<Refusal>,
}

/// Verbs that record each call and answer it with a set answer, or with the refusal set for the next call.
#[derive(Clone, Default)]
pub struct Fake(Arc<Mutex<Book>>);

impl Fake {
  fn book(&self) -> MutexGuard<'_, Book> {
    self.0.lock().expect("the book is not poisoned")
  }

  /// Every call taken so far, in order.
  pub fn took(&self) -> Vec<Took> {
    self.book().took.clone()
  }

  /// The next call refused with `refusal`.
  pub fn refuse_next(&self, refusal: Refusal) {
    self.book().refusal = Some(refusal);
  }

  fn call<T>(&self, verb: &'static str, handed: String, answer: T) -> Done<T> {
    let mut book = self.book();

    book.took.push((verb, handed));

    match book.refusal.take() {
      Some(refusal) => Err(refusal),
      None => Ok(answer),
    }
  }
}

impl Telling for Fake {
  async fn job_ended(&self, ended: JobEnded) -> Done<QueueTold> {
    self.call("job_ended", format!("{ended:?}"), QueueTold::Told)
  }

  async fn deliver_report(&self, job: i64) -> Done<()> {
    self.call("deliver_report", job.to_string(), ())
  }

  async fn tell_queue(&self, moment: Moment) -> Done<QueueTold> {
    self.call("tell_queue", format!("{moment:?}"), QueueTold::NothingWaiting)
  }
}

impl Runners for Fake {
  async fn launch_runner(&self, launch: RunnerLaunch) -> Done<()> {
    self.call("launch_runner", format!("{launch:?}"), ())
  }
}

impl PoolBoard for Fake {
  async fn pool_board(&self) -> Done<Board> {
    let board = Board {
      read_at: 7,
      buckets: vec![],
      workers: vec![],
    };

    self.call("pool_board", String::new(), board)
  }
}

impl Questions for Fake {
  async fn answer_question(&self, id: i64, settling: Settling) -> Done<QuestionAnswer> {
    self.call("answer_question", format!("{id} {settling:?}"), QuestionAnswer::QuestionSettled)
  }

  async fn pause_question(&self, id: i64, paused: bool) -> Done<QuestionAnswer> {
    let clock = QuestionClock {
      paused_left: paused.then_some(60_000),
      due_at: (!paused).then_some(90_000),
    };
    let answer = if paused {
      QuestionAnswer::QuestionPaused(clock)
    } else {
      QuestionAnswer::QuestionResumed(clock)
    };

    self.call("pause_question", format!("{id} {paused}"), answer)
  }
}

impl RunStops for Fake {
  async fn stop_run(&self, stop: Stop) -> Done<RunStopAnswer> {
    let answer = RunStopAnswer::StopAsked {
      target: stop.target.to_string(),
      runs: 1,
    };

    self.call("stop_run", format!("{stop:?}"), answer)
  }

  async fn stop_all(&self) -> Done<RunStopAnswer> {
    let answer = RunStopAnswer::AllAsked {
      asked: 1,
      confirmed: 0,
      targets: vec![TargetStop {
        target: "unit:job-3".into(),
        state: StopState::Stopping,
      }],
    };

    self.call("stop_all", String::new(), answer)
  }
}

impl Tasks for Fake {
  async fn launch_task(&self, launch: TaskLaunch) -> Done<TaskAnswer> {
    let answer = TaskAnswer::JobLaunched {
      job: 41,
      unit: "job-41".into(),
      worker: launch.worker.clone().unwrap_or_else(|| "worker".into()),
      cwd: "/work".into(),
      chat: launch.chat,
      notes: vec![],
    };

    self.call("launch_task", format!("{launch:?}"), answer)
  }
}

impl Todos for Fake {
  async fn change_todo(&self, id: i64, action: TodoAction) -> Done<TodoChanged> {
    let changed = TodoChanged {
      moved: true,
      stopped: matches!(action, TodoAction::Stop { .. }).then_some(2),
      raised: matches!(action, TodoAction::Raise { .. }).then_some(99),
    };

    self.call("change_todo", format!("{id} {action:?}"), changed)
  }
}

impl WorkOrders for Fake {
  async fn change_order(&self, id: i64, action: OrderAction) -> Done<Value> {
    self.call("change_order", format!("{id} {action:?}"), json!({ "todoId": id, "status": "active" }))
  }
}

impl Workers for Fake {
  async fn add_worker(&self, add: WorkerAdd) -> Done<WorkerAdded> {
    let added = WorkerAdded {
      name: add.name.clone(),
      label: add.label.clone().unwrap_or_else(|| add.name.clone()),
      cwd: add.path.clone().unwrap_or_else(|| "/homes/new".into()),
    };

    self.call("add_worker", format!("{add:?}"), added)
  }

  async fn update_worker(&self, name: String, update: WorkerUpdate) -> Done<()> {
    self.call("update_worker", format!("{name} {update:?}"), ())
  }

  async fn remove_worker(&self, name: String) -> Done<()> {
    self.call("remove_worker", name, ())
  }

  async fn change_node(&self, change: NodeChange) -> Done<Option<NodeCheckout>> {
    let checkout = match &change {
      NodeChange::Place { .. } => None,
      NodeChange::AddCheckout { name, node, .. } | NodeChange::RemoveCheckout { name, node } => {
        Some(NodeCheckout {
          worker: format!("{name}-{node}"),
          cwd: format!("/nodes/{node}/{name}"),
        })
      }
    };

    self.call("change_node", format!("{change:?}"), checkout)
  }
}

impl WorkerWords for Fake {
  async fn say_to_worker(&self, word: WorkerWord) -> Done<()> {
    let files: Vec<(String, usize)> =
      word.files.iter().map(|file| (file.name.clone(), file.bytes.len())).collect();

    self.call("say_to_worker", format!("{:?} {:?} {files:?}", word.worker, word.text), ())
  }
}

/// The settings of a server over the database at `url`, calling the stand-ins at their addresses.
pub fn settings(url: &str, agency: &Url, gate: &Url, receiver: &Url) -> Settings {
  Settings::read(&settings_text(url, agency, gate, receiver)).expect("the test settings read")
}

/// The same settings as the file a deployment gives.
pub fn settings_text(url: &str, agency: &Url, gate: &Url, receiver: &Url) -> String {
  format!(
    r#"[database]
url = "{url}"

[deployment]
owner_name = "Owner"
agent_name = "Agent"
agent_key = "agent"
sender_name = "Agent"
receiver_unit = "receiver"

[server]
listen = "127.0.0.1:0"
address = "http://127.0.0.1:1/"
unit = "job"

[units]
agency = "{agency}"
gate = "{gate}"
receiver = "{receiver}"
"#
  )
}

/// A server running on loopback, stopped when it is dropped.
pub struct Served {
  pub base: Url,
  pub http: reqwest::Client,
  pub server: Arc<Server<Fake>>,
  pub verbs: Fake,
  pub agency: agency::Stub,
  pub gate: gate::Stub,
  pub receiver: receiver::Stub,
  stop: Option<oneshot::Sender<()>>,
  serving: JoinHandle<std::io::Result<()>>,
  _scratch: TempDir,
}

impl Served {
  /// A server over a fresh scratch database.
  pub async fn start() -> Self {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let url = format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display());
    let (agency, gate, receiver) =
      (agency::Stub::start().await, gate::Stub::start().await, receiver::Stub::start().await);
    let settings = settings(&url, &agency.address(), &gate.address(), &receiver.address());
    let db = settings.database.url.connect(2).await.expect("the scratch database opens");

    Migrator::up(&db, None).await.expect("the migrations run up");

    let verbs = Fake::default();
    let server = Arc::new(Server::new(db, &settings, verbs.clone()).expect("the server is made"));
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port is free");
    let base = Url::parse(&format!("http://{}/", listener.local_addr().expect("the port is bound")))
      .expect("the address reads");
    let (stop, stopping) = oneshot::channel::<()>();
    let serving = tokio::spawn(server::serve(listener, server.clone(), async move {
      let _ = stopping.await;
    }));

    Self {
      base,
      http: reqwest::Client::new(),
      server,
      verbs,
      agency,
      gate,
      receiver,
      stop: Some(stop),
      serving,
      _scratch: scratch,
    }
  }

  pub fn db(&self) -> &DatabaseConnection {
    &self.server.db
  }

  /// A statement run on the server's database, as a test seeds it.
  pub async fn seed(&self, sql: &str, values: Vec<Bound>) {
    run(self.db(), sql, values).await.unwrap_or_else(|failed| panic!("{sql}: {failed}"));
  }

  pub fn url(&self, path: &str) -> Url {
    self.base.join(path.trim_start_matches('/')).expect("a path joins the base")
  }

  /// A request answered: its status and its body read as JSON, null where it has none.
  pub async fn answered(&self, request: reqwest::RequestBuilder) -> (u16, Value) {
    let response = request.send().await.expect("the server answers");
    let status = response.status().as_u16();
    let bytes = response.bytes().await.expect("the answer's body reads");
    let body = if bytes.is_empty() {
      Value::Null
    } else {
      serde_json::from_slice(&bytes).expect("the answer is JSON")
    };

    (status, body)
  }

  pub async fn get(&self, path: &str) -> (u16, Value) {
    self.answered(self.http.get(self.url(path))).await
  }

  pub async fn post(&self, path: &str, body: Value) -> (u16, Value) {
    self.answered(self.http.post(self.url(path)).json(&body)).await
  }

  pub async fn put(&self, path: &str, body: Value) -> (u16, Value) {
    self.answered(self.http.put(self.url(path)).json(&body)).await
  }

  pub async fn delete(&self, path: &str) -> (u16, Value) {
    self.answered(self.http.delete(self.url(path))).await
  }
}

impl Drop for Served {
  fn drop(&mut self) {
    if let Some(stop) = self.stop.take() {
      let _ = stop.send(());
    }

    self.serving.abort();
  }
}

/// The body of a refusal: its code, as the API answers it beside its sentence.
pub fn code(body: &Value) -> &str {
  assert_eq!(
    body.as_object().map(|object| object.len()),
    Some(2),
    "a refusal is its error and its code: {body}"
  );
  assert!(
    body["error"].as_str().is_some_and(|error| !error.is_empty()),
    "a refusal carries a sentence: {body}"
  );

  body["code"].as_str().expect("a refusal carries a code")
}
