//! A store of schema version 1 brought to version 2: the question's recommendation and the reply's source
//! and reason are added, and a reply already standing came from a person.

use bmo_job::{Clock, Store};
use sqlx::Row;

#[tokio::test]
async fn a_version_one_store_is_brought_to_the_questions_recommendation() {
  let directory = tempfile::tempdir().expect("a scratch directory");
  let path = directory.path().join("job.db");
  let url = format!("sqlite://{}?mode=rwc", path.display());
  let pool = sqlx::SqlitePool::connect(&url).await.expect("the store");
  sqlx::raw_sql(
    "CREATE TABLE questions (
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
     );
     INSERT INTO replies (question_id, text, actor_name, settles, created_at)
       VALUES (1, 'a word', 'tester', 0, 0);
     PRAGMA user_version = 1;",
  )
  .execute(&pool)
  .await
  .expect("the old schema");
  pool.close().await;

  drop(Store::open(&url, Clock::manual(0)).await.expect("the store migrates"));

  let pool = sqlx::SqlitePool::connect(&url).await.expect("the store");
  let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&pool).await.expect("the version");
  assert_eq!(version, 2);
  let questions: Vec<String> = sqlx::query("PRAGMA table_info(questions)")
    .fetch_all(&pool)
    .await
    .expect("the columns")
    .iter()
    .map(|row| row.try_get("name").expect("a column"))
    .collect();
  assert!(questions.iter().any(|column| column == "recommendation"), "{questions:?}");
  let source: String = sqlx::query_scalar("SELECT source FROM replies WHERE id = 1")
    .fetch_one(&pool)
    .await
    .expect("the reply");
  assert_eq!(source, "person");
}
