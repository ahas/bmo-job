//! The two other ways an open job stops being work (docs/queue.md, "The sweep"): circling — started over
//! past the most a run may be, inside a process that never exits — and standing still, five minutes with
//! nothing written. Both are only told: what to do about them is the agent's. A job that marked a long wait
//! is judged by its mark first.

use std::collections::HashSet;
use std::path::Path;
use std::process::Stdio;

use job_codes::job_wait::WaitTarget;
use job_codes::queue_facts::{LastLine, WaitMark, WaitStanding};
use job_codes::{Fact, QueueFact};
use serde::{Deserialize, Serialize};

use super::Teller;
use super::dead::open_jobs;
use crate::Refusal;
use crate::queue::jobs::JOB_STOP_PREFIX;
use crate::queue::launches::WORK_ORDER_STARTED_BY;
use crate::queue::settings;

/// How many starts a run may make before it is circling.
pub const MAX_STARTS: u64 = 12;

/// How long a running job may do nothing at all before the agent is told so.
pub const JOB_QUIET_TELL_MS: i64 = 5 * 60_000;

/// The settings key a job's wait mark is kept under, followed by the job's number.
pub const WAIT_PREFIX: &str = "job_wait:";

fn spin_told_key(job_id: i64) -> String {
  format!("job_spin_told:{job_id}")
}

fn quiet_told_key(job_id: i64) -> String {
  format!("job_quiet_told:{job_id}")
}

/// A job's own mark on a long wait, as its settings row holds it; its times are epoch milliseconds.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobWait {
  pub what: String,
  pub target: WaitTarget,
  pub since: i64,
  pub until: i64,
  pub ended_at: Option<i64>,
}

impl JobWait {
  /// The mark out of its row, or none where the row is not one or names a target that cannot be asked.
  pub fn read(text: &str) -> Option<Self> {
    let read: Self = serde_json::from_str(text).ok()?;
    let askable = match &read.target {
      WaitTarget::Unit { name } => !name.is_empty(),
      WaitTarget::Pid { pid } => *pid > 0,
    };

    askable.then_some(read)
  }

  /// Where the mark stands at `now`.
  pub fn standing(&self, now: i64) -> WaitStanding {
    if self.ended_at.is_some() {
      WaitStanding::Ended
    } else if now >= self.until {
      WaitStanding::Overdue
    } else {
      WaitStanding::Waiting
    }
  }
}

/// Whether a wait's target lives: a process the machine still has, or a unit systemd calls alive; none
/// where systemd gave no answer or none is known to ask.
pub fn probe_target(target: &WaitTarget, systemctl: Option<&Path>) -> Option<bool> {
  match target {
    WaitTarget::Pid { pid } => Some(Path::new("/proc").join(pid.to_string()).exists()),
    WaitTarget::Unit { name } => {
      let typed = name.rsplit_once('.').is_some_and(|(_, suffix)| {
        !suffix.is_empty() && suffix.chars().all(|character| character.is_ascii_lowercase())
      });
      let unit = if typed { name.clone() } else { format!("{name}.service") };
      let said = std::process::Command::new(systemctl?)
        .args(["--user", "is-active", &unit])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
      let state = String::from_utf8_lossy(&said.stdout).trim().to_owned();

      (!state.is_empty()).then_some(matches!(
        state.as_str(),
        "active" | "activating" | "deactivating" | "reloading" | "refreshing"
      ))
    }
  }
}

impl Teller<'_> {
  /// Every open job past the most starts a run may make told once; a stage of a work order is its runner's
  /// to watch.
  pub(super) async fn tell_spinning(&self) -> Result<(), Refusal> {
    for job in open_jobs(self).await? {
      if job.started_by.as_deref() == Some(WORK_ORDER_STARTED_BY) {
        continue;
      }

      let key = spin_told_key(job.id);

      if settings::get(self.db, &key).await?.is_some() {
        continue;
      }

      let starts = self.units.run_starts(&job.worker, &job.unit).await?;

      if starts <= MAX_STARTS {
        continue;
      }

      settings::set(self.db, &key, &starts.to_string()).await?;
      self
        .tell_owner(Fact::Queue(QueueFact::JobCircling {
          job_id: job.id,
          item: job.todo_id,
          worker: job.worker.clone(),
          unit: job.unit.clone(),
          starts,
          most: MAX_STARTS,
        }))
        .await?;
    }

    Ok(())
  }

  // A job's wait mark as this pass leaves it: its target asked, the moment it was first found gone written
  // down, and the mark taken away once the job has written a line after that moment. The mark still
  // standing, and whether anything the screens show moved.
  async fn settle_wait(
    &self,
    job_id: i64,
    last_line_at: i64,
    now: i64,
  ) -> Result<(Option<JobWait>, bool), Refusal> {
    let key = format!("{WAIT_PREFIX}{job_id}");
    let Some(wait) = settings::get(self.db, &key).await?.as_deref().and_then(JobWait::read) else {
      return Ok((None, false));
    };

    if let Some(ended) = wait.ended_at {
      if last_line_at <= ended {
        return Ok((Some(wait), false));
      }

      settings::forget(self.db, &key).await?;

      return Ok((None, false));
    }

    if probe_target(&wait.target, self.systemctl.as_deref()) != Some(false) {
      return Ok((Some(wait), false));
    }

    let ended = JobWait {
      ended_at: Some(now),
      ..wait
    };

    settings::set(self.db, &key, &serde_json::to_string(&ended).unwrap_or_default()).await?;

    Ok((Some(ended), true))
  }

  /// Every open job that has done nothing for five minutes told once for that stillness, at `now`; a job a
  /// person asked to stop is winding down and a stage of a work order is its runner's, so neither is told.
  pub(super) async fn tell_quiet(&self, now: i64) -> Result<(), Refusal> {
    let open = open_jobs(self).await?;
    let open_ids: HashSet<i64> = open.iter().map(|job| job.id).collect();
    let mut moved = false;

    for id in settings::numbered(self.db, WAIT_PREFIX).await? {
      if !open_ids.contains(&id) {
        settings::forget(self.db, &format!("{WAIT_PREFIX}{id}")).await?;
      }
    }

    for job in &open {
      if job.started_by.as_deref() == Some(WORK_ORDER_STARTED_BY)
        || settings::get(self.db, &format!("{JOB_STOP_PREFIX}{}", job.id)).await?.is_some()
      {
        continue;
      }

      let activity = self.units.job_last_line(job.id).await?;
      let last_line_at = job.launched_at.max(activity.as_ref().map_or(0, |activity| activity.at));
      let (wait, wait_moved) = self.settle_wait(job.id, last_line_at, now).await?;
      let standing = wait.as_ref().map(|wait| wait.standing(now));

      moved |= wait_moved;

      if standing == Some(WaitStanding::Waiting) {
        continue;
      }

      let last = match &wait {
        Some(JobWait {
          ended_at: Some(ended),
          ..
        }) => last_line_at.max(*ended),
        _ => last_line_at,
      };
      let line = activity
        .as_ref()
        .map_or_else(|| "launch".to_owned(), |activity| activity.id.to_string());
      let mark = match (&wait, standing) {
        (Some(wait), Some(standing)) => format!("{standing}:{}:{line}", wait.since),
        _ => line,
      };
      let quiet_ms = now - last;

      if quiet_ms < JOB_QUIET_TELL_MS
        || settings::get(self.db, &quiet_told_key(job.id)).await?.as_deref() == Some(mark.as_str())
      {
        continue;
      }

      settings::set(self.db, &quiet_told_key(job.id), &mark).await?;
      self
        .tell_owner(Fact::Queue(QueueFact::JobQuiet {
          job_id: job.id,
          item: job.todo_id,
          worker: job.worker.clone(),
          unit: job.unit.clone(),
          minutes: u32::try_from(quiet_ms / 60_000).unwrap_or(u32::MAX),
          last_activity_at: last,
          last_line: activity.map(|activity| LastLine {
            kind: activity.kind,
            name: activity.name,
          }),
          wait: wait.zip(standing).map(|(wait, standing)| WaitMark {
            what: wait.what,
            target: wait.target,
            until: wait.until,
            ended_at: wait.ended_at,
            standing,
          }),
        }))
        .await?;
    }

    if moved && let Some(events) = self.events {
      events.queue_moved(None);
    }

    Ok(())
  }
}
