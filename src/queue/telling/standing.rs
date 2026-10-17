//! The queue's standing (docs/queue.md, "Telling the agent"): which workers stand with an empty checkout
//! and work of their own waiting, told to the agent at a moment that moved the queue, and the record of a
//! worker standing that way, which the agent hears of once it has stood long enough.

use std::collections::HashSet;

use job_agency::LaunchHeld;
use job_codes::queue_facts::WorkerWaiting;
use job_codes::{Fact, QueueFact, QueueTold};
use queue_rows::instructions::QUEUE_CHANGED;
use queue_rows::todos::Item;
use sea_orm::DatabaseConnection;

use super::Teller;
use crate::Refusal;
use crate::queue::jobs::OpenJob;
use crate::queue::launches::{family, note_held, queue_for};
use crate::queue::routes::Roads;
use crate::queue::turn::{Routable, open_items, worker_queue_turn};
use crate::queue::workers::Worker;
use crate::queue::{holds, settings};

/// How long a worker may stand with work in front of it and nothing in its checkout before the agent is
/// told so.
pub const QUEUE_STALL_TELL_MS: i64 = 5 * 60_000;

// The standing last told, so the same one is not told again by every moment that changes nothing in it.
const QUEUE_TOLD_KEY: &str = "job_queue_told";

fn waiting_key(worker: &str) -> String {
  format!("job_queue_waiting:{worker}")
}

fn waiting_told_key(worker: &str) -> String {
  format!("job_queue_waiting_told:{worker}")
}

fn count(value: usize) -> u32 {
  u32::try_from(value).unwrap_or(u32::MAX)
}

/// One base worker as a look at the queue finds it.
pub(super) struct Standing<'w> {
  pub worker: &'w Worker,
  pub waiting: usize,
  pub checkouts: usize,
  pub open: bool,
}

/// What a look at the queue found, or why it read no worker.
pub(super) enum Look<'w> {
  Read(Vec<Standing<'w>>),
  Stopped(QueueTold),
}

// Whether an item has a route to run on now, as the roads read once for the look say.
struct Routes<'r> {
  roads: Roads<'r, DatabaseConnection>,
  now: i64,
}

impl Routable for Routes<'_> {
  async fn routable(&self, item: &Item) -> bool {
    self
      .roads
      .item_route_open(item.model.as_deref(), item.worker.as_deref(), self.now)
      .await
  }
}

impl Teller<'_> {
  async fn forget_waiting(&self, worker: &str) -> Result<(), Refusal> {
    settings::forget(self.db, &waiting_key(worker)).await?;
    settings::forget(self.db, &waiting_told_key(worker)).await
  }

  async fn forget_all_waiting(&self, known: &[Worker]) -> Result<(), Refusal> {
    for worker in known {
      self.forget_waiting(&worker.name).await?;
    }

    Ok(())
  }

  // What this look found of one worker: a worker at work, or with nothing waiting, is written off the
  // record; one standing open is written down as it is found, and the agent hears of it once it has stood
  // so past the stall's length.
  async fn note_waiting(
    &self,
    worker: &Worker,
    waiting: usize,
    free: bool,
    now: i64,
  ) -> Result<(), Refusal> {
    if waiting == 0 || !free {
      return self.forget_waiting(&worker.name).await;
    }

    let since = settings::get(self.db, &waiting_key(&worker.name))
      .await?
      .and_then(|since| since.trim().parse::<i64>().ok());
    let Some(since) = since else {
      return settings::set(self.db, &waiting_key(&worker.name), &now.to_string()).await;
    };

    if now - since < QUEUE_STALL_TELL_MS
      || settings::get(self.db, &waiting_told_key(&worker.name)).await?.is_some()
    {
      return Ok(());
    }

    settings::set(self.db, &waiting_told_key(&worker.name), "1").await?;
    self
      .tell(
        Fact::Queue(QueueFact::QueueStalled {
          worker: worker.name.clone(),
          label: worker.label.clone(),
          cwd: worker.cwd.clone(),
          waiting: count(waiting),
          minutes: u32::try_from((now - since + 30_000) / 60_000).unwrap_or(u32::MAX),
        }),
        vec![QUEUE_CHANGED.into()],
        None,
        self.owner_chat,
      )
      .await
  }

  /// Every base worker as it stands, each written down or off the waiting record on the way. Nothing is
  /// read while the queue is held, while what is running cannot be asked, or while the machine has no room
  /// for another job, whose hold is written down and told to bmo-agency, which tells the queue again once
  /// the room is back.
  pub(super) async fn standings<'w>(&self, known: &'w [Worker], now: i64) -> Result<Look<'w>, Refusal> {
    if holds::queue_held(self.db).await? {
      self.forget_all_waiting(known).await?;
      settings::forget(self.db, QUEUE_TOLD_KEY).await?;

      return Ok(Look::Stopped(QueueTold::Paused));
    }

    let held = self.units.machine_room().await?.held;

    if !held.is_empty() {
      self.forget_all_waiting(known).await?;
      settings::forget(self.db, QUEUE_TOLD_KEY).await?;
      note_held(self.db, &held, now).await?;
      self.units.tell_launch_held(&LaunchHeld { at: now, held }).await?;

      return Ok(Look::Stopped(QueueTold::MachineShort));
    }

    let waiting = open_items(self.db).await?;
    let Some(jobs) = self.running().jobs().await? else {
      self.forget_all_waiting(known).await?;

      return Ok(Look::Stopped(QueueTold::Unreadable));
    };

    self.read_standings(known, waiting, &jobs, now).await.map(Look::Read)
  }

  async fn read_standings<'w>(
    &self,
    known: &'w [Worker],
    waiting: Vec<Item>,
    jobs: &[OpenJob],
    now: i64,
  ) -> Result<Vec<Standing<'w>>, Refusal> {
    let names = self.running().worker_names(jobs).await?;
    // The checkout is what a turn is taken on, not the worker's name: two workers pointed at one directory
    // would fight over the same files and the same git index.
    let directories: HashSet<&str> = names
      .iter()
      .map(|name| {
        known
          .iter()
          .find(|worker| &worker.name == name)
          .map_or(name.as_str(), |worker| worker.cwd.as_str())
      })
      .collect();
    let occupied: HashSet<&str> = known
      .iter()
      .filter(|worker| directories.contains(worker.cwd.as_str()))
      .map(|worker| worker.cwd.as_str())
      .collect();
    // An item a job is already at work on is no item waiting, whichever checkout that job is in.
    let at_work: HashSet<i64> = jobs.iter().filter_map(|job| job.todo_id).collect();
    let waiting_now: Vec<Item> = waiting.into_iter().filter(|item| !at_work.contains(&item.id)).collect();
    let routes = Routes {
      roads: Roads::read(self.db, self.units, self.policy).await?,
      now,
    };
    let mut found = vec![];

    for worker in known.iter().filter(|worker| !worker.is_support()) {
      let queue = queue_for(&waiting_now, &worker.name, known).len();
      let members = family(known, &worker.name, &self.policy.workers)
        .map_or_else(|| vec![worker], |(_, members)| members);
      let checkouts = members.iter().filter(|member| !occupied.contains(member.cwd.as_str())).count();
      let free = checkouts > 0;

      self.note_waiting(worker, queue, free, now).await?;

      let open = free
        && queue > 0
        && self.launch_waits(&waiting_now, worker, known, &routes, members.len(), now).await?;

      found.push(Standing {
        worker,
        waiting: queue,
        checkouts,
        open,
      });
    }

    if !found.iter().any(|standing| standing.open) {
      settings::forget(self.db, QUEUE_TOLD_KEY).await?;
    }

    Ok(found)
  }

  // Whether a worker's line has an item the agent may launch. A stopped work order at the head is its
  // runner's to resume, in the checkout it holds; a repository with more than one checkout has the others
  // free for what stands behind it, so the look goes past as many such orders as it has other checkouts.
  async fn launch_waits(
    &self,
    items: &[Item],
    worker: &Worker,
    known: &[Worker],
    routes: &Routes<'_>,
    checkouts: usize,
    now: i64,
  ) -> Result<bool, Refusal> {
    let mut line: Vec<Item> = items.to_vec();

    for _ in 0..checkouts {
      let turn = worker_queue_turn(self.db, &line, &worker.name, known, routes, now).await?;

      if !turn.is_resume() {
        return Ok(turn.is_launch());
      }

      let resumed = turn.item();

      line.retain(|item| Some(item.id) != resumed);
    }

    Ok(false)
  }

  /// The queue as it stands, told to the agent at a moment that changed it without a job ending: which
  /// workers stand with an empty checkout and work of their own waiting. A dead job's row is closed first,
  /// so a job that is gone holds no checkout. A moment that repeats tells even a standing told before; one
  /// that rides goes in the next turn until the ride is over.
  pub async fn tell_queue_standing(
    &self,
    moment: &str,
    repeat: bool,
    rides_ms: Option<i64>,
    now: i64,
  ) -> Result<QueueTold, Refusal> {
    self.upkeep(now).await?;

    let known = self.workers().await?;
    let read = match self.standings(&known, now).await? {
      Look::Read(read) => read,
      Look::Stopped(told) => return Ok(told),
    };
    let open: Vec<&Standing> = read.iter().filter(|standing| standing.open).collect();

    if open.is_empty() {
      return Ok(if read.iter().any(|standing| standing.waiting > 0) {
        QueueTold::AllBusy
      } else {
        QueueTold::NothingWaiting
      });
    }

    let workers: Vec<WorkerWaiting> = open
      .iter()
      .map(|standing| WorkerWaiting {
        worker: standing.worker.name.clone(),
        label: standing.worker.label.clone(),
        cwd: standing.worker.cwd.clone(),
        waiting: count(standing.waiting),
        free_checkouts: count(standing.checkouts),
      })
      .collect();
    let told = serde_json::to_string(&workers).unwrap_or_default();

    if !repeat && settings::get(self.db, QUEUE_TOLD_KEY).await?.as_deref() == Some(told.as_str()) {
      return Ok(QueueTold::Unchanged);
    }

    settings::set(self.db, QUEUE_TOLD_KEY, &told).await?;
    self
      .tell(
        Fact::Queue(QueueFact::QueueWaiting {
          moment: moment.to_owned(),
          workers,
        }),
        vec![QUEUE_CHANGED.into()],
        rides_ms.map(|ride| now + ride),
        self.owner_chat,
      )
      .await?;

    Ok(QueueTold::Told)
  }

  /// Whether any worker stands written down as waiting, which is what keeps a sweep that closed nothing
  /// looking at the queue.
  pub(super) async fn any_waiting(&self, known: &[Worker]) -> Result<bool, Refusal> {
    for worker in known {
      if settings::get(self.db, &waiting_key(&worker.name)).await?.is_some() {
        return Ok(true);
      }
    }

    Ok(false)
  }
}
