//! The queue read off a migrated database on SQLite, in a file of the test's own.

mod support;

#[tokio::test]
async fn the_queue_reads_a_database_on_sqlite() {
  let (_scratch, db) = support::sqlite().await;

  support::check(db).await;
}
