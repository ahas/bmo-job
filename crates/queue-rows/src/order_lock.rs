//! A work order's lock, as `work_order_locks` holds it (docs/rows.md, "A work order's lock"): the work order
//! runner and an edit wait on the same row. Within this process the takers queue first, so only the head of
//! the queue asks the database.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use job_base::{Refusal, now_ms, one, run};
use sea_orm::DatabaseConnection;

// How long a taker waits before it asks the database again.
const POLL: Duration = Duration::from_millis(200);

// One queue per work order this process is taking or holding.
static QUEUES: LazyLock<Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>> = LazyLock::new(Default::default);
// The tokens of the holdings this process stands behind now, so a row of this process's own that none of
// them stands behind is read as left over.
static HELD: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
// The holdings this process has taken, which a token counts so no two of its own are alike.
static TAKEN: AtomicU64 = AtomicU64::new(0);

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Who holds a lock: the process, by its machine, its pid and the kernel's start time of that pid, and the
/// one holding by its token.
struct Holder {
  host: String,
  pid: i64,
  start: String,
  token: String,
}

/// The machine's name as the kernel gives it.
fn host() -> String {
  std::fs::read_to_string("/proc/sys/kernel/hostname")
    .map(|name| name.trim().to_owned())
    .unwrap_or_default()
}

/// The kernel's start time of a process, so a pid handed to a later process is not taken for the one that
/// held the lock.
fn start_of(pid: i64) -> Option<String> {
  let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
  let after = &stat[stat.rfind(')')? + 2..];

  after.split(' ').nth(19).map(str::to_owned)
}

// Whether the holder a row names still holds anything. A process of another machine is not this one's to
// judge and is waited on.
fn gone(holder: &Holder) -> bool {
  if holder.host != host() {
    return false;
  }

  if holder.pid == i64::from(std::process::id()) {
    return !locked(&HELD).contains(&holder.token);
  }

  start_of(holder.pid).as_deref() != Some(holder.start.as_str())
}

async fn holder(db: &DatabaseConnection, todo_id: i64) -> Result<Option<Holder>, Refusal> {
  let row = one(
    db,
    r#"SELECT "host", "pid", "start", "token" FROM "work_order_locks" WHERE "todo_id" = $1"#,
    vec![todo_id.into()],
  )
  .await?;

  row
    .map(|row| -> Result<Holder, Refusal> {
      Ok(Holder {
        host: row.try_get("", "host")?,
        pid: row.try_get("", "pid")?,
        start: row.try_get("", "start")?,
        token: row.try_get("", "token")?,
      })
    })
    .transpose()
}

async fn release(db: &DatabaseConnection, todo_id: i64, token: &str) -> Result<(), Refusal> {
  run(
    db,
    r#"DELETE FROM "work_order_locks" WHERE "todo_id" = $1 AND "token" = $2"#,
    vec![todo_id.into(), token.into()],
  )
  .await
  .map(drop)
}

async fn take(db: &DatabaseConnection, todo_id: i64, token: &str) -> Result<(), Refusal> {
  let pid = i64::from(std::process::id());
  let start = start_of(pid).ok_or_else(|| {
    Refusal::from(sea_orm::DbErr::Custom(
      "this process cannot read its own start time, so it cannot take a work order's lock".to_owned(),
    ))
  })?;
  let host = host();

  loop {
    let taken = run(
      db,
      r#"INSERT INTO "work_order_locks" ("todo_id", "host", "pid", "start", "token", "taken_at")
         VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING"#,
      vec![
        todo_id.into(),
        host.clone().into(),
        pid.into(),
        start.clone().into(),
        token.into(),
        now_ms().into(),
      ],
    )
    .await?;

    if taken > 0 {
      return Ok(());
    }

    match holder(db, todo_id).await? {
      Some(holder) if gone(&holder) => release(db, todo_id, &holder.token).await?,
      _ => tokio::time::sleep(POLL).await,
    }
  }
}

// A token no other holding has: this process, the moment, and how many it has taken.
fn token() -> String {
  format!("{:x}-{:x}-{:x}", std::process::id(), now_ms(), TAKEN.fetch_add(1, Ordering::Relaxed))
}

/// Runs `work` holding the work order's lock, waiting for as long as another holds it.
pub async fn with_order_lock<T, E, F>(db: &DatabaseConnection, todo_id: i64, work: F) -> Result<T, E>
where
  F: Future<Output = Result<T, E>>,
  E: From<Refusal>,
{
  let queue = locked(&QUEUES).entry(todo_id).or_default().clone();
  let turn = queue.lock().await;
  let token = token();

  locked(&HELD).insert(token.clone());

  let answer = match take(db, todo_id, &token).await {
    Ok(()) => {
      let answer = work.await;
      let released = release(db, todo_id, &token).await.map_err(E::from);

      answer.and_then(|answer| released.map(|()| answer))
    }
    Err(refusal) => Err(E::from(refusal)),
  };

  locked(&HELD).remove(&token);
  drop(turn);

  {
    let mut queues = locked(&QUEUES);

    // The last taker of an order leaves nothing behind it.
    if Arc::strong_count(&queue) == 2 {
      queues.remove(&todo_id);
    }
  }

  answer
}
