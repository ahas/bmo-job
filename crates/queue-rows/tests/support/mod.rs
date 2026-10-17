//! What the rows' tests stand on: a migrated database of the test's own, on SQLite in a scratch directory or
//! on the test PostgreSQL server in a scratch schema; the policy as a deployment named Owner and Agent reads
//! it; and hands and units that record every call and answer from what the test set.

// Each test file is its own crate and uses only part of this module.
#![allow(dead_code)]

use std::collections::HashMap;
use std::future::Future;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::future::BoxFuture;
use job_agency::TurnRecord;
use job_base::{Refusal, Settings, run};
use job_brain::{Memory, MemoryAdd};
use job_codes::QueueTold;
use job_policy::{Names, Policy};
use job_receiver::{Instruction, Message};
use migration::{Migrator, MigratorTrait};
use queue_rows::closing::Reader;
use queue_rows::wording::BannedTerm;
use queue_rows::{Filer, Ground, Hands, Units, WordsAdded, Written};
use sea_orm::DatabaseConnection;
use tempfile::TempDir;

/// The owner as a sender, and the owner's private chat.
pub const OWNER: i64 = 100;

/// The moment every test stands at, in epoch milliseconds: 2026-09-20 11:09 UTC.
pub const NOW: i64 = 1_789_902_540_000;

const DEPLOYMENT: &str = r#"
[deployment]
owner_name = "Owner"
agent_name = "Agent"
agent_key = "agent"
sender_name = "Agent"
receiver_unit = "receiver"
owner_chat = 100
"#;

fn settings(url: &str) -> Settings {
  Settings::read(&format!("[database]\nurl = \"{url}\"\n{DEPLOYMENT}")).expect("the test settings read")
}

async fn connect(url: &str) -> DatabaseConnection {
  settings(url)
    .database
    .url
    .connect(2)
    .await
    .unwrap_or_else(|error| panic!("{url}: {error}"))
}

/// The policy as the deployment reads it.
pub fn policy() -> Policy {
  Policy::load(&settings("sqlite::memory:")).expect("the policy loads")
}

/// The deployment's names.
pub fn names() -> Names {
  Names::of(&settings("sqlite::memory:").deployment)
}

/// The closing's patterns as the deployment builds them.
pub fn reader() -> Reader {
  Reader::new(&policy().closing, &names())
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().expect("a test's record is not poisoned")
}

/// Hands that record each call: the queue moved, the queue told, words added and a stop.
#[derive(Default)]
pub struct Recorded {
  pub moved: Mutex<Vec<(Option<i64>, Option<Written>)>>,
  pub told: Mutex<Vec<String>>,
  pub words: Mutex<Vec<String>>,
}

impl Recorded {
  pub fn moved(&self) -> Vec<(Option<i64>, Option<Written>)> {
    locked(&self.moved).clone()
  }

  pub fn told(&self) -> Vec<String> {
    locked(&self.told).clone()
  }

  pub fn words(&self) -> Vec<String> {
    locked(&self.words).clone()
  }
}

impl Hands for Recorded {
  fn moved(&self, work_order: Option<i64>, written: Option<Written>) -> BoxFuture<'_, ()> {
    locked(&self.moved).push((work_order, written));

    Box::pin(async {})
  }

  fn queue_standing<'a>(
    &'a self,
    moment: &'a str,
    repeat: bool,
  ) -> BoxFuture<'a, Result<QueueTold, Refusal>> {
    locked(&self.told).push(format!("{moment} {repeat}"));

    Box::pin(async { Ok(QueueTold::Told) })
  }

  fn words_added<'a>(&'a self, added: &'a WordsAdded<'a>) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.words).push(serde_json::to_string(&added.fact()).expect("a fact writes"));

    Box::pin(async { Ok(()) })
  }

  fn stop<'a>(&'a self, _: i64, _: &'a str) -> BoxFuture<'a, Result<u32, Refusal>> {
    Box::pin(async { Ok(0) })
  }
}

/// Units that keep what each call hands them and answer from what the test set: the messages of the rooms,
/// and the memories filed.
#[derive(Default)]
pub struct Kept {
  pub messages: Mutex<HashMap<(i64, i64), Message>>,
  pub memories: Mutex<Vec<Memory>>,
  pub instructions: Mutex<Vec<Instruction>>,
  pub records: Mutex<Vec<TurnRecord>>,
}

impl Kept {
  /// A message said in a room.
  pub fn said(&self, chat: i64, id: i64, sender: Option<i64>, name: &str, text: &str) {
    let message = Message {
      sender,
      sender_name: Some(name.to_owned()),
      sent_at: NOW - 60_000,
      text: text.to_owned(),
    };

    locked(&self.messages).insert((chat, id), message);
  }

  pub fn instructions(&self) -> Vec<Instruction> {
    locked(&self.instructions).clone()
  }

  pub fn records(&self) -> Vec<TurnRecord> {
    locked(&self.records).clone()
  }

  pub fn memories(&self) -> Vec<Memory> {
    locked(&self.memories).clone()
  }
}

impl Units for Kept {
  fn queue_instruction<'a>(&'a self, instruction: &'a Instruction) -> BoxFuture<'a, Result<i64, Refusal>> {
    let mut kept = locked(&self.instructions);

    kept.push(instruction.clone());

    let id = i64::try_from(kept.len()).expect("a count fits");

    Box::pin(async move { Ok(id) })
  }

  fn message(&self, chat: i64, message: i64) -> BoxFuture<'_, Result<Option<Message>, Refusal>> {
    let found = locked(&self.messages).get(&(chat, message)).cloned();

    Box::pin(async move { Ok(found) })
  }

  fn memories(&self, chat: i64) -> BoxFuture<'_, Result<Vec<Memory>, Refusal>> {
    let found: Vec<Memory> = locked(&self.memories)
      .iter()
      .filter(|memory| memory.chat_id == Some(chat))
      .cloned()
      .collect();

    Box::pin(async move { Ok(found) })
  }

  fn add_memory<'a>(&'a self, memory: &'a MemoryAdd) -> BoxFuture<'a, Result<(), Refusal>> {
    let mut kept = locked(&self.memories);
    let id = i64::try_from(kept.len()).expect("a count fits") + 1;

    kept.push(Memory {
      id,
      slug: format!("memory-{id}"),
      kind: memory.kind.clone(),
      chat_id: memory.chat,
      action: memory.action.clone(),
      todo_id: memory.todo,
      active: true,
      set_at: NOW / 1000,
      done_at: None,
    });

    Box::pin(async { Ok(()) })
  }

  fn keep_turn_record<'a>(&'a self, record: &'a TurnRecord) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.records).push(record.clone());

    Box::pin(async { Ok(()) })
  }
}

/// Everything a ground borrows, on a database of its own.
pub struct Scene {
  _scratch: Option<TempDir>,
  pub db: DatabaseConnection,
  pub names: Names,
  pub policy: Policy,
  pub reader: Reader,
  pub hands: Recorded,
  pub units: Kept,
  pub banned: Vec<BannedTerm>,
  pub providers: Vec<String>,
}

impl Scene {
  /// A scene on a migrated SQLite database in a scratch directory, gone with the scene.
  pub async fn new() -> Self {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let db = connect(&format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display())).await;

    Migrator::up(&db, None).await.expect("the migrations run up");

    Self::on(db, Some(scratch))
  }

  /// A scene on a database already migrated.
  pub fn on(db: DatabaseConnection, scratch: Option<TempDir>) -> Self {
    Self {
      _scratch: scratch,
      db,
      names: names(),
      policy: policy(),
      reader: reader(),
      hands: Recorded::default(),
      units: Kept::default(),
      banned: vec![],
      providers: vec!["claude-code".to_owned(), "codex".to_owned()],
    }
  }

  /// The ground of a person at a shell.
  pub fn ground(&self) -> Ground<'_> {
    Ground {
      db: &self.db,
      owner_sender: OWNER,
      owner_chat: OWNER,
      names: &self.names,
      agent_key: "agent",
      policy: &self.policy,
      reader: &self.reader,
      banned: &self.banned,
      providers: &self.providers,
      in_job: false,
      run_name: None,
      filer: Filer::default(),
      person: Some("person"),
      hands: &self.hands,
      units: &self.units,
      now: NOW,
    }
  }

  /// The ground of a job working under `run`.
  pub fn job(&self, run: &'static str) -> Ground<'_> {
    Ground {
      in_job: true,
      run_name: Some(run),
      person: None,
      ..self.ground()
    }
  }

  /// The ground of the chat agent's own turn.
  pub fn chat(&self) -> Ground<'_> {
    Ground {
      filer: Filer {
        agent: Some("agent"),
        label: None,
      },
      person: None,
      ..self.ground()
    }
  }

  /// One item, waiting in the owner's room, with what it was filed with.
  pub async fn item(&self, id: i64, title: &str) {
    self
      .run(
        r#"INSERT INTO "todos" ("id", "title", "instruction", "requester", "chat_id", "approved", "priority", "status", "created_at")
           VALUES ($1, $2, 'What to do.', 'Owner', $3, 1, $1, 'waiting', $4)"#,
        vec![id.into(), title.into(), OWNER.into(), NOW.into()],
      )
      .await;
  }

  /// A statement run on the scene's database.
  pub async fn run(&self, sql: &str, values: Vec<sea_orm::Value>) {
    run(&self.db, sql, values).await.unwrap_or_else(|failed| panic!("{sql}: {failed}"));
  }

  /// One column of an item as it now stands.
  pub async fn column<T: sea_orm::TryGetable>(&self, id: i64, column: &str) -> T {
    let row = job_base::one(
      &self.db,
      &format!(r#"SELECT "{column}" FROM "todos" WHERE "id" = $1"#),
      vec![id.into()],
    )
    .await
    .expect("the item reads")
    .expect("the item stands");

    row.try_get("", column).expect("the column reads")
  }
}

/// The machine's own PostgreSQL server, as every PostgreSQL test of the service reaches it.
fn test_postgres() -> String {
  std::env::var("BMO_TEST_POSTGRES")
    .unwrap_or_else(|_| "postgres:///bmo_test?host=/var/run/postgresql".into())
}

/// Runs `check` on a migrated database in a scratch schema of its own on the test PostgreSQL server, and
/// drops the schema whatever the check came to.
pub async fn in_postgres<F, Fut>(name: &str, check: F)
where
  F: FnOnce(DatabaseConnection) -> Fut + Send + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  let admin = connect(&test_postgres()).await;
  let seconds = SystemTime::now().duration_since(UNIX_EPOCH).expect("the clock reads").as_secs();
  let schema = format!("t_{seconds}_{}_{name}", std::process::id());

  run(&admin, &format!("CREATE SCHEMA \"{schema}\""), vec![])
    .await
    .expect("the schema is made");

  let separator = if test_postgres().contains('?') { '&' } else { '?' };
  let db = connect(&format!("{}{separator}options[search_path]={schema}", test_postgres())).await;
  let checked = tokio::spawn(async move {
    Migrator::up(&db, None).await.expect("the migrations run up");
    check(db).await;
  })
  .await;

  run(&admin, &format!("DROP SCHEMA \"{schema}\" CASCADE"), vec![])
    .await
    .expect("the schema is dropped");

  if let Err(failure) = checked {
    std::panic::resume_unwind(failure.into_panic());
  }
}
