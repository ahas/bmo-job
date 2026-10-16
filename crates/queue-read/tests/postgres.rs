//! The queue read off a migrated database on PostgreSQL, against the machine's own server in a scratch
//! schema of the test's own.

mod support;

#[tokio::test]
async fn the_queue_reads_a_database_on_postgresql() {
  support::in_postgres("queue", support::check).await;
}
