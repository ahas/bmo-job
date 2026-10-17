//! A job's ending written as the runner closes it (docs/runs.md, "The ending"): the row closed with its report
//! and outcome, a run that broke handed to the agent, the agent told the state of a run that took no work,
//! the report left waiting and delivered by the telling, where the job's time went, and the handoff that tells
//! the queue the job is over with the servers it left listening swept. What a failed call of the telling
//! leaves waits for the telling's sweep, which takes it as the service running then.

use job_agency::{ModelSpend, TurnSpan};
use job_codes::job_ended::{JobEnding, Selection};
use job_codes::{Fact, JobFact};
use job_receiver::{Instruction, Said};
use queue_read::time::RUN_PHASE;
use queue_rows::todos;
use sea_orm::TransactionTrait;

use super::{Ground, WORK_FINAL_PHASE, WORK_STEP_PHASE};
use crate::queue::outcomes::{JOB_BLOCKED, JOB_DONE, JOB_ERROR, JOB_LIMIT, JOB_WORK_STEP, RUN_STOPPED};
use crate::queue::settings;
use crate::queue::telling::{JOB_SORT, WAITING_ENDING, WAITING_REPORT};
use crate::queue::work_orders as orders;
use crate::{Refusal, all, now_ms, one, run};

/// The words a report no row would keep goes to the agent under, ahead of the report as it stands.
const UNKEPT_REPORT: &str =
  "A worker's report could not be kept on its job's row, so it is handed over here as it stands:";

// The phases of a job: the wait for the checkout's lock, what the runner did before the run, the run itself,
// and what it did after the run answered.
const WAIT_PHASE: &str = "wait";
const READY_PHASE: &str = "ready";
const FINISH_PHASE: &str = "finish";

/// Whether a run's ending is the item's job ending: never a work order's middle turn, and a final turn only
/// where it ended done.
pub fn ends_as_job(phase: &str, outcome: &str) -> bool {
  if phase == WORK_STEP_PHASE {
    return false;
  }

  phase != WORK_FINAL_PHASE || outcome == JOB_DONE
}

/// What the work measured for the ending's record: the model asked for and the road it ran on, when the run
/// itself began and ended, what it spent, and the spans its turns kept.
#[derive(Clone, Debug, Default)]
pub struct RunFacts {
  pub requested: String,
  pub actual: Option<Selection>,
  pub run_started_at: i64,
  pub run_ended_at: i64,
  pub spend: Vec<ModelSpend>,
  pub spans: Vec<TurnSpan>,
}

fn phase(name: &str, from: i64, to: i64) -> TurnSpan {
  TurnSpan {
    kind: "phase".into(),
    name: name.into(),
    seconds: (to - from).max(0) as f64 / 1000.0,
    times: 1,
    tokens: None,
    input_tokens: None,
    output_tokens: None,
    cache_read_tokens: None,
    cache_write_tokens: None,
  }
}

/// The phases of a job, cut at its row's opening, the lock held, the run's start and end, and the row's
/// closing, all in milliseconds, so they sum to the stretch the item's total counts.
pub fn job_phases(
  launched_at: i64,
  started_at: i64,
  run_started_at: i64,
  run_ended_at: i64,
  ended_at: i64,
) -> Vec<TurnSpan> {
  vec![
    phase(WAIT_PHASE, launched_at, started_at),
    phase(READY_PHASE, started_at, run_started_at),
    phase(RUN_PHASE, run_started_at, run_ended_at),
    phase(FINISH_PHASE, run_ended_at, ended_at),
  ]
}

/// What the run spent, one span per model it ran on, with no seconds of its own.
pub fn usage_spans(spend: &[ModelSpend]) -> Vec<TurnSpan> {
  spend
    .iter()
    .filter(|one| one.tokens > 0)
    .map(|one| TurnSpan {
      kind: "usage".into(),
      name: one.model.clone(),
      seconds: 0.0,
      times: 1,
      tokens: Some(one.tokens),
      input_tokens: Some(one.input_tokens),
      output_tokens: Some(one.output_tokens),
      cache_read_tokens: Some(one.cache_read_tokens),
      cache_write_tokens: Some(one.cache_write_tokens),
    })
    .collect()
}

// The ending an outcome word stands for, where it is one of the endings a job's row keeps.
fn ending_kind(outcome: &str) -> Option<JobEnding> {
  serde_json::from_value(serde_json::Value::String(outcome.replace('-', "_"))).ok()
}

/// The job whose ending is written, and what writing it stands on.
pub struct Ending<'a> {
  pub ground: &'a Ground<'a>,
  pub job_id: i64,
  pub worker: String,
  pub phase: String,
  /// Whether the run kept no session of the worker's: a co-work branch in a place of its own.
  pub ephemeral: bool,
  /// When the runner held its locks, in milliseconds: where the job's own work began.
  pub started_at: i64,
}

impl Ending<'_> {
  async fn todo_id(&self) -> Result<Option<i64>, Refusal> {
    let row = one(
      self.ground.db,
      r#"SELECT "todo_id" FROM "jobs" WHERE "id" = $1"#,
      vec![self.job_id.into()],
    )
    .await?;

    Ok(row.map(|row| row.try_get::<Option<i64>>("", "todo_id")).transpose()?.flatten())
  }

  // Whether the item, still open, has steps of its work order still to build; none where either cannot be read.
  // An item the final turn closed ends its order whatever the order still holds, and that ending is told.
  async fn steps_ahead(&self) -> bool {
    let Ok(Some(todo_id)) = self.todo_id().await else { return false };

    if !matches!(todos::item(self.ground.db, todo_id).await, Ok(Some(item)) if !item.done) {
      return false;
    }

    matches!(queue_rows::work_orders::read(self.ground.db, todo_id).await, Ok(Some(order)) if orders::steps_ahead(&order))
  }

  // When the job's row was opened and closed, in milliseconds.
  async fn times(&self) -> Result<Option<(i64, Option<i64>)>, Refusal> {
    let row = one(
      self.ground.db,
      r#"SELECT "launched_at", "finished_at" FROM "jobs" WHERE "id" = $1"#,
      vec![self.job_id.into()],
    )
    .await?;

    Ok(
      row
        .map(|row| {
          Ok::<_, sea_orm::DbErr>((
            row.try_get::<i64>("", "launched_at")?,
            row.try_get::<Option<i64>>("", "finished_at")?,
          ))
        })
        .transpose()?,
    )
  }

  /// The ending written: the report closes the row as the outcome says, and what follows is the state told for
  /// a run that took no work, or the report left for the telling to deliver. `gated` is off for the runner's
  /// fixed notices, which are not a report a gate should judge.
  pub async fn write(&self, text: &str, outcome: &str, gated: bool, facts: &RunFacts) {
    let ground = self.ground;
    let ok = outcome == JOB_DONE || outcome == JOB_WORK_STEP;
    let kept = match ground.teller.close_job(self.job_id, text, ok, outcome, now_ms()).await {
      Ok(()) => true,
      Err(why) => {
        eprintln!("bmo-job: job {}'s ending could not be written: {}", self.job_id, why.error);
        false
      }
    };

    // A run that broke while it worked is the agent's to look into, as an order to find the cause.
    if let Err(why) = ground.teller.order_broken_job(self.job_id, now_ms()).await {
      eprintln!("bmo-job: the failure order could not be raised: {}", why.error);
    }

    let times = self.times().await.unwrap_or_else(|why| {
      eprintln!("bmo-job: job {}'s times could not be read: {}", self.job_id, why.error);
      None
    });

    if outcome == JOB_ERROR {
      return self.record_time(times, facts).await;
    }

    // None of these is a report about work: the agent is told the state instead.
    if [JOB_LIMIT, JOB_BLOCKED, RUN_STOPPED].contains(&outcome) {
      if let Some(ending) = ending_kind(outcome) {
        let todo_id = self.todo_id().await.unwrap_or(None);

        ground
          .tell(Fact::Job(JobFact::JobStopped {
            job_id: self.job_id,
            item: todo_id,
            worker: self.worker.clone(),
            outcome: ending,
            requested: facts.requested.clone(),
            actual: facts.actual.clone(),
          }))
          .await;
      }

      return self.record_time(times, facts).await;
    }

    // A work order's middle turn is the order's to read; nobody is handed it as a report.
    if self.phase == WORK_STEP_PHASE {
      return self.record_time(times, facts).await;
    }

    let waiting = kept
      && match settings::set(
        ground.db,
        &format!("{WAITING_REPORT}{}", self.job_id),
        if gated { "gated" } else { "ungated" },
      )
      .await
      {
        Ok(()) => true,
        Err(why) => {
          eprintln!(
            "bmo-job: job {}'s report could not be left for the telling: {}",
            self.job_id, why.error
          );
          false
        }
      };

    if waiting {
      if let Err(why) = ground.teller.deliver_report(self.job_id, now_ms()).await {
        eprintln!("bmo-job: job report: it waits for the telling's next sweep: {}", why.error);
      }
    } else {
      self.hand_unkept_report(text).await;
    }

    self.record_time(times, facts).await;
  }

  // A report no row would keep, handed to the agent as it stands.
  async fn hand_unkept_report(&self, text: &str) {
    let ground = self.ground;
    let instruction = Instruction {
      agent: None,
      chat: Some(ground.owner_chat),
      said: Said::Order(format!("{UNKEPT_REPORT}\n\n{text}")),
      kind: None,
      sort: JOB_SORT.to_owned(),
      summary: None,
      expires_at: None,
      events: vec![],
      rides_until: None,
      via: None,
      run: None,
    };

    if let Err(why) = ground.units.queue_instruction(&instruction).await {
      eprintln!("bmo-job: job {}'s report could not be handed over: {}", self.job_id, why.error);
    }
  }

  // Where this job's time went and what it spent, written once as the job ends: the phases off the runner's
  // clock and the row's own instants, the spans its turns kept, and what each model burnt.
  async fn record_time(&self, times: Option<(i64, Option<i64>)>, facts: &RunFacts) {
    let Some((launched_at, finished_at)) = times else {
      return;
    };
    let ended_at = finished_at.unwrap_or_else(now_ms);
    let spans: Vec<TurnSpan> =
      job_phases(launched_at, self.started_at, facts.run_started_at, facts.run_ended_at, ended_at)
        .into_iter()
        .chain(facts.spans.iter().cloned())
        .chain(usage_spans(&facts.spend))
        .collect();

    if let Err(why) = self.record_spans(&spans).await {
      eprintln!("bmo-job: job {}'s time could not be written: {}", self.job_id, why.error);
    }
  }

  async fn record_spans(&self, spans: &[TurnSpan]) -> Result<(), Refusal> {
    let todo_id = self.todo_id().await?;
    let tx = self.ground.db.begin().await?;

    run(&tx, r#"DELETE FROM "job_spans" WHERE "job_id" = $1"#, vec![self.job_id.into()]).await?;

    for span in spans {
      run(
        &tx,
        r#"INSERT INTO "job_spans" ("job_id", "todo_id", "kind", "name", "seconds", "times", "tokens", "input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens")
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
        vec![
          self.job_id.into(),
          todo_id.into(),
          span.kind.clone().into(),
          span.name.clone().into(),
          span.seconds.into(),
          span.times.into(),
          span.tokens.into(),
          span.input_tokens.into(),
          span.output_tokens.into(),
          span.cache_read_tokens.into(),
          span.cache_write_tokens.into(),
        ],
      )
      .await?;
    }

    tx.commit().await?;

    Ok(())
  }

  /// The queue's telling told the job is over and how it ended, which it tells the agent as a fact; an ending
  /// the telling did not take waits for its next sweep. A server the job's turns left listening dies with the
  /// job first; a branch place's step ran in no program of the worker's. A work order's run is handed off only
  /// as its final turn ending done, and not where that turn added steps: the order goes back to its build, and
  /// its ending is nobody's to be told.
  pub async fn hand_off(&self, outcome: &str) {
    if !ends_as_job(&self.phase, outcome) || (self.phase == WORK_FINAL_PHASE && self.steps_ahead().await) {
      return;
    }

    if !self.ephemeral {
      match self.ground.calls.sweep_servers(&self.worker).await {
        Ok(lines) => lines.iter().for_each(|line| println!("{line}")),
        Err(why) => eprintln!("bmo-job: job servers: {}", why.error),
      }
    }

    self.tell_finished(outcome).await;
  }

  /// The queue's telling told the job ended and how; an ending it did not take waits for its next sweep.
  pub async fn tell_finished(&self, outcome: &str) {
    let ground = self.ground;
    let told = async {
      let ending = ground.teller.ending_of(Some(self.job_id), outcome).await?;

      ground.teller.tell_job_ended(&ending, now_ms()).await
    };

    match told.await {
      Ok(told) => println!("handoff ({outcome}): {}", Into::<&str>::into(told)),
      Err(why) => {
        eprintln!("bmo-job: handoff: the ending waits for the telling's next sweep: {}", why.error);

        if let Err(why) =
          settings::set(ground.db, &format!("{WAITING_ENDING}{}", self.job_id), outcome).await
        {
          eprintln!("bmo-job: handoff: the ending could not be left waiting either: {}", why.error);
        }
      }
    }
  }

  /// The checkouts this job borrowed let go: their loans ended, and the workers they belonged to named, for the
  /// runner to put each back as it stood.
  pub async fn release_leases(&self) -> Result<Vec<String>, Refusal> {
    let tx = self.ground.db.begin().await?;
    let names = all(
      &tx,
      r#"SELECT "worker" FROM "checkout_leases" WHERE "job_id" = $1 ORDER BY "worker""#,
      vec![self.job_id.into()],
    )
    .await?
    .iter()
    .map(|row| row.try_get::<String>("", "worker"))
    .collect::<Result<Vec<_>, _>>()?;

    run(&tx, r#"DELETE FROM "checkout_leases" WHERE "job_id" = $1"#, vec![self.job_id.into()]).await?;
    tx.commit().await?;

    Ok(names)
  }
}
