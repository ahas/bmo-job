//! The open job rows held against what still runs (docs/queue.md, "The sweep"): a runner that died without
//! reporting leaves its row open and its chat waiting on an answer nobody is going to send, so its row is
//! closed, its chat told the job is gone, and the run handed to the agent as a broken one to look into.
//! The endings and reports a runner left waiting for a service that was not there are taken on the same
//! pass.

use job_codes::{Fact, JobFact};
use queue_read::runs::work_label;
use sea_orm::TransactionTrait;
use serde_json::Value;

use super::{NewsRoom, Teller, job_ended_event};
use crate::queue::jobs::{self, JOB_STOP_PREFIX, JobEnding};
use crate::queue::launches::{self, WORK_ORDER_STARTED_BY};
use crate::queue::outcomes::{JOB_DIED, RUN_STOPPED};
use crate::queue::settings;
use crate::queue::voiced::{self, VoicedNotice};
use crate::{Refusal, all, one};

/// How long a job is given to start in: a unit gone with the row still open before this is a unit systemd
/// has not finished starting.
const DEAD_JOB_GRACE_MS: i64 = 5 * 60_000;

/// The setting an ending no service took waits under, before the job's number.
pub const WAITING_ENDING: &str = "job_ending_waiting:";

/// One job not yet finished, as the sweep reads it.
pub(super) struct OpenJob {
  pub id: i64,
  pub chat_id: i64,
  pub unit: String,
  pub launched_at: i64,
  pub started_by: Option<String>,
  pub todo_id: Option<i64>,
  pub purpose: Option<String>,
  pub worker: String,
}

/// Every job not yet finished, oldest first.
pub(super) async fn open_jobs(teller: &Teller<'_>) -> Result<Vec<OpenJob>, Refusal> {
  let rows = all(
    teller.db,
    r#"SELECT "id", "chat_id", "unit", "launched_at", "started_by", "todo_id", "purpose", "worker" FROM "jobs" WHERE "finished_at" IS NULL ORDER BY "id""#,
    vec![],
  )
  .await?;

  rows
    .iter()
    .map(|row| {
      Ok(OpenJob {
        id: row.try_get("", "id")?,
        chat_id: row.try_get("", "chat_id")?,
        unit: row.try_get("", "unit")?,
        launched_at: row.try_get("", "launched_at")?,
        started_by: row.try_get("", "started_by")?,
        todo_id: row.try_get("", "todo_id")?,
        purpose: row.try_get("", "purpose")?,
        worker: row
          .try_get::<Option<String>>("", "worker")?
          .unwrap_or_else(|| queue_read::lines::BUILTIN_WORKER.to_owned()),
      })
    })
    .collect()
}

/// How the job ended, as its closed row records it; none while it runs.
pub(super) async fn outcome_of(teller: &Teller<'_>, job_id: i64) -> Result<Option<String>, Refusal> {
  let row = one(
    teller.db,
    r#"SELECT "outcome" FROM "jobs" WHERE "id" = $1 AND "finished_at" IS NOT NULL"#,
    vec![job_id.into()],
  )
  .await?;

  Ok(row.map(|row| row.try_get::<Option<String>>("", "outcome")).transpose()?.flatten())
}

/// A job's row closed with its report and ending at `now`, in one transaction with the mark that asked
/// for its stop.
pub(super) async fn finish_job(
  teller: &Teller<'_>,
  job_id: i64,
  report: &str,
  ok: bool,
  outcome: &str,
  now: i64,
) -> Result<(), Refusal> {
  let unit = teller.db.begin().await?;

  jobs::finish(&unit, job_id, report, ok, outcome, now).await?;
  unit.commit().await?;

  Ok(())
}

/// What the row of a job that died without reporting says.
pub fn gone_report() -> String {
  "The job ended without reporting: its run was killed or crashed, and nothing more will come of it."
    .to_owned()
}

/// What the row of a stopped job whose runner wrote no ending of its own says.
pub fn stopped_job_report(reason: &str) -> String {
  format!("The run was stopped and ended here. Reason: {reason}")
}

/// What a chat is told about a job of its own that died, and where: a job launched in a room whose
/// registration is gone reports to the owner instead, naming that room. A run with no item behind it is
/// named by why it was launched only where the owner is the one told: that line was written for the owner
/// and may name a room another chat has no business hearing of.
pub(super) fn dead_job_notice(
  told: &NewsRoom,
  owner_chat: i64,
  job: &OpenJob,
  attempt: i64,
) -> VoicedNotice {
  let purpose = job
    .purpose
    .as_deref()
    .map(str::trim)
    .filter(|purpose| job.todo_id.is_none() && told.chat == owner_chat && !purpose.is_empty())
    .map(str::to_owned);
  let fact = Fact::Job(JobFact::JobGone {
    job_id: job.id,
    label: job.todo_id.map(|todo| work_label(Some(todo), attempt)),
    purpose,
    removed_room: told.removed,
  });

  VoicedNotice {
    follow_up: Some(
      "It is over; no result will come from it and nothing will be retried on its own.".into(),
    ),
    events: vec![job_ended_event(job.todo_id)],
    ..VoicedNotice::of(fact, "tell the chat the job it was waiting on is gone.")
  }
}

impl Teller<'_> {
  // A stage of a work order under way is its runner's to close; a stopped order's rows are closed here.
  async fn runner_collects(&self, job: &OpenJob) -> Result<bool, Refusal> {
    let (Some(todo_id), Some(WORK_ORDER_STARTED_BY)) = (job.todo_id, job.started_by.as_deref()) else {
      return Ok(false);
    };

    Ok(
      queue_rows::work_orders::read(self.db, todo_id)
        .await?
        .is_some_and(|order| order.get("status").and_then(Value::as_str) == Some("active")),
    )
  }

  // The jobs whose unit is gone with the row still open, past the grace a job gets to start in. A probe
  // nobody could answer names none of them: read as an empty answer it would close every row.
  async fn dead_jobs(&self, now: i64) -> Result<Vec<OpenJob>, Refusal> {
    let before = now - DEAD_JOB_GRACE_MS;
    let mut old = vec![];

    for job in open_jobs(self).await? {
      if job.launched_at < before && !self.runner_collects(&job).await? {
        old.push(job);
      }
    }

    if old.is_empty() {
      return Ok(old);
    }

    let units: Vec<String> = old.iter().map(|job| job.unit.clone()).collect();
    let Some(alive) = self.running().answered(&units).await? else {
      return Ok(vec![]);
    };

    Ok(old.into_iter().filter(|job| !alive.contains(&job.unit)).collect())
  }

  /// Every job whose runner died without reporting closed at `now`, its chat told and the broken run
  /// handed to the agent; the rows it closed.
  pub(super) async fn close_dead(&self, now: i64) -> Result<Vec<i64>, Refusal> {
    let mut closed = vec![];

    for job in self.dead_jobs(now).await? {
      let room = self.news_room(job.chat_id).await?;
      let notice = dead_job_notice(&room, self.owner_chat, &job, launches::attempt(self.db, job.id).await?);
      // Read before the closing, which takes the mark away with it.
      let stop_reason = settings::get(self.db, &format!("{JOB_STOP_PREFIX}{}", job.id)).await?;
      let report = stop_reason.as_deref().map_or_else(gone_report, stopped_job_report);

      finish_job(self, job.id, &report, false, JOB_DIED, now).await?;
      closed.push(job.id);

      // A job somebody stopped is gone because it was asked to go; nobody is left waiting on it who has
      // not already been told.
      if outcome_of(self, job.id).await?.as_deref() == Some(RUN_STOPPED) {
        continue;
      }

      // A notice that could not be queued is the receiver's failure, not the sweep's: the other dead rows
      // are still closed.
      if let Err(failed) = voiced::queue(self.units, room.chat, &notice, now).await {
        eprintln!("bmo-job: dead job {}: its notice was not queued: {}", job.id, failed.error);
      }

      if let Err(failed) = self.order_job_failure(job.id, now).await {
        eprintln!("bmo-job: dead job {}: its failure order was not raised: {}", job.id, failed.error);
      }
    }

    Ok(closed)
  }

  /// What a job's end left waiting, taken at `now` by the service running now: every report no service
  /// took as its job ended, then every ending, each once and a job's report before its ending.
  pub async fn deliver_waiting(&self, now: i64) -> Result<(), Refusal> {
    self.deliver_waiting_reports(now).await?;

    let mut endings = vec![];

    for job_id in settings::numbered(self.db, WAITING_ENDING).await? {
      if let Some(outcome) = settings::take(self.db, &format!("{WAITING_ENDING}{job_id}")).await? {
        endings.push(jobs::ending(self.db, Some(job_id), &outcome).await?);
      }
    }

    if endings.is_empty() {
      return Ok(());
    }

    for ending in &endings {
      self.tell_ending(ending, now).await?;
    }

    let known = self.workers().await?;

    self.standings(&known, now).await.map(drop)
  }

  /// The upkeep the two ordinary ends cannot do, at `now`: a dead runner's row closed and told as that
  /// job's ending, what runners left waiting taken, a circling job and a quiet one told, and the waiting
  /// record kept current; the rows it closed.
  pub async fn sweep(&self, now: i64) -> Result<Vec<i64>, Refusal> {
    let closed = self.close_dead(now).await?;

    self.deliver_waiting(now).await?;
    // After the dead are closed, so a row just written off is never told for circling or standing still.
    self.tell_spinning().await?;
    self.tell_quiet(now).await?;

    let known = self.workers().await?;

    if closed.is_empty() {
      if self.any_waiting(&known).await? {
        self.standings(&known, now).await?;
      }

      return Ok(closed);
    }

    for &id in &closed {
      let row = one(
        self.db,
        r#"SELECT "started_by", "todo_id", "worker", "outcome" FROM "jobs" WHERE "id" = $1"#,
        vec![id.into()],
      )
      .await?;
      let Some(row) = row else { continue };

      // A stage of a work order is its runner's to tell of, in its own notice.
      if row.try_get::<Option<String>>("", "started_by")?.as_deref() == Some(WORK_ORDER_STARTED_BY) {
        continue;
      }

      let ending = JobEnding {
        job_id: Some(id),
        todo_id: row.try_get("", "todo_id")?,
        worker: row.try_get("", "worker")?,
        outcome: row.try_get::<Option<String>>("", "outcome")?.unwrap_or_else(|| JOB_DIED.to_owned()),
      };

      self.tell_ending(&ending, now).await?;
    }

    self.standings(&known, now).await?;

    Ok(closed)
  }
}
