//! Runs stopped (docs/api.md, "Stopping runs"; docs/queue.md, "Stopping runs"): a run in the agent's host by
//! its execution, a run standing apart by its unit, and an item by the owner's stop of it; and everything
//! running, one target that cannot be stopped keeping none of the rest running. A stop answers whether it
//! was asked or confirmed, and never more than that.

use std::collections::HashSet;

use job_agency::{Agency, Execution};
use job_codes::run_stop::{StopState, TargetStop};
use job_codes::{JobError, RunStopAnswer, RunStopRefusal, WorkOrderRefusal};
use queue_rows::order_lock::with_order_lock;
use queue_rows::{todos, work_orders as rows};

use super::Built;
use crate::queue::outcomes::RUN_STOPPED;
use crate::queue::units::{Units, agency_failed};
use crate::queue::work_orders::{self as orders, StoppedRun};
use crate::queue::{cancels, jobs};
use crate::verbs::{Done, RunStops, Stop, Target, TargetKind};
use crate::{Refusal, now_ms};

/// What a job stopped by its unit carries where nobody said why.
const UNIT_STOP_REASON: &str = "stopped through the service's API";

/// The line of an execution that runs inside its parent's program, as a co-worker does.
const NATIVE_CHILD_LINE: &str = "native-child";

/// The most characters a stop's reason holds.
const REASON_MOST: usize = 2000;

fn refused(refusal: RunStopRefusal) -> Refusal {
  JobError::from(refusal).into()
}

fn order_refused(refusal: WorkOrderRefusal) -> Refusal {
  JobError::from(refusal).into()
}

// A co-worker runs inside its parent's program, under its parent's pane, with nothing of its own to stop.
fn coworker(execution: &Execution) -> bool {
  execution.shared_pane || execution.line.as_deref() == Some(NATIVE_CHILD_LINE)
}

/// How far one target's stop got: confirmed where its run is seen gone, and how many runs it reached.
struct Reached {
  confirmed: bool,
  runs: u32,
}

impl Built {
  /// The owner's stop of an item (docs/verbs.md, "The server's verbs"), at `moment`: held as a stop with his
  /// reason, its work order stopped, every run of it marked and asked to end after the answer, the pool places
  /// still waited for given back, and the queue's standing told. How many runs it asked to end.
  pub(super) async fn stop_item(&self, id: i64, why: &str, moment: &str) -> Result<u32, Refusal> {
    let reason = why.trim().to_owned();

    if reason.is_empty() || reason.chars().count() > REASON_MOST {
      return Err(order_refused(WorkOrderRefusal::StopReasonSize));
    }

    let db = &self.db;
    let runs = with_order_lock(db, id, async {
      let item = todos::item(db, id).await?.ok_or_else(|| order_refused(WorkOrderRefusal::ItemAbsent))?;

      if item.done {
        return Err(order_refused(WorkOrderRefusal::StopDoneItem));
      }

      let now = now_ms();
      let mut stopped: Vec<StoppedRun> = vec![];

      if let Some(mut order) = rows::read(db, id).await?
        && matches!(orders::text(&order, "status"), Some("active" | "paused"))
        && order.get("closing").is_none()
      {
        stopped.extend(orders::stop(&mut order, &reason, now)?);
        rows::save(db, &mut order).await?;
        self.events.order_changed(id, &order);
      }

      let units: Vec<String> = stopped.iter().filter_map(|run| run.unit.clone()).collect();

      for job in jobs::unfinished(db).await? {
        if job.todo_id == Some(id) && !units.contains(&job.unit) {
          stopped.push(StoppedRun {
            unit: Some(job.unit),
            job_id: Some(job.id),
            pool_request: None,
          });
        }
      }

      // Every run asked is marked first, so whoever closes its row closes it as stopped.
      for run in &stopped {
        let job = match (run.job_id, &run.unit) {
          (Some(job), _) => Some(job),
          (None, Some(unit)) => jobs::of_unit(db, unit).await?.map(|(job, _)| job),
          (None, None) => None,
        };

        if let Some(job) = job {
          jobs::request_stop(db, job, &reason).await?;
        }
      }

      todos::stop_hold(db, id, &reason, now).await?;

      Ok::<_, Refusal>(stopped)
    })
    .await?;
    let units = self.units();

    // A place still waited for is given back now: left at the head of the pool it would hold up other work
    // for as long as the running units take to end.
    for run in runs.iter().filter(|run| run.unit.is_none()) {
      if let Some(request) = &run.pool_request {
        units.finish_request(request, &reason, true).await?;
      }
    }

    self.events.queue_moved(None);

    let running: Vec<StoppedRun> = runs.into_iter().filter(|run| run.unit.is_some()).collect();
    let count = u32::try_from(running.len()).unwrap_or(u32::MAX);
    let ending = (self.ground.clone(), self.db.clone(), self.settle);

    tokio::spawn(async move {
      let (ground, db, settle) = ending;
      let units = ground.units();

      if let Err(failed) = ground
        .stops(&db, &units, settle)
        .stop_runs(&running, &reason, RUN_STOPPED, now_ms())
        .await
      {
        eprintln!("bmo-job: the runs of item #{id} were not all stopped: {}", failed.error);
      }
    });

    self.tell_standing(moment, false).await?;

    Ok(count)
  }

  // A run in the agent's host, reached by the cancel of its own execution, whose record confirms it: a row
  // still open is a stop asked that has not yet arrived.
  async fn stop_execution(&self, target: &Target) -> Result<Reached, Refusal> {
    let id = target.id.as_str();
    let execution = self.ground.agency.execution(id).await.map_err(agency_failed)?;
    let Some(execution) = execution else {
      return Err(refused(RunStopRefusal::StopTargetAbsent {
        target: target.to_string(),
      }));
    };

    if coworker(&execution) {
      return Err(refused(RunStopRefusal::StopCoworker {
        parent: execution.parent,
      }));
    }

    if execution.ended_at.is_some() {
      return Err(refused(RunStopRefusal::StopTargetEnded {
        target: target.to_string(),
      }));
    }

    let units = self.units();

    self.stops(&units).stop(id).await?;

    let after = self.ground.agency.execution(id).await.map_err(agency_failed)?;
    let gone = after.is_none_or(|after| after.ended_at.is_some());

    Ok(Reached {
      confirmed: take_back(&units, id, gone).await?,
      runs: 1,
    })
  }

  // A run standing apart in a unit of its own: the open rows and what stands say whether there is anything
  // under that name, and what stands says afterwards whether it is gone.
  async fn stop_unit(&self, target: &Target, why: Option<&str>) -> Result<Reached, Refusal> {
    let unit = target.id.as_str();
    let units = self.units();
    let running = self.ground.running(&self.db, &units);
    let jobs: Vec<i64> = jobs::unfinished(&self.db)
      .await?
      .into_iter()
      .filter(|job| job.unit == unit)
      .map(|job| job.id)
      .collect();
    let named = [unit.to_owned()];

    if jobs.is_empty() && !running.active(&named).await?.contains(unit) {
      return Err(refused(RunStopRefusal::StopTargetEnded {
        target: target.to_string(),
      }));
    }

    // A job is marked stopped before its unit is asked: a runner taken down before it writes its own ending
    // leaves its row to the sweep, which reads the mark to close it as a stop.
    let reason = why.map(str::trim).filter(|why| !why.is_empty()).unwrap_or(UNIT_STOP_REASON);

    for job in jobs {
      jobs::request_stop(&self.db, job, reason).await?;
    }

    self.stops(&units).stop(unit).await?;

    let gone = !running.active(&named).await?.contains(unit);

    Ok(Reached {
      confirmed: take_back(&units, unit, gone).await?,
      runs: 1,
    })
  }

  // The owner's stop of an item, which asks its runs to end without waiting for them, so the only ending it
  // can honestly confirm is an item that had nothing running.
  async fn stop_of_item(&self, target: &Target, why: Option<&str>) -> Result<Reached, Refusal> {
    let id = target.id.as_str();
    let Some(number) = id.parse::<i64>().ok().filter(|number| *number > 0 && number.to_string() == id)
    else {
      return Err(refused(RunStopRefusal::StopItemNumber { id: id.to_owned() }));
    };
    let runs = self.stop_item(number, why.unwrap_or_default(), "stopped").await?;

    Ok(Reached {
      confirmed: runs == 0,
      runs,
    })
  }

  async fn stop_target(&self, target: &Target, why: Option<&str>) -> Result<Reached, Refusal> {
    if target.id.trim().is_empty() {
      return Err(refused(RunStopRefusal::StopTargetMissing));
    }

    match target.kind {
      TargetKind::Execution => self.stop_execution(target).await,
      TargetKind::Unit => self.stop_unit(target, why).await,
      TargetKind::Item => self.stop_of_item(target, why).await,
    }
  }

  // Everything running now: the units of the open jobs that still stand first, then the open executions that
  // stand under none of them and are no co-worker, so a walk asks everything else before the turn that may
  // be the one asking.
  async fn running_targets(&self) -> Result<Vec<Target>, Refusal> {
    let units = self.units();
    let mut named: Vec<String> = vec![];

    for job in jobs::unfinished(&self.db).await? {
      if !named.contains(&job.unit) {
        named.push(job.unit);
      }
    }

    let standing = self.ground.running(&self.db, &units).active(&named).await?;
    let named: Vec<String> = named.into_iter().filter(|unit| standing.contains(unit)).collect();
    let taken: HashSet<&str> = named.iter().map(String::as_str).collect();
    let under = |owner: &Option<String>| owner.as_deref().is_some_and(|owner| taken.contains(owner));
    let mut targets: Vec<Target> = named
      .iter()
      .map(|unit| Target {
        kind: TargetKind::Unit,
        id: unit.clone(),
      })
      .collect();

    for execution in units.open_executions().await? {
      if coworker(&execution) || under(&execution.parent) || under(&execution.root) {
        continue;
      }

      targets.push(Target {
        kind: TargetKind::Execution,
        id: execution.id,
      });
    }

    Ok(targets)
  }
}

// The cancel taken back once the run is seen gone. A stop only asked leaves it: the run is still on its way
// down, and the cancel keeps it from starting anything new.
async fn take_back(units: &dyn Units, name: &str, gone: bool) -> Result<bool, Refusal> {
  if gone {
    cancels::take_back(units, name).await?;
  }

  Ok(gone)
}

impl RunStops for Built {
  async fn stop_run(&self, stop: Stop) -> Done<RunStopAnswer> {
    let reached = self.stop_target(&stop.target, stop.why.as_deref()).await?;
    let target = stop.target.to_string();

    Ok(if reached.confirmed {
      RunStopAnswer::StopConfirmed {
        target,
        runs: reached.runs,
      }
    } else {
      RunStopAnswer::StopAsked {
        target,
        runs: reached.runs,
      }
    })
  }

  async fn stop_all(&self) -> Done<RunStopAnswer> {
    let targets = self.running_targets().await?;

    if targets.is_empty() {
      return Ok(RunStopAnswer::NothingRunning);
    }

    let (mut asked, mut confirmed) = (0, 0);
    let mut stops = vec![];

    for target in targets {
      let state = match self.stop_target(&target, None).await {
        Ok(reached) if reached.confirmed => {
          asked += 1;
          confirmed += 1;
          StopState::Ended
        }
        Ok(_) => {
          asked += 1;
          StopState::Stopping
        }
        Err(refusal) => StopState::Refused { why: refusal.error },
      };

      stops.push(TargetStop {
        target: target.to_string(),
        state,
      });
    }

    Ok(RunStopAnswer::AllAsked {
      asked,
      confirmed,
      targets: stops,
    })
  }
}
