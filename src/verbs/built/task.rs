//! A job launched on a worker (docs/api.md, "Jobs"; docs/queue.md, "Launching a job" and "Starting a
//! job"): which worker and which of its checkouts take it, and every refusal a launch is owed before anything
//! is written or started; the unit started under its checkout's lock by the queue's start, or, for a co-work,
//! the item's work order started through the order's own change.

use job_agency::LaunchHeld;
use job_codes::task::{CheckoutHolder, LaunchNote};
use job_codes::{AgentRefusal, JobError, TaskAnswer, TaskRefusal, WorkOrderRefusal};
use queue_read::lines::BUILTIN_WORKER;
use queue_rows::todos::{self, Item};
use queue_rows::work_orders as rows;

use super::Built;
use super::orders::is_checkout;
use crate::queue::job_start::{self, JobStart, Place, Started};
use crate::queue::launches::{self, Launch, Stand, WORK_ORDER_STARTED_BY, checkout_holders};
use crate::queue::requester::{Lack, Requester, requester_run};
use crate::queue::units::Units;
use crate::queue::work_orders as orders;
use crate::queue::workers::{Worker, workers};
use crate::verbs::{Done, OrderAction, TaskLaunch, Tasks};
use crate::{Refusal, now_ms};

fn refused(refusal: TaskRefusal) -> Refusal {
  JobError::from(refusal).into()
}

impl Built {
  // Whose work the item is: a job another user filed runs only on what that user holds, and waits where they
  // lack it. The user rides the job's unit.
  async fn requester(
    &self,
    units: &dyn Units,
    item: Option<&Item>,
    base: &str,
  ) -> Result<Option<String>, Refusal> {
    let Some(item) = item else { return Ok(None) };
    let owner = self.owner_sender().await?;

    match requester_run(units, owner, item.requester_id, item.approved_via, base).await? {
      Requester::Platform => Ok(None),
      Requester::User(user) => Ok(Some(user)),
      Requester::Waits(Lack::Grant { user, grant }) => Err(refused(TaskRefusal::RequesterLacks {
        item: item.id,
        user,
        grant: Some(grant),
      })),
      Requester::Waits(Lack::ClaudeAccount { user }) => Err(refused(TaskRefusal::RequesterLacks {
        item: item.id,
        user,
        grant: None,
      })),
    }
  }

  // A launch that runs the item as a co-work starts its work order, through the same door every change to an
  // order's body goes through. A stopped order is resumed at its turn in its worker's line, never by a launch.
  async fn start_order(
    &self,
    asked: TaskLaunch,
    item: Option<&Item>,
    worker: &Worker,
    notes: Vec<LaunchNote>,
  ) -> Result<TaskAnswer, Refusal> {
    let Some(item) = item else {
      return Err(refused(TaskRefusal::CoworkNeedsItem));
    };
    let id = item.id;

    if !item.approved || item.done || item.held_at.is_some() || item.merged() {
      return Err(JobError::from(WorkOrderRefusal::ItemNotApproved).into());
    }

    if let Some(order) = rows::read(&self.db, id).await?
      && orders::text(&order, "status") == Some("active")
    {
      return Ok(TaskAnswer::OrderAlreadyActive { item: id, notes });
    }

    let start = OrderAction::Start {
      chat: asked.chat,
      cwd: worker.cwd.clone(),
      worker: worker.name.clone(),
      instruction: Some(asked.prompt),
      model: asked.model,
    };
    let order = self.apply(id, start).await?;

    Ok(TaskAnswer::OrderStarted {
      item: id,
      phase: orders::text(&order, "phase").unwrap_or("planning").to_owned(),
      notes,
    })
  }
}

impl Tasks for Built {
  async fn launch_task(&self, asked: TaskLaunch) -> Done<TaskAnswer> {
    let db = &self.db;
    let units = self.units();
    let now = now_ms();
    let known = workers(db, &self.ground.source).await?;
    let item = match asked.item {
      Some(id) => todos::item(db, id).await?,
      None => None,
    };

    // Which worker does the work: the one the launch named, or the one the item is assigned to.
    let wanted = asked
      .worker
      .clone()
      .or_else(|| item.as_ref().map(|item| launches::todo_worker(item, &known).to_owned()))
      .unwrap_or_else(|| BUILTIN_WORKER.to_owned());
    let Some(named) = known.iter().find(|worker| worker.name == wanted) else {
      return Err(JobError::from(AgentRefusal::NoWorker { worker: wanted }).into());
    };

    // The repository the launch is for: the base and its support workers within the count.
    let family = launches::family(&known, &named.name, &self.policy.workers)
      .filter(|(_, members)| members.iter().any(|one| std::ptr::eq(*one, named)));
    let Some((base, members)) = family else {
      return Err(refused(TaskRefusal::LaunchBeyondCount {
        worker: named.name.clone(),
        base: named.base_name().to_owned(),
      }));
    };

    // A launch is of an item that is open, approved and not held, on its own worker, and nothing else.
    let open_items = todos::open_items(db).await?;
    let queue: Vec<Item> =
      launches::queue_for(&open_items, &base.name, &known).into_iter().cloned().collect();
    let runs = match asked.item {
      Some(id) => launches::run_endings(db, id).await?,
      None => vec![],
    };

    launches::launch_refusal(
      &Launch {
        started_by: asked.started_by.as_deref(),
        todo_id: asked.item,
        purpose: asked.purpose.as_deref(),
      },
      &Stand {
        worker: base,
        queue: &queue,
        item: item.as_ref(),
        item_worker: item.as_ref().map(|item| launches::todo_worker(item, &known)),
        runs: &runs,
        now,
      },
    )?;

    let user = self.requester(&units, item.as_ref(), &base.name).await?;
    let jobs = self.ground.running(db, &units).jobs().await?;
    let leases = launches::leases(db).await?;

    // An item already at work is not launched a second time, whichever checkout the job that has it is in.
    if let Some(id) = asked.item {
      let at_work: Vec<CheckoutHolder> = jobs
        .iter()
        .flatten()
        .filter(|job| job.todo_id == Some(id) && job.started_by.as_deref() != Some(WORK_ORDER_STARTED_BY))
        .map(|job| CheckoutHolder {
          worker: job.worker.clone(),
          job: job.id,
          item: job.todo_id,
          unit: job.unit.clone(),
          borrowed: false,
        })
        .collect();

      if !at_work.is_empty() {
        return Err(refused(TaskRefusal::LaunchItemAtWork {
          item: id,
          holders: at_work,
        }));
      }
    }

    // Room on the machine, asked once the item is owed no refusal of its own: a launch the machine has no
    // room for is written down and bmo-agency told, which tells the queue again once there is room.
    let held = units.machine_room().await?.held;

    if !held.is_empty() {
      launches::note_held(db, &held, now).await?;
      units
        .tell_launch_held(&LaunchHeld {
          at: now,
          held: held.clone(),
        })
        .await?;

      return Err(refused(TaskRefusal::LaunchHeldByMachine { held }));
    }

    // Which checkout takes the job: a support worktree already on the item's branch, the base, the support
    // workers by number — or the one support worker the launch named.
    let branch = asked.item.map(launches::item_branch);
    let candidates: Vec<&Worker> = if std::ptr::eq(named, base) {
      launches::launch_order(&members, |member| {
        branch.as_ref().is_some_and(|branch| {
          is_checkout(&member.cwd) && launches::current_branch(&member.cwd).as_ref() == Some(branch)
        })
      })
    } else {
      vec![named]
    };
    let mut open: Vec<&Worker> = vec![];

    for member in &candidates {
      let free = launches::orders_holding(db, &member.cwd, asked.item).await?.is_empty()
        && checkout_holders(member, &known, jobs.as_deref(), &leases)
          .is_none_or(|holders| holders.is_empty());

      if free {
        open.push(member);
      }
    }

    // The checkout the rest of the launch is judged in: the first open one, or the first asked for.
    let worker = open.first().or(candidates.first()).copied().unwrap_or(base);
    let cowork_asked = launches::runs_as_cowork(item.as_ref(), asked.cowork, &asked.prompt);
    let cowork = cowork_asked && is_checkout(&base.cwd);
    let mut notes = vec![];

    if cowork_asked && !cowork {
      notes.push(LaunchNote::CoworkOffCheckout {
        worker: worker.name.clone(),
        cwd: worker.cwd.clone(),
      });
    }

    if cowork {
      return self.start_order(asked, item.as_ref(), worker, notes).await;
    }

    // A job is not started in a checkout a work order has, nor in one another job is at work in.
    let holding = launches::orders_holding(db, &worker.cwd, asked.item).await?;

    if !holding.is_empty() {
      return Err(refused(TaskRefusal::CheckoutHeldByOrder {
        cwd: worker.cwd.clone(),
        orders: holding,
      }));
    }

    launches::checkout_held(worker, checkout_holders(worker, &known, jobs.as_deref(), &leases).as_deref())?;

    let places = (if open.is_empty() { vec![worker] } else { open })
      .into_iter()
      .map(|place| Place {
        name: place.name.clone(),
        cwd: place.cwd.clone(),
      })
      .collect();
    let start = JobStart {
      chat_id: asked.chat,
      prompt: asked.prompt.clone(),
      model: asked.model.clone(),
      started_by: asked.started_by.clone(),
      todo_id: asked.item,
      purpose: asked.purpose.clone(),
      user,
      item_model: item.as_ref().and_then(|item| item.model.clone()),
      places,
    };

    match job_start::start(db, &self.ground.host, &start).await? {
      Started::Job {
        job_id,
        unit,
        landed,
      } => {
        // A job on an item is what turns that item's row to running, so the screens are told.
        if asked.item.is_some() {
          self.events.queue_moved(None);
        }

        Ok(TaskAnswer::JobLaunched {
          job: job_id,
          unit,
          worker: landed.name,
          cwd: landed.cwd,
          chat: asked.chat,
          notes,
        })
      }
      // Which job has it, where a row can say so: the lock is the reading that is always right and it names
      // nobody, so the rows are asked once more for the reader's sake alone.
      Started::Held => {
        let jobs = self.ground.running(db, &units).jobs().await?;
        let leases = launches::leases(db).await?;

        Err(refused(TaskRefusal::CheckoutHeld {
          cwd: worker.cwd.clone(),
          holders: checkout_holders(worker, &known, jobs.as_deref(), &leases).unwrap_or_default(),
        }))
      }
    }
  }
}
