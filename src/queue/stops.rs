//! A run asked to stop from outside (docs/queue.md, "Stopping runs"): its execution asked cancelled
//! through bmo-agency, which ends every run it covers by whatever reaches it, the unit stopped by systemd
//! where it still stands after that, and the runs a change to a work order ended, confirmed gone before
//! the rows they left are closed.

use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use job_codes::WorkOrderRefusal;
use sea_orm::ConnectionTrait;

use super::running::Running;
use super::units::Units;
use super::work_orders::{StoppedRun, refused};
use super::{cancels, jobs};
use crate::Refusal;

/// What a stop stands on: the database, the calls on other units, how what runs is read, and how long
/// the runs a cancel reached are given to see their own ending before systemd is asked.
pub struct Stops<'a, C> {
  pub db: &'a C,
  pub units: &'a dyn Units,
  pub systemctl: Option<PathBuf>,
  pub settle: Duration,
}

impl<'a, C: ConnectionTrait> Stops<'a, C> {
  fn running(&self) -> Running<'a, C> {
    Running {
      db: self.db,
      units: self.units,
      systemctl: self.systemctl.clone(),
    }
  }

  // Whether systemd holds the unit, asked of it alone.
  async fn unit_running(&self, unit: &str) -> bool {
    self
      .running()
      .systemd(&[unit.to_owned()])
      .await
      .is_some_and(|alive| alive.contains(unit))
  }

  /// One run asked to stop by its name, a unit's or an execution's: its execution and everything it
  /// covers asked cancelled, a while given for them to reach their own ending, and the unit stopped by
  /// systemd where it still stands. A failure of systemd's own stop is answered as the words it gave.
  pub async fn stop(&self, unit: &str) -> Result<Option<String>, Refusal> {
    self.units.cancel_execution(unit).await?;
    tokio::time::sleep(self.settle).await;

    if !self.unit_running(unit).await {
      return Ok(None);
    }

    let Some(systemctl) = &self.systemctl else {
      return Ok(Some(format!("{unit} still stands and no systemctl is known to stop it")));
    };
    let stopped = tokio::process::Command::new(systemctl)
      .args(["--user", "stop", &format!("{unit}.service")])
      .stdin(Stdio::null())
      .status()
      .await;

    Ok(match stopped {
      Ok(status) if status.success() => None,
      Ok(status) => {
        Some(format!("systemctl stopped {unit} with exit code {}", status.code().unwrap_or(-1)))
      }
      Err(why) => Some(format!("systemctl could not stop {unit}: {why}")),
    })
  }

  /// The runs a change to a work order ended, at `now`: every unit asked and confirmed gone first, so a
  /// slow stop cannot overlap the work replacing it; then each row they left closed with the ending named,
  /// each place in the pool given back, and each cancel taken back. Refused `runs_still_alive`, writing
  /// nothing, where a unit still stands.
  pub async fn stop_runs(
    &self,
    runs: &[StoppedRun],
    why: &str,
    outcome: &str,
    now: i64,
  ) -> Result<(), Refusal> {
    let units: Vec<String> = runs.iter().filter_map(|run| run.unit.clone()).collect();

    for unit in &units {
      self.stop(unit).await?;
    }

    let alive = self.running().active(&units).await?;

    if !alive.is_empty() {
      let mut named: Vec<String> = units.iter().filter(|unit| alive.contains(*unit)).cloned().collect();

      named.dedup();

      return Err(refused(WorkOrderRefusal::RunsStillAlive { alive: named }));
    }

    for run in runs {
      let job = match &run.unit {
        Some(unit) => jobs::of_unit(self.db, unit).await?,
        None => None,
      };
      let job_id = job.map(|(id, _)| id).or(run.job_id);
      let open = job.is_none_or(|(_, finished)| !finished);

      if let Some(job_id) = job_id
        && open
      {
        jobs::finish(self.db, job_id, why, false, outcome, now).await?;
      }

      if let Some(request) = &run.pool_request {
        self.units.finish_request(request, why, true).await?;
      }
    }

    // The stop is over: every unit is gone and every row it left is closed, so the cancel has nothing left
    // to refuse.
    for unit in &units {
      cancels::take_back(self.units, unit).await?;
    }

    Ok(())
  }
}
