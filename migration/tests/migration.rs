//! The migrations take an empty database up to the tables bmo-job owns and back down again, on SQLite and
//! on PostgreSQL alike, and both backends end up with the same shape (docs/schema.md).

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use job_base::{Settings, all, run};
use migration::{Migrator, MigratorTrait};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Value};
use tempfile::TempDir;

const DEPLOYMENT: &str = r#"
[deployment]
owner_name = "Owner"
agent_name = "Agent"
agent_key = "agent"
sender_name = "Agent"
receiver_unit = "receiver"
"#;

/// The tables the migrations make.
const TABLES: [&str; 20] = [
  "branch_agents",
  "checkout_leases",
  "clock_holds",
  "job_questions",
  "job_spans",
  "jobs",
  "live_claude_openings",
  "live_claude_runs",
  "question_talk",
  "session_work_items",
  "session_work_results",
  "settings",
  "todo_approvals",
  "todo_notes",
  "todo_sources",
  "todo_talk",
  "todos",
  "work_order_locks",
  "work_orders",
  "workers",
];

/// The table the migrations are recorded in.
const RECORD: &str = "seaql_migrations";

/// The indexes the migrations make, besides those of the keys.
const INDEXES: [&str; 20] = [
  "job_questions_open",
  "job_spans_job",
  "job_spans_todo",
  "jobs_finished",
  "jobs_launched",
  "jobs_open",
  "jobs_report",
  "jobs_todo",
  "jobs_unit",
  "live_claude_openings_job",
  "live_claude_runs_opening",
  "question_talk_question",
  "todo_approvals_item",
  "todo_notes_item",
  "todo_sources_source",
  "todo_talk_item",
  "todos_merged_into",
  "todos_queue",
  "todos_repairs_todo_id",
  "work_orders_status",
];

const TRIGGERS: [&str; 3] = ["todos_merge_delete", "todos_merge_insert", "todos_merge_update"];

/// What a database's tables look like: each owned table's columns in order with whether they are
/// required, and the named indexes and triggers.
#[derive(Debug, PartialEq, Eq)]
struct Shape {
  columns: BTreeMap<String, Vec<(String, bool)>>,
  indexes: BTreeSet<String>,
  triggers: BTreeSet<String>,
}

async fn connect(url: &str) -> DatabaseConnection {
  let settings = Settings::read(&format!("[database]\nurl = \"{url}\"\n{DEPLOYMENT}")).unwrap();

  settings.database.url.connect(2).await.unwrap_or_else(|error| panic!("{url}: {error}"))
}

/// A database in a file of its own, gone with the directory.
async fn sqlite() -> (TempDir, DatabaseConnection) {
  let scratch = tempfile::tempdir().unwrap();
  let url = format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display());
  let db = connect(&url).await;

  (scratch, db)
}

/// The machine's own PostgreSQL server, as every PostgreSQL test of the service reaches it.
fn test_postgres() -> String {
  std::env::var("BMO_TEST_POSTGRES")
    .unwrap_or_else(|_| "postgres:///bmo_test?host=/var/run/postgresql".into())
}

/// Runs `check` on a database in a scratch schema of its own on the test PostgreSQL server, and drops
/// the schema whatever the check came to.
async fn in_postgres<F, Fut>(name: &str, check: F)
where
  F: FnOnce(DatabaseConnection) -> Fut,
  Fut: Future<Output = ()> + Send + 'static,
{
  let admin = connect(&test_postgres()).await;
  let seconds = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
  let schema = format!("t_{seconds}_{}_{name}", std::process::id());

  run(&admin, &format!("CREATE SCHEMA \"{schema}\""), vec![]).await.unwrap();

  let separator = if test_postgres().contains('?') { '&' } else { '?' };
  let db = connect(&format!("{}{separator}options[search_path]={schema}", test_postgres())).await;
  let checked = tokio::spawn(check(db)).await;

  run(&admin, &format!("DROP SCHEMA \"{schema}\" CASCADE"), vec![]).await.unwrap();

  if let Err(failure) = checked {
    std::panic::resume_unwind(failure.into_panic());
  }
}

/// The one-column answer of a catalog question, asked the way the database's backend reads it.
async fn names(db: &DatabaseConnection, sqlite: &str, postgres: &str) -> BTreeSet<String> {
  let sql = match db.get_database_backend() {
    DbBackend::Sqlite => sqlite,
    _ => postgres,
  };
  let rows = all(db, sql, vec![]).await.unwrap_or_else(|error| panic!("{sql}: {error}"));

  rows.iter().map(|row| row.try_get("", "name").unwrap()).collect()
}

async fn tables(db: &DatabaseConnection) -> BTreeSet<String> {
  names(
    db,
    "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'",
    "SELECT table_name::text AS name FROM information_schema.tables WHERE table_schema = current_schema()",
  )
  .await
}

/// The named indexes: SQLite's own key indexes have no text, and PostgreSQL's are named after the key.
async fn indexes(db: &DatabaseConnection) -> BTreeSet<String> {
  let found = names(
    db,
    "SELECT name FROM sqlite_master WHERE type = 'index' AND sql IS NOT NULL",
    "SELECT indexname::text AS name FROM pg_indexes WHERE schemaname = current_schema()",
  )
  .await;

  found
    .into_iter()
    .filter(|name| !name.ends_with("_pkey") && !name.ends_with("_rowid_key"))
    .collect()
}

async fn triggers(db: &DatabaseConnection) -> BTreeSet<String> {
  names(
    db,
    "SELECT name FROM sqlite_master WHERE type = 'trigger'",
    "SELECT DISTINCT trigger_name::text AS name FROM information_schema.triggers \
     WHERE trigger_schema = current_schema()",
  )
  .await
}

/// A table's columns in order, each with whether it is required. PostgreSQL's `rowid` column stands in
/// for SQLite's own row number, so it is not one of them.
async fn columns(db: &DatabaseConnection, table: &str) -> Vec<(String, bool)> {
  let sql = match db.get_database_backend() {
    DbBackend::Sqlite => {
      "SELECT name, (\"notnull\" OR pk > 0) AS required FROM pragma_table_info($1) ORDER BY cid"
    }
    _ => {
      "SELECT column_name::text AS name, (CASE WHEN is_nullable = 'YES' THEN 0 ELSE 1 END)::bigint AS required \
       FROM information_schema.columns \
       WHERE table_schema = current_schema() AND table_name = $1 AND column_name <> 'rowid' \
       ORDER BY ordinal_position"
    }
  };
  let rows = all(db, sql, vec![table.into()]).await.unwrap();

  rows
    .iter()
    .map(|row| (row.try_get("", "name").unwrap(), row.try_get::<i64>("", "required").unwrap() == 1))
    .collect()
}

async fn shape(db: &DatabaseConnection) -> Shape {
  let mut columns = BTreeMap::new();

  for table in TABLES {
    columns.insert(table.to_owned(), self::columns(db, table).await);
  }

  Shape {
    columns,
    indexes: indexes(db).await,
    triggers: triggers(db).await,
  }
}

fn set<const N: usize>(names: [&str; N]) -> BTreeSet<String> {
  names.into_iter().map(str::to_owned).collect()
}

async fn ok(db: &DatabaseConnection, sql: &str) {
  db.execute_unprepared(sql).await.unwrap_or_else(|error| panic!("{sql}: {error}"));
}

async fn refused(db: &DatabaseConnection, sql: &str) -> String {
  db.execute_unprepared(sql).await.expect_err(sql).to_string()
}

async fn numbers(db: &DatabaseConnection, sql: &str) -> Vec<i64> {
  let rows = all(db, sql, vec![]).await.unwrap_or_else(|error| panic!("{sql}: {error}"));

  rows.iter().map(|row| row.try_get("", "n").unwrap()).collect()
}

/// The rules every table keeps, checked by writing rows the way the service does.
async fn behaves(db: &DatabaseConnection) {
  let item = "INSERT INTO todos (title, instruction, requester, priority, status, created_at) \
              VALUES ('title', 'instruction', 'owner', 1, 'open', 1)";

  for _ in 0..3 {
    ok(db, item).await;
  }

  assert_eq!(numbers(db, "SELECT id AS n FROM todos ORDER BY id").await, [1, 2, 3]);
  assert_eq!(numbers(db, "SELECT rowid AS n FROM todos ORDER BY id").await, [1, 2, 3]);
  assert_eq!(numbers(db, "SELECT approved + done AS n FROM todos WHERE id = 1").await, [0]);

  let merge = |into: i64, item: i64| format!("UPDATE todos SET merged_into = {into} WHERE id = {item}");

  assert!(
    refused(db, &merge(1, 1))
      .await
      .contains("todo merge: an item is never merged into itself")
  );
  assert!(
    refused(db, &merge(9, 1))
      .await
      .contains("todo merge: the item merged into does not exist")
  );
  ok(db, &merge(1, 2)).await;
  assert!(
    refused(db, &merge(2, 3))
      .await
      .contains("todo merge: the item merged into is itself merged")
  );
  assert!(
    refused(db, &merge(3, 1))
      .await
      .contains("todo merge: other items are merged into this one")
  );
  assert!(
    refused(db, "DELETE FROM todos WHERE id = 1")
      .await
      .contains("todo merge: other items are merged into this one")
  );

  ok(db, "DELETE FROM todos WHERE id = 3").await;
  ok(db, item).await;
  assert_eq!(
    numbers(db, "SELECT id AS n FROM todos ORDER BY id").await,
    [1, 2, 4],
    "a number is never reused"
  );

  let merged_item = "INSERT INTO todos (title, instruction, requester, priority, status, created_at, merged_into) \
                     VALUES ('title', 'instruction', 'owner', 1, 'open', 1, 2)";

  assert!(
    refused(db, merged_item)
      .await
      .contains("todo merge: the item merged into is itself merged")
  );

  ok(
    db,
    "INSERT INTO job_spans (job_id, kind, name, seconds) VALUES (7, 'tool', 'first', 1.5), (7, 'tool', 'second', 0.25)",
  )
  .await;
  assert_eq!(numbers(db, "SELECT rowid AS n FROM job_spans ORDER BY rowid").await, [1, 2]);
  assert_eq!(numbers(db, "SELECT times AS n FROM job_spans ORDER BY rowid").await, [1, 1]);

  let total = all(db, "SELECT sum(seconds) AS total FROM job_spans", vec![]).await.unwrap();

  assert_eq!(total[0].try_get::<f64>("", "total").unwrap(), 1.75);

  ok(db, "INSERT INTO work_orders (todo_id, body) VALUES (4, '{\"status\":\"open\"}')").await;
  ok(db, "INSERT INTO workers (name, created_at) VALUES ('worker', 1)").await;
  assert_eq!(
    numbers(db, "SELECT builtin + support_count + support_keepalive AS n FROM workers").await,
    [0]
  );
  refused(db, "INSERT INTO workers (name) VALUES ('unborn')").await;

  let owned = [
    "queue_paused",
    "work_retry_model",
    "job_stop:5",
    "job_queue_waiting_told:12",
    "todo_repair_ceiling_told:7",
  ];
  let foreign = [
    "execution_cancel:5",
    "agent_gate:worker",
    "rest_until:worker",
    "brain_hold:1",
    "job_stop:",
    "JOB_STOP:5",
    "jobxstop:5",
    "queue_paused_too",
  ];

  for key in owned {
    run(
      db,
      "INSERT INTO settings (\"key\", value) VALUES ($1, $2)",
      vec![key.into(), Value::from("1")],
    )
    .await
    .unwrap_or_else(|error| panic!("{key}: {error}"));
  }

  for key in foreign {
    let written = run(
      db,
      "INSERT INTO settings (\"key\", value) VALUES ($1, $2)",
      vec![key.into(), Value::from("1")],
    );

    assert!(written.await.is_err(), "{key} is not a key of this service");
  }
}

/// Up, the rules, down to nothing but the record, and up again to the same shape.
async fn up_and_down(db: &DatabaseConnection) {
  let mut made = set(TABLES);

  made.insert(RECORD.to_owned());

  Migrator::up(db, None).await.expect("the migrations run up");
  assert_eq!(tables(db).await, made);

  let first = shape(db).await;

  assert_eq!(first.indexes, set(INDEXES));
  assert_eq!(first.triggers, set(TRIGGERS));
  assert!(first.columns.values().all(|columns| !columns.is_empty()));
  assert!(Migrator::get_pending_migrations(db).await.unwrap().is_empty());

  behaves(db).await;

  Migrator::down(db, None).await.expect("the migrations run down");
  assert_eq!(tables(db).await, set([RECORD]));
  assert!(indexes(db).await.is_empty());
  assert!(triggers(db).await.is_empty());

  Migrator::up(db, None).await.expect("the migrations run up again");
  assert_eq!(shape(db).await, first);
}

#[tokio::test]
async fn a_sqlite_database_goes_up_and_down() {
  let (_scratch, db) = sqlite().await;

  up_and_down(&db).await;
}

#[tokio::test]
async fn a_postgres_database_goes_up_and_down() {
  in_postgres("migration", |db| async move {
    up_and_down(&db).await;
    Migrator::down(&db, None).await.unwrap();

    let functions = all(
      &db,
      "SELECT p.proname::text AS name FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace \
       WHERE n.nspname = current_schema()",
      vec![],
    )
    .await
    .unwrap();

    assert!(functions.is_empty(), "down takes the merge functions with it");
  })
  .await;
}

#[tokio::test]
async fn both_backends_take_the_same_shape() {
  let (_scratch, lite) = sqlite().await;

  Migrator::up(&lite, None).await.unwrap();

  let expected = shape(&lite).await;

  in_postgres("shape", |db| async move {
    Migrator::up(&db, None).await.unwrap();
    assert_eq!(shape(&db).await, expected);
  })
  .await;
}
