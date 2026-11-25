//! What the service holds, in one SQLite database (docs/schema.md), and every rule of docs/api.md that moves
//! it. Each change runs in one transaction with the events it tells, and wakes every reader waiting on a
//! change once it is committed.

mod events;
mod items;
mod label_values;
mod leases;
mod notes;
mod questions;
mod queues;
mod reports;
mod reviews;
mod scopes;
mod subscriptions;
mod sweep;
mod work_orders;

use std::str::FromStr;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Sqlite, SqlitePool, Transaction};
use tokio::sync::watch;

use crate::clock::Clock;
use crate::refusal::Refusal;

pub use events::{EVENT_WINDOW_MS, EventFilter, EventKeys};
pub use questions::{LEAST_WAIT_MS, LONGEST_WAIT_MS, QUESTION_WAIT_MS};
pub use subscriptions::{Delivery, Kept};

/// One transaction on the database.
pub(crate) type Tx = Transaction<'static, Sqlite>;

/// The 1.0.0 baseline's version, which `schema.sql` makes whole (docs/schema.md). An install from before 1.0.0 at
/// its last release holds this version already.
const BASELINE_VERSION: i64 = 9;

/// The steps that bring a database from the baseline up to the current version, each from the version it reads.
const STEPS: [(i64, &str); 1] = [(9, include_str!("migrate_v9.sql"))];

/// The schema's version, kept in the database's own user version: the baseline's raised by each step.
const SCHEMA_VERSION: i64 = BASELINE_VERSION + STEPS.len() as i64;

/// The name the service writes its own changes under: the notes it writes and the changes its sweep makes.
pub(crate) const OWN_NAME: &str = "bmo-job";

/// A page's size where the caller names none, and the most it may name.
const DEFAULT_LIMIT: i64 = 50;
const MOST_LIMIT: i64 = 500;

/// The longest a caller may ask an answer to be held open.
pub const MOST_WAIT_MS: i64 = 60_000;

/// How long a change waits for the database: for the one connection, and for a lock another process holds. The
/// library takes a span, never "until it comes", and SQLite's busy handler counts it in an `int` of milliseconds,
/// so this is the longest the library can be told: a change waits for the holder to let go.
const LOCK_WAIT: Duration = Duration::from_millis(i32::MAX as u64);

/// The database, the clock it is written by, the signal of each committed change, and the deployment's question
/// wait (docs/settings.md, `[questions]`).
#[derive(Debug, Clone)]
pub struct Store {
  pool: SqlitePool,
  clock: Clock,
  changes: watch::Sender<u64>,
  question_wait_ms: i64,
}

impl Store {
  /// The database at `url`, its tables made where it has none and the steps it lacks taken where it stands at the
  /// baseline or past it.
  pub async fn open(url: &str, clock: Clock) -> Result<Self, Refusal> {
    let options = SqliteConnectOptions::from_str(url)?
      .create_if_missing(true)
      .journal_mode(SqliteJournalMode::Wal)
      .busy_timeout(LOCK_WAIT);
    // One connection: every change runs alone, so a lease raced for by several holders goes to one.
    let pool = SqlitePoolOptions::new()
      .max_connections(1)
      .acquire_timeout(LOCK_WAIT)
      .connect_with(options)
      .await?;
    let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&pool).await?;
    if version == 0 || (BASELINE_VERSION..SCHEMA_VERSION).contains(&version) {
      let mut tx = pool.begin().await?;
      let from = if version == 0 {
        sqlx::raw_sql(include_str!("schema.sql")).execute(&mut *tx).await?;
        BASELINE_VERSION
      } else {
        version
      };
      for (_, step) in STEPS.iter().filter(|(reads, _)| *reads >= from) {
        sqlx::raw_sql(step).execute(&mut *tx).await?;
      }
      sqlx::raw_sql(&format!("PRAGMA user_version = {SCHEMA_VERSION}"))
        .execute(&mut *tx)
        .await?;
      tx.commit().await?;
    } else if version < BASELINE_VERSION {
      return Err(Refusal::new(
        bmo_job_client::Code::Failed,
        format!(
          "The database's schema is version {version}, from a release before 1.0.0 short of its last; that \
           release brings it to version {BASELINE_VERSION} first."
        ),
      ));
    } else if version != SCHEMA_VERSION {
      return Err(Refusal::new(
        bmo_job_client::Code::Failed,
        format!(
          "The database's schema is version {version}, and this build reads version {SCHEMA_VERSION}."
        ),
      ));
    }
    let (changes, _) = watch::channel(0);
    Ok(Self {
      pool,
      clock,
      changes,
      question_wait_ms: QUESTION_WAIT_MS,
    })
  }

  /// The store whose questions wait `wait_ms` on whoever holds them: the supervisor before the service puts one to
  /// a person, and a person at the most before its recommendation settles it; a minute where this is not called.
  #[must_use]
  pub fn with_question_wait(mut self, wait_ms: i64) -> Self {
    self.question_wait_ms = wait_ms;
    self
  }

  pub fn now(&self) -> i64 {
    self.clock.now()
  }

  /// A signal that moves each time a change is committed.
  pub fn changes(&self) -> watch::Receiver<u64> {
    self.changes.subscribe()
  }

  async fn begin(&self) -> Result<Tx, Refusal> {
    Ok(self.pool.begin().await?)
  }

  /// Commits a change and wakes every reader waiting on one.
  async fn commit(&self, tx: Tx) -> Result<(), Refusal> {
    tx.commit().await?;
    self.changes.send_modify(|count| *count += 1);
    Ok(())
  }
}

/// A page's size from what the caller asked.
pub(crate) fn page_limit(limit: Option<i64>) -> Result<i64, Refusal> {
  match limit {
    None => Ok(DEFAULT_LIMIT),
    Some(limit) if limit < 1 => Err(Refusal::bad_request("The limit is not a positive number.")),
    Some(limit) => Ok(limit.min(MOST_LIMIT)),
  }
}

/// How long to hold an answer open, from what the caller asked.
pub fn wait_span(wait_ms: Option<i64>) -> Result<Duration, Refusal> {
  match wait_ms {
    None => Ok(Duration::ZERO),
    Some(ms) if ms < 0 => Err(Refusal::bad_request("The waitMs is negative.")),
    Some(ms) => Ok(Duration::from_millis(ms.min(MOST_WAIT_MS) as u64)),
  }
}

/// A list's page from rows read one past the limit.
pub(crate) fn paged<T>(mut rows: Vec<T>, limit: i64) -> bmo_job_client::Page<T> {
  let more = rows.len() as i64 > limit;
  rows.truncate(limit as usize);
  bmo_job_client::Page { items: rows, more }
}

pub(crate) fn to_json<T: Serialize>(value: &T) -> String {
  serde_json::to_string(value).expect("a stored value renders as JSON")
}

pub(crate) fn from_json<T: DeserializeOwned>(text: &str) -> Result<T, Refusal> {
  Ok(serde_json::from_str(text)?)
}

/// A stored word read back as the member it names.
pub(crate) fn stored<T>(word: &str, read: impl Fn(&str) -> Option<T>) -> Result<T, Refusal> {
  read(word).ok_or_else(|| {
    Refusal::new(
      bmo_job_client::Code::Failed,
      format!("A stored word {word:?} is not one this build reads."),
    )
  })
}

/// A required text that may not be empty.
pub(crate) fn filled(field: &str, value: &str) -> Result<(), Refusal> {
  if value.trim().is_empty() {
    return Err(Refusal::bad_request(format!("The {field} is empty.")));
  }
  Ok(())
}

/// A queue's name: 1 to 64 letters, digits, dots, underscores and hyphens.
pub(crate) fn queue_name(name: &str) -> Result<(), Refusal> {
  let fits = (1..=64).contains(&name.len())
    && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
  if !fits {
    return Err(Refusal::bad_request(format!(
      "The queue name {name:?} is not 1 to 64 letters, digits, dots, underscores and hyphens."
    )));
  }
  Ok(())
}

/// The values of a comma-separated filter, each read by `read`.
pub(crate) fn listed<T>(
  field: &str,
  text: Option<&str>,
  read: impl Fn(&str) -> Option<T>,
) -> Result<Vec<T>, Refusal> {
  let Some(text) = text else {
    return Ok(Vec::new());
  };
  text
    .split(',')
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .map(|part| {
      read(part).ok_or_else(|| Refusal::bad_request(format!("The {field} {part:?} is not one it takes.")))
    })
    .collect()
}
