//! A job's end told to the agent as one fact (docs/queue.md, "Telling the agent"): how it ended, what that
//! means for its item — counted against it, set aside at either ceiling, a repair chain held at its last
//! round — and what stands around it: commits waiting to go live, what holds the queue, the machine short
//! of room, and the report it carried.

use std::collections::HashSet;

use job_codes::queue_facts::{Attempts, HoldCause, JobFinish, PendingCommit, RepairChain, Tally};
use job_codes::{Fact, QueueFact, QueueTold};
use queue_rows::todos;
use sea_orm::QueryResult;
use serde_json::Value;

use super::{Teller, job_ended_event};
use crate::queue::jobs::{self, JobEnding};
use crate::queue::launches::run_endings;
use crate::queue::outcomes::{
  JOB_DONE, MAX_EXECUTION_FAILURES, MAX_TODO_ATTEMPTS, RUN_STOPPED, counts_as_attempt,
  counts_as_execution_failure, ending as ending_code, execution_failure_streak, tried_the_work,
};
use crate::queue::{holds, settings};
use crate::{Refusal, all, one};

/// How many rounds of repairing one item a chain may have.
pub const MAX_REPAIR_ROUNDS: i64 = 3;

/// The kind of hold an item set aside for its failures is held under.
pub const FAILURE_HOLD: &str = "failure";

// Where the report a delivered job's ending carries to the agent is to be told.
const REPORT_ROADS: &str = "job_report_roads:";

fn count(value: i64) -> u32 {
  u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

/// The reason an item set aside for its failures is held with, as its row keeps it.
pub fn hold_words(cause: &HoldCause) -> String {
  match cause {
    HoldCause::Attempts { count, last } => format!("The work failed {count} times (last: {last})."),
    HoldCause::LaunchFailures { count, last } => {
      format!("The run failed to start {count} times in a row (last: {last}).")
    }
    HoldCause::RepairCeiling { rounds, item, last } => {
      format!("{rounds} rounds of repairing #{item} ran and it failed again (last: {last}).")
    }
  }
}

/// An item as a judgement of its runs reads it.
struct Held {
  id: i64,
  title: String,
  done: bool,
  done_at: Option<i64>,
  held_at: Option<i64>,
  merged_into: Option<i64>,
  repairs: Option<i64>,
}

const HELD_COLUMNS: &str =
  r#""id", "title", "done", "done_at", "held_at", "merged_into", "repairs_todo_id""#;

fn held_of(row: &QueryResult) -> Result<Held, sea_orm::DbErr> {
  Ok(Held {
    id: row.try_get("", "id")?,
    title: row.try_get("", "title")?,
    done: row.try_get::<i64>("", "done")? == 1,
    done_at: row.try_get("", "done_at")?,
    held_at: row.try_get("", "held_at")?,
    merged_into: row.try_get("", "merged_into")?,
    repairs: row.try_get("", "repairs_todo_id")?,
  })
}

/// The item a chain of repairs began at, and every repair of it after, by number.
struct Chain {
  root: Held,
  repairs: Vec<Held>,
}

/// A delivered report's part of the fact, the room it belongs to and the companion that tells it there.
#[derive(Default)]
struct Reported {
  report: Option<String>,
  room_line_to: Option<i64>,
  removed_room: Option<i64>,
  chat: Option<i64>,
  companion: Option<String>,
}

impl Teller<'_> {
  async fn held_item(&self, id: i64) -> Result<Option<Held>, Refusal> {
    let row = one(
      self.db,
      &format!(r#"SELECT {HELD_COLUMNS} FROM "todos" WHERE "id" = $1"#),
      vec![id.into()],
    )
    .await?;

    Ok(row.map(|row| held_of(&row)).transpose()?)
  }

  // The item a chain of repairs began at: the one it repairs, walked back until one repairs nothing.
  async fn chain_root(&self, id: i64) -> Result<Option<Held>, Refusal> {
    let mut seen = HashSet::new();
    let mut item = self.held_item(id).await?;

    while let Some(at) = &item {
      let Some(next) = at.repairs.filter(|_| !seen.contains(&at.id)) else { break };

      seen.insert(at.id);

      match self.held_item(next).await? {
        Some(found) => item = Some(found),
        None => break,
      }
    }

    Ok(item)
  }

  async fn chain_of(&self, id: i64) -> Result<Option<Chain>, Refusal> {
    let Some(root) = self.chain_root(id).await? else {
      return Ok(None);
    };
    let mut found: Vec<Held> = vec![];
    let mut waiting = vec![root.id];

    while !waiting.is_empty() {
      let next = waiting.remove(0);
      let rows = all(
        self.db,
        &format!(r#"SELECT {HELD_COLUMNS} FROM "todos" WHERE "repairs_todo_id" = $1 ORDER BY "id""#),
        vec![next.into()],
      )
      .await?;

      for row in rows {
        let repair = held_of(&row)?;

        if repair.id == root.id || found.iter().any(|one| one.id == repair.id) {
          continue;
        }

        waiting.push(repair.id);
        found.push(repair);
      }
    }

    found.sort_by_key(|repair| repair.id);

    Ok((!found.is_empty()).then_some(Chain {
      root,
      repairs: found,
    }))
  }

  // How many times the item's work has failed: its own runs whose ending was a try at the work.
  async fn attempts(&self, todo_id: i64, closed_at: Option<i64>) -> Result<i64, Refusal> {
    let runs = run_endings(self.db, todo_id).await?;

    Ok(
      runs
        .iter()
        .filter(|(ended, outcome)| tried_the_work(outcome.as_deref(), closed_at, *ended))
        .count() as i64,
    )
  }

  // What one ending comes to for its item: a failed one counted, and an item at either ceiling set aside,
  // since that hold is what keeps it from being launched again whoever launches.
  async fn judge(&self, ending: &JobEnding, finish: &mut JobFinish, now: i64) -> Result<(), Refusal> {
    let outcome = ending.outcome.as_str();
    let Some(todo_id) = ending.todo_id.filter(|_| outcome != JOB_DONE && outcome != RUN_STOPPED) else {
      return Ok(());
    };
    let item = self.held_item(todo_id).await?;
    let closed_at = item.as_ref().and_then(|item| item.done_at);
    let attempts = self.attempts(todo_id, closed_at).await?;
    let streak = execution_failure_streak(&run_endings(self.db, todo_id).await?);

    if counts_as_attempt(Some(outcome)) {
      finish.attempts = Some(Attempts {
        count: count(attempts),
        limit: count(MAX_TODO_ATTEMPTS),
        counted: tried_the_work(Some(outcome), closed_at, Some(now)),
      });
    }

    if counts_as_execution_failure(Some(outcome)) {
      finish.launch_failures = Some(Tally {
        count: count(streak),
        limit: count(MAX_EXECUTION_FAILURES),
      });
    }

    let cause = if attempts >= MAX_TODO_ATTEMPTS {
      HoldCause::Attempts {
        count: count(attempts),
        last: outcome.to_owned(),
      }
    } else if counts_as_execution_failure(Some(outcome)) && streak >= MAX_EXECUTION_FAILURES {
      HoldCause::LaunchFailures {
        count: count(streak),
        last: outcome.to_owned(),
      }
    } else {
      return Ok(());
    };

    if let Some(item) = &item
      && !item.done
      && item.held_at.is_none()
    {
      todos::hold_as(self.db, todo_id, Some(&hold_words(&cause)), Some(FAILURE_HOLD), now).await?;
      finish.held = Some(cause);
    }

    Ok(())
  }

  // A repair chain that has had all its rounds and still failed, held whole, and told once.
  async fn hold_at_repair_ceiling(
    &self,
    todo_id: i64,
    outcome: &str,
    now: i64,
  ) -> Result<Option<(Chain, Vec<i64>)>, Refusal> {
    if !counts_as_attempt(Some(outcome)) && !counts_as_execution_failure(Some(outcome)) {
      return Ok(None);
    }

    let Some(chain) = self.chain_of(todo_id).await? else {
      return Ok(None);
    };
    let rounds = chain.repairs.len() as i64;

    if rounds < MAX_REPAIR_ROUNDS {
      return Ok(None);
    }

    let reason = hold_words(&HoldCause::RepairCeiling {
      rounds: count(rounds),
      item: todo_id,
      last: outcome.to_owned(),
    });
    let mut held = vec![];

    for item in std::iter::once(&chain.root).chain(chain.repairs.iter()) {
      if !item.done
        && item.held_at.is_none()
        && item.merged_into.is_none()
        && todos::hold_as(self.db, item.id, Some(&reason), Some(FAILURE_HOLD), now).await?
      {
        held.push(item.id);
      }
    }

    let told = format!("todo_repair_ceiling_told:{}", chain.root.id);

    if settings::get(self.db, &told).await?.is_some() {
      return Ok(None);
    }

    settings::set(self.db, &told, &rounds.to_string()).await?;

    Ok(Some((chain, held)))
  }

  // The report a delivered job carries whole, and the room that already heard a line of it; the room the
  // report belongs to; and the companion that tells it there, where its item was a companion's. Nothing
  // where the report was never delivered.
  async fn report_facts(&self, job_id: Option<i64>) -> Result<Reported, Refusal> {
    let Some(job_id) = job_id else {
      return Ok(Reported::default());
    };
    let Some(roads) = settings::take(self.db, &format!("{REPORT_ROADS}{job_id}")).await? else {
      return Ok(Reported::default());
    };
    let roads: Value = serde_json::from_str(&roads).unwrap_or(Value::Null);
    let report = one(
      self.db,
      r#"SELECT "report" FROM "jobs" WHERE "id" = $1 AND "finished_at" IS NOT NULL"#,
      vec![job_id.into()],
    )
    .await?
    .map(|row| row.try_get::<Option<String>>("", "report"))
    .transpose()?
    .flatten()
    .map(|report| report.trim().to_owned())
    .filter(|report| !report.is_empty());

    Ok(Reported {
      report,
      room_line_to: roads.get("room").and_then(Value::as_i64),
      removed_room: roads.get("removed_room").and_then(Value::as_i64),
      chat: roads.get("chat").and_then(Value::as_i64),
      companion: roads.get("agent").and_then(Value::as_str).map(str::to_owned),
    })
  }

  // What stands around the ending: the item's repair chain, the commits it closed on, what holds the queue
  // and the machine short of room.
  async fn surroundings(&self, ending: &JobEnding, finish: &mut JobFinish) -> Result<(), Refusal> {
    if let Some(todo_id) = ending.todo_id
      && let Some(chain) = self.chain_of(todo_id).await?
    {
      finish.repair = Some(RepairChain {
        of: chain.root.id,
        rounds: count(chain.repairs.len() as i64),
        limit: count(MAX_REPAIR_ROUNDS),
      });
    }

    if let Some(job_id) = ending.job_id {
      finish.pending_deploy = self
        .units
        .pending_deploys(job_id)
        .await?
        .into_iter()
        .map(|pending| PendingCommit {
          commit: pending.commit,
          urgent: pending.urgent,
        })
        .collect();
    }

    finish.queue_paused = holds::queue_paused(self.db).await?;
    finish.deploy_hold = holds::deploy_held(self.db).await?;
    finish.machine_short = self.units.machine_room().await?.held;

    Ok(())
  }

  /// One ending told to the agent at `now`, in the room its report belongs to, and a repair chain held at
  /// its ceiling told in the owner's chat once.
  pub(super) async fn tell_ending(&self, ending: &JobEnding, now: i64) -> Result<(), Refusal> {
    let ceiling = match ending.todo_id {
      Some(todo_id) => self.hold_at_repair_ceiling(todo_id, &ending.outcome, now).await?,
      None => None,
    };
    let label = self
      .workers()
      .await?
      .into_iter()
      .find(|worker| Some(&worker.name) == ending.worker.as_ref())
      .map(|worker| worker.label)
      .or_else(|| ending.worker.clone())
      .unwrap_or_default();
    let reported = self.report_facts(ending.job_id).await?;
    let mut finish = JobFinish {
      worker: ending.worker.clone(),
      label,
      job_id: ending.job_id,
      item: ending.todo_id,
      outcome: ending.outcome.clone(),
      ending: ending_code(&ending.outcome),
      succeeded: ending.outcome == JOB_DONE,
      attempts: None,
      launch_failures: None,
      held: None,
      repair: None,
      pending_deploy: vec![],
      queue_paused: false,
      deploy_hold: false,
      machine_short: vec![],
      report_untold: reported.report.is_some() && reported.companion.is_none(),
      report: reported.report,
      room_line_to: reported.room_line_to,
      removed_room: reported.removed_room,
    };

    self.judge(ending, &mut finish, now).await?;
    self.surroundings(ending, &mut finish).await?;

    // A companion's item is told by the companion in the room it filed it in.
    self
      .tell_as(
        reported.companion,
        Fact::Queue(QueueFact::JobFinished(Box::new(finish))),
        vec![job_ended_event(ending.todo_id)],
        None,
        reported.chat.unwrap_or(self.owner_chat),
      )
      .await?;

    if let Some((chain, held)) = ceiling {
      self
        .tell_owner(Fact::Queue(QueueFact::RepairCeiling {
          item: chain.root.id,
          title: chain.root.title,
          rounds: count(chain.repairs.len() as i64),
          limit: count(MAX_REPAIR_ROUNDS),
          held,
          last_job: ending.job_id,
          last_item: ending.todo_id,
          outcome: ending.outcome.clone(),
        }))
        .await?;
    }

    Ok(())
  }

  /// A job that has ended told to the agent as one fact at `now`, after the upkeep every look at the queue
  /// does, and the workers' standings read after it, so the waiting record is kept by every ending.
  pub async fn tell_job_ended(&self, ending: &JobEnding, now: i64) -> Result<QueueTold, Refusal> {
    self.upkeep(now).await?;
    self.tell_ending(ending, now).await?;

    let known = self.workers().await?;

    self.standings(&known, now).await?;

    Ok(QueueTold::Told)
  }

  /// One run's ending as its record reads it, out of the job the runner names and how it says it ended.
  pub async fn ending_of(&self, job_id: Option<i64>, outcome: &str) -> Result<JobEnding, Refusal> {
    jobs::ending(self.db, job_id, outcome).await
  }
}
