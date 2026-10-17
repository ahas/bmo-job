//! What is running (docs/queue.md, "What is running"): a unit stands where the machine's systemd user
//! manager holds it active or an open execution of bmo-agency names it as its parent or root — the union,
//! since asking systemd alone about a run in a pane reads it dead the moment it starts. From that, the
//! jobs at work and the workers they hold. "Nothing is running" and "nobody could say" are kept apart
//! throughout: a machine nobody could ask is never read as one with nothing running.

use std::collections::HashSet;
use std::path::PathBuf;
use std::process::Stdio;

use sea_orm::ConnectionTrait;
use serde_json::Value;

use super::jobs::{self, OpenJob};
use super::launches;
use super::turn::orders_in;
use super::units::Units;
use crate::Refusal;

/// The line of an execution that coordinates others, which holds nothing up once it has recorded its
/// outcome.
const COORDINATOR_LINE: &str = "coordinator";

/// What the queue reads of what runs: the database's open rows, bmo-agency's open executions, and the
/// machine's systemd through the `systemctl` program, where one is known.
pub struct Running<'a, C> {
  pub db: &'a C,
  pub units: &'a dyn Units,
  pub systemctl: Option<PathBuf>,
}

impl<C: ConnectionTrait> Running<'_, C> {
  /// Which of these units systemd holds active, asked of it alone in one call: it prints one line per
  /// unit in the order asked. None where it could not be asked or gave fewer lines than it was asked
  /// about.
  pub async fn systemd(&self, units: &[String]) -> Option<HashSet<String>> {
    let said = tokio::process::Command::new(self.systemctl.as_ref()?)
      .args(["--user", "is-active"])
      .args(units.iter().map(|unit| format!("{unit}.service")))
      .stdin(Stdio::null())
      .stderr(Stdio::null())
      .output()
      .await
      .ok()?;
    let lines: Vec<String> = String::from_utf8_lossy(&said.stdout)
      .lines()
      .map(str::trim)
      .filter(|line| !line.is_empty())
      .map(str::to_owned)
      .collect();

    (lines.len() >= units.len()).then(|| {
      units
        .iter()
        .zip(&lines)
        .filter(|(_, state)| *state == "active")
        .map(|(unit, _)| unit.clone())
        .collect()
    })
  }

  // systemd's answer with what else holds a run joined to it: an open execution naming one as its owner,
  // unless it shares its parent's pane, it coordinates and has recorded how it ended, or a cancel stands
  // over it.
  async fn joined(&self, units: &[String], mut alive: HashSet<String>) -> Result<HashSet<String>, Refusal> {
    for execution in self.units.open_executions().await? {
      if execution.shared_pane
        || execution.cancelled
        || (execution.line.as_deref() == Some(COORDINATOR_LINE) && execution.outcome.is_some())
      {
        continue;
      }

      for owner in [&execution.parent, &execution.root].into_iter().flatten() {
        if units.iter().any(|unit| unit == owner) {
          alive.insert(owner.clone());
        }
      }
    }

    Ok(alive)
  }

  /// Which of these units still stand, or none where systemd gave no answer: for a caller to whom
  /// "nothing stands" and "nobody could say" call for different things.
  pub async fn answered(&self, units: &[String]) -> Result<Option<HashSet<String>>, Refusal> {
    if units.is_empty() {
      return Ok(Some(HashSet::new()));
    }

    match self.systemd(units).await {
      Some(alive) => self.joined(units, alive).await.map(Some),
      None => Ok(None),
    }
  }

  /// Which of these units still stand, systemd giving no answer read as none of its own.
  pub async fn active(&self, units: &[String]) -> Result<HashSet<String>, Refusal> {
    if units.is_empty() {
      return Ok(HashSet::new());
    }

    let alive = self.systemd(units).await.unwrap_or_default();

    self.joined(units, alive).await
  }

  /// The jobs at work now, or none where nothing could say. A job of an item a person stopped is no work
  /// going on whatever its run is still doing on the way down; one stopped by its unit alone counts until
  /// its run is gone, so its item is not launched a second time beside it.
  pub async fn jobs(&self) -> Result<Option<Vec<OpenJob>>, Refusal> {
    let mut at_work = vec![];

    for job in jobs::unfinished(self.db).await? {
      if jobs::stop_asked(self.db, job.id).await? && self.nothing_left_in_line(job.todo_id).await? {
        continue;
      }

      at_work.push(job);
    }

    if at_work.is_empty() {
      return Ok(Some(at_work));
    }

    let units: Vec<String> = at_work.iter().map(|job| job.unit.clone()).collect();
    let Some(alive) = self.answered(&units).await? else {
      return Ok(None);
    };

    Ok(Some(at_work.into_iter().filter(|job| alive.contains(&job.unit)).collect()))
  }

  // Whether nothing of a stopped job waits in the line: it had no item, or its item has left the line.
  async fn nothing_left_in_line(&self, todo_id: Option<i64>) -> Result<bool, Refusal> {
    let Some(todo_id) = todo_id else {
      return Ok(true);
    };

    Ok(
      queue_rows::todos::item(self.db, todo_id)
        .await?
        .is_none_or(|item| item.done || item.held_at.is_some()),
    )
  }

  /// Every worker that has something of its own going on, named once per thing: a job's, a work order
  /// under way or one stopped still holding its checkout, and a checkout a running job has borrowed.
  pub async fn worker_names(&self, jobs: &[OpenJob]) -> Result<Vec<String>, Refusal> {
    let mut names: Vec<String> = jobs.iter().map(|job| job.worker.clone()).collect();

    for order in orders_in(self.db, &["active", "paused"]).await? {
      let active = order.get("status").and_then(Value::as_str) == Some("active");

      if active || order.get("holdsCheckout") == Some(&Value::Bool(true)) {
        names.push(order.get("worker").and_then(Value::as_str).unwrap_or_default().to_owned());
      }
    }

    let running: HashSet<i64> = jobs.iter().map(|job| job.id).collect();

    for lease in launches::leases(self.db).await? {
      if running.contains(&lease.job_id) {
        names.push(lease.worker);
      }
    }

    Ok(names)
  }

  /// The workers with something at work, or none where what is running could not be read.
  pub async fn workers(&self) -> Result<Option<Vec<String>>, Refusal> {
    match self.jobs().await? {
      Some(jobs) => self.worker_names(&jobs).await.map(Some),
      None => Ok(None),
    }
  }
}
