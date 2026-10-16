//! The one way a statement is bound and run on the service's database, SQLite or PostgreSQL alike, and
//! the pool a PostgreSQL connection is drawn from (docs/settings.md, "`[database]`").

use std::future::Future;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use sea_orm::{
  ConnectOptions, ConnectionTrait, Database, DatabaseConnection, DbBackend, DbErr, QueryResult, RuntimeErr,
  SqlxPostgresConnector, Statement, Value,
};
use sqlx::ConnectOptions as _;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};

use crate::Refusal;

/// One statement with its values, for a database of `backend`, every number at its widest.
pub fn statement(backend: DbBackend, sql: &str, values: Vec<Value>) -> Statement {
  Statement::from_sql_and_values(backend, sql, values.into_iter().map(widest))
}

// A number at the one width each kind is bound at. PostgreSQL's driver prepares a text once per
// connection with the types its first values had, and a value of another width bound to it later is
// refused.
fn widest(value: Value) -> Value {
  match value {
    Value::TinyInt(n) => Value::BigInt(n.map(i64::from)),
    Value::SmallInt(n) => Value::BigInt(n.map(i64::from)),
    Value::Int(n) => Value::BigInt(n.map(i64::from)),
    Value::TinyUnsigned(n) => Value::BigInt(n.map(i64::from)),
    Value::SmallUnsigned(n) => Value::BigInt(n.map(i64::from)),
    Value::Unsigned(n) => Value::BigInt(n.map(i64::from)),
    Value::Float(n) => Value::Double(n.map(f64::from)),
    other => other,
  }
}

/// The one row a statement answers, or none.
pub async fn one<C: ConnectionTrait>(
  db: &C,
  sql: &str,
  values: Vec<Value>,
) -> Result<Option<QueryResult>, Refusal> {
  let statement = statement(db.get_database_backend(), sql, values);

  Ok(replanned(|| db.query_one(statement.clone())).await?)
}

/// Every row a statement answers.
pub async fn all<C: ConnectionTrait>(
  db: &C,
  sql: &str,
  values: Vec<Value>,
) -> Result<Vec<QueryResult>, Refusal> {
  let statement = statement(db.get_database_backend(), sql, values);

  Ok(replanned(|| db.query_all(statement.clone())).await?)
}

/// A statement run for what it writes.
pub async fn run<C: ConnectionTrait>(db: &C, sql: &str, values: Vec<Value>) -> Result<u64, Refusal> {
  let statement = statement(db.get_database_backend(), sql, values);

  Ok(replanned(|| db.execute(statement.clone())).await?.rows_affected())
}

// PostgreSQL's code for a prepared statement whose table moved under it: "cached plan must not
// change result type" once a column is added to what a `SELECT *` reads.
const PLAN_OUTLIVED: &str = "0A000";

// When this process last met a statement prepared on the tables' shape before it moved.
static SHAPE_MOVED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// Whether an error is a statement prepared before the tables' shape moved under it.
pub fn plan_outlived(error: &DbErr) -> bool {
  match error {
    DbErr::Query(RuntimeErr::SqlxError(sqlx::Error::Database(fault)))
    | DbErr::Exec(RuntimeErr::SqlxError(sqlx::Error::Database(fault))) => {
      fault.code().is_some_and(|code| code == PLAN_OUTLIVED)
    }
    _ => false,
  }
}

/// Marks every connection this process opened until now as prepared on the shape before: each is
/// closed as it is next handed out, and its prepared statements with it.
pub fn shape_moved() {
  *SHAPE_MOVED_AT.lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(Instant::now());
}

// Whether a connection of this age was opened before the last mark.
fn opened_before_move(age: Duration) -> bool {
  SHAPE_MOVED_AT
    .lock()
    .unwrap_or_else(|poisoned| poisoned.into_inner())
    .is_some_and(|at| at.elapsed() <= age)
}

/// A PostgreSQL pool of at most `max` connections, which never hands out a connection opened before
/// the tables' shape last moved under this process.
pub fn pool(max: u32) -> PgPoolOptions {
  PgPoolOptions::new()
    .max_connections(max)
    .before_acquire(|_, meta| Box::pin(async move { Ok(!opened_before_move(meta.age)) }))
}

/// The database at `address`, of `backend`, with at most `connections` connections open.
pub(crate) async fn connect(
  backend: DbBackend,
  address: &str,
  connections: u32,
) -> Result<DatabaseConnection, DbErr> {
  match backend {
    DbBackend::Postgres => {
      let options = PgConnectOptions::from_str(address).map_err(sqlx_fault)?.disable_statement_logging();
      let pool = pool(connections).connect_with(options).await.map_err(sqlx_fault)?;

      Ok(SqlxPostgresConnector::from_sqlx_postgres_pool(pool))
    }
    _ => {
      let mut options = ConnectOptions::new(address);

      options.max_connections(connections).sqlx_logging(false);
      Database::connect(options).await
    }
  }
}

fn sqlx_fault(error: sqlx::Error) -> DbErr {
  DbErr::Conn(RuntimeErr::SqlxError(error))
}

/// Work on the database run once more where it met a statement prepared before the tables' shape
/// moved, after marking the shape moved, so the second run stands on a connection that prepares it
/// afresh. A second run that fails too answers the first error: inside a transaction it can only meet
/// the transaction already aborted.
pub async fn replanned<T, F, Fut>(mut work: F) -> Result<T, DbErr>
where
  F: FnMut() -> Fut,
  Fut: Future<Output = Result<T, DbErr>>,
{
  match work().await {
    Err(error) if plan_outlived(&error) => {
      shape_moved();
      work().await.map_err(|_| error)
    }
    answer => answer,
  }
}
