//! A store of an older schema version brought up to the current one at start: from version 1, the question's
//! recommendation and the reply's source and reason are added, a reply already standing coming from a person;
//! from version 2, the item an item was raised from, filled from its `raised-by` label where that label is the
//! key of another standing item, and the caller who holds a lease; from version 3, whether a question's asker
//! tells of it and what it was handed; from version 6, what a lease holds of its item fixed; from version 7, who a
//! question is put to and the wait it was asked with (docs/schema.md).

use bmo_job::{Clock, Store};
use sqlx::Row;

/// The tables of schema version 1 the later versions change, as version 1 made them.
const ITEMS_AND_LEASES_V1: &str = "CREATE TABLE items (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    brief TEXT NOT NULL,
    state TEXT NOT NULL,
    queue_name TEXT NOT NULL,
    position INTEGER NOT NULL,
    priority INTEGER NOT NULL,
    assignee_name TEXT,
    requester_name TEXT,
    scope TEXT,
    labels TEXT NOT NULL,
    approvals TEXT NOT NULL,
    cowork INTEGER NOT NULL,
    merged_into INTEGER,
    lease_id INTEGER,
    created_at INTEGER NOT NULL,
    changed_at INTEGER NOT NULL,
    ended_at INTEGER
  );
  CREATE TABLE leases (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    queue_name TEXT NOT NULL,
    item_id INTEGER NOT NULL,
    holder_name TEXT NOT NULL,
    ttl_ms INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    ended_at INTEGER
  );";

const QUESTIONS_AND_REPLIES_V1: &str = "CREATE TABLE questions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    item_id INTEGER,
    asker_name TEXT NOT NULL,
    scope TEXT,
    text TEXT NOT NULL,
    choices TEXT NOT NULL,
    state TEXT NOT NULL,
    due_at INTEGER,
    remaining_ms INTEGER,
    due_told INTEGER NOT NULL,
    created_at INTEGER NOT NULL
  );
  CREATE TABLE replies (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    question_id INTEGER NOT NULL,
    choice TEXT,
    text TEXT NOT NULL,
    actor_name TEXT NOT NULL,
    settles INTEGER NOT NULL,
    created_at INTEGER NOT NULL
  );";

/// A store at `version` made by `schema`, opened by this build, and the pool reading it afterwards.
async fn migrated(directory: &tempfile::TempDir, schema: &str, version: i64) -> sqlx::SqlitePool {
  let path = directory.path().join("job.db");
  let url = format!("sqlite://{}?mode=rwc", path.display());
  let pool = sqlx::SqlitePool::connect(&url).await.expect("the store");
  sqlx::raw_sql(&format!("{schema} PRAGMA user_version = {version};"))
    .execute(&pool)
    .await
    .expect("the old schema");
  pool.close().await;

  drop(Store::open(&url, Clock::manual(0)).await.expect("the store migrates"));

  let pool = sqlx::SqlitePool::connect(&url).await.expect("the store");
  let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&pool).await.expect("the version");
  assert_eq!(version, 8);
  pool
}

async fn columns(pool: &sqlx::SqlitePool, table: &str) -> Vec<String> {
  sqlx::query(&format!("PRAGMA table_info({table})"))
    .fetch_all(pool)
    .await
    .expect("the columns")
    .iter()
    .map(|row| row.try_get("name").expect("a column"))
    .collect()
}

#[tokio::test]
async fn a_version_one_store_is_brought_to_the_questions_recommendation() {
  let directory = tempfile::tempdir().expect("a scratch directory");
  let schema = format!(
    "{ITEMS_AND_LEASES_V1} {QUESTIONS_AND_REPLIES_V1}
     INSERT INTO replies (question_id, text, actor_name, settles, created_at)
       VALUES (1, 'a word', 'tester', 0, 0);"
  );
  let pool = migrated(&directory, &schema, 1).await;
  let questions = columns(&pool, "questions").await;
  assert!(questions.iter().any(|column| column == "recommendation"), "{questions:?}");
  assert!(questions.iter().any(|column| column == "told_by_asker"), "{questions:?}");
  assert!(questions.iter().any(|column| column == "heard_through"), "{questions:?}");
  let source: String = sqlx::query_scalar("SELECT source FROM replies WHERE id = 1")
    .fetch_one(&pool)
    .await
    .expect("the reply");
  assert_eq!(source, "person");
  assert!(columns(&pool, "items").await.iter().any(|column| column == "raised_by"));
}

#[tokio::test]
async fn a_version_two_store_fills_raised_by_from_a_label_naming_a_standing_item() {
  let directory = tempfile::tempdir().expect("a scratch directory");
  let item = |id: i64, labels: &str| {
    format!(
      "INSERT INTO items (id, title, brief, state, queue_name, position, priority, labels, approvals, cowork, \
       created_at, changed_at) VALUES ({id}, 'item {id}', '', 'open', 'build', {id}, 0, '{labels}', '[]', 0, 0, 0);"
    )
  };
  let schema = [
    ITEMS_AND_LEASES_V1.to_owned(),
    QUESTIONS_AND_REPLIES_V1.to_owned(),
    item(1, "{}"),
    item(2, r#"{"raised-by":"1"}"#),
    item(3, r#"{"raised-by":"99"}"#),
    item(4, r#"{"raised-by":"the first"}"#),
    item(5, r#"{"raised-by":"5"}"#),
    item(6, r#"{"area":"parsing"}"#),
    "INSERT INTO leases (queue_name, item_id, holder_name, ttl_ms, expires_at, created_at) \
     VALUES ('build', 1, 'taker', 60000, 60000, 0);"
      .to_owned(),
  ]
  .join("\n");
  let pool = migrated(&directory, &schema, 2).await;

  let raised: Vec<(i64, Option<i64>)> = sqlx::query("SELECT id, raised_by FROM items ORDER BY id")
    .fetch_all(&pool)
    .await
    .expect("the items")
    .iter()
    .map(|row| (row.try_get("id").expect("a key"), row.try_get("raised_by").expect("a column")))
    .collect();
  assert_eq!(
    raised,
    [(1, None), (2, Some(1)), (3, None), (4, None), (5, None), (6, None)],
    "only a label naming another standing item fills it"
  );
  let holder: Option<String> = sqlx::query_scalar("SELECT caller_name FROM leases WHERE id = 1")
    .fetch_one(&pool)
    .await
    .expect("the lease");
  assert_eq!(holder, None, "a lease standing then is held by no named caller");
  let frozen: Option<String> = sqlx::query_scalar("SELECT frozen FROM leases WHERE id = 1")
    .fetch_one(&pool)
    .await
    .expect("the lease");
  assert_eq!(frozen, None, "a lease standing then holds nothing of its item fixed");
}

#[tokio::test]
async fn a_question_standing_before_the_addressee_is_a_persons_with_the_wait_it_was_asked_with() {
  let directory = tempfile::tempdir().expect("a scratch directory");
  let schema = format!(
    "{ITEMS_AND_LEASES_V1} {QUESTIONS_AND_REPLIES_V1}
     INSERT INTO questions (asker_name, text, choices, state, due_at, due_told, created_at)
       VALUES ('builder', 'timed', '[]', 'open', 10500, 0, 500);
     INSERT INTO questions (asker_name, text, choices, state, due_at, remaining_ms, due_told, created_at)
       VALUES ('builder', 'paused', '[]', 'paused', NULL, 4000, 0, 500);"
  );
  let pool = migrated(&directory, &schema, 1).await;

  let kept: Vec<(String, i64)> = sqlx::query("SELECT addressee, wait_ms FROM questions ORDER BY id")
    .fetch_all(&pool)
    .await
    .expect("the questions")
    .iter()
    .map(|row| (row.try_get("addressee").expect("a column"), row.try_get("wait_ms").expect("a column")))
    .collect();
  assert_eq!(kept, [("person".to_owned(), 10_000), ("person".to_owned(), 60_000)]);
}
