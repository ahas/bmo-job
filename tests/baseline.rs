//! The schema's starting point (docs/schema.md): a database with no tables takes the 1.0.0 baseline whole, an
//! install from before 1.0.0 at its last release is read as it stands, and one of an older version is refused.

use bmo_job::{Clock, Store};

const SCHEMA: &str = include_str!("../src/store/schema.sql");

// Every object of the database as SQLite keeps it, and its user version.
async fn shape(pool: &sqlx::SqlitePool) -> (i64, Vec<(String, String)>) {
  let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(pool).await.expect("the version");
  let objects: Vec<(String, String)> = sqlx::query_as(
    "SELECT name, COALESCE(sql, '') FROM sqlite_master WHERE name NOT LIKE 'sqlite_%' ORDER BY type, name",
  )
  .fetch_all(pool)
  .await
  .expect("the objects");
  (version, objects)
}

// A database at `version` holding the baseline's schema and an item, as an install left it.
async fn installed(directory: &tempfile::TempDir, version: i64) -> String {
  let path = directory.path().join("job.db");
  let url = format!("sqlite://{}?mode=rwc", path.display());
  let pool = sqlx::SqlitePool::connect(&url).await.expect("the store");
  sqlx::raw_sql(&format!(
    "{SCHEMA} INSERT INTO items (title, brief, state, queue_name, position, priority, labels, approvals, \
     created_at, changed_at) VALUES ('An item', 'Its brief', 'todo', 'todo', 1, 0, '{{}}', '[]', 1, 1); \
     PRAGMA user_version = {version};"
  ))
  .execute(&pool)
  .await
  .expect("the install");
  pool.close().await;
  url
}

#[tokio::test]
async fn a_database_with_no_tables_takes_the_baseline_whole() {
  let directory = tempfile::tempdir().expect("a directory");
  let url = format!("sqlite://{}?mode=rwc", directory.path().join("job.db").display());
  drop(Store::open(&url, Clock::manual(0)).await.expect("the store opens"));
  let opened = sqlx::SqlitePool::connect(&url).await.expect("the store");

  let laid_directory = tempfile::tempdir().expect("a directory");
  let laid_url = format!("sqlite://{}?mode=rwc", laid_directory.path().join("job.db").display());
  let laid = sqlx::SqlitePool::connect(&laid_url).await.expect("the store");
  sqlx::raw_sql(&format!("{SCHEMA} PRAGMA user_version = 9;"))
    .execute(&laid)
    .await
    .expect("the schema");

  assert_eq!(shape(&opened).await, shape(&laid).await);
  assert_eq!(shape(&opened).await.0, 9);
}

#[tokio::test]
async fn an_install_at_the_last_release_before_the_baseline_is_read_as_it_stands() {
  let directory = tempfile::tempdir().expect("a directory");
  let url = installed(&directory, 9).await;
  let pool = sqlx::SqlitePool::connect(&url).await.expect("the store");
  let before = shape(&pool).await;
  pool.close().await;

  drop(Store::open(&url, Clock::manual(0)).await.expect("the store opens"));
  let pool = sqlx::SqlitePool::connect(&url).await.expect("the store");
  assert_eq!(shape(&pool).await, before);
  let titles: Vec<String> =
    sqlx::query_scalar("SELECT title FROM items").fetch_all(&pool).await.expect("the items");
  assert_eq!(titles, ["An item"]);
}

#[tokio::test]
async fn an_install_short_of_the_last_release_before_the_baseline_is_refused() {
  let directory = tempfile::tempdir().expect("a directory");
  let url = installed(&directory, 8).await;

  let refused = Store::open(&url, Clock::manual(0)).await.expect_err("an older version is refused");
  assert!(format!("{refused:?}").contains("version 8"), "{refused:?}");
  let pool = sqlx::SqlitePool::connect(&url).await.expect("the store");
  assert_eq!(shape(&pool).await.0, 8);
}
