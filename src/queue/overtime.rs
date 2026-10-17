//! A clock asks before it cuts (docs/queue.md, "A run past its clock"): a run of work that reaches its
//! clock is not cut on the clock alone. The agent is asked in the owner's private chat, by an order queued
//! through bmo-receiver, and the run goes on until it answers keep or cut, or until the wait for an answer
//! runs out and the service cuts it itself. The ask is a row of `clock_holds`, which the agent answers
//! through the overtime verb and the turn reads on its next look.

use job_receiver::{Instruction, InstructionState, Said};
use queue_read::dates::iso;
use sea_orm::ConnectionTrait;

use super::units::Units;
use crate::{Refusal, now_ms, one, run};

/// How long the service waits for the agent's answer before it cuts on its own: the run holds a lock or a
/// place others wait on the whole while.
pub const ANSWER_WAIT_MS: i64 = 30 * 60_000;

// How often a turn waiting on an answer looks for one.
const LOOK_MS: i64 = 5_000;

/// The longest one keep gives; the agent is asked again when it runs out.
pub const LONGEST_KEEP_MS: i64 = 2 * 60 * 60_000;

/// The family an ask's order is listed under.
pub const OVERTIME_SORT: &str = "overtime";

/// A run past its clock, as the turn that runs it reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overrun {
  /// The clock, in milliseconds.
  pub bound_ms: i64,
  pub started_at: i64,
  pub last_activity_at: Option<i64>,
  pub last_activity: Option<String>,
}

/// What a turn past its clock is told: go on and look again at a moment, or be cut.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overtime {
  AskAgainAt(i64),
  Cut,
}

/// Who gave the verdict that cut a run: the agent answering, or the service once the wait ran out.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decider {
  Agent,
  Service,
}

/// The call that cut a run, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cut {
  pub by: Decider,
  pub why: Option<String>,
}

/// Why a run cut on its clock failed, as its failure is recorded: whose call it was, and the agent's
/// reason where it gave one; none where no call cut it.
pub fn cut_cause(cut: Option<&Cut>, clock_ms: i64) -> Option<String> {
  let cut = cut?;

  Some(match cut.by {
    Decider::Agent => format!(
      "The run passed its clock of {} and the agent decided to stop it{}",
      spoken(clock_ms),
      cut.why.as_ref().map_or_else(|| ".".to_owned(), |why| format!(": {why}"))
    ),
    Decider::Service => format!(
      "The run passed its clock of {}, the agent did not answer whether to keep it within {}, and the service stopped it.",
      spoken(clock_ms),
      spoken(ANSWER_WAIT_MS)
    ),
  })
}

/// One run's ask: the owner's private chat it is asked in, the run, its agent, what the agent is told the
/// run is, and the clock's name.
pub struct Ask {
  pub owner_chat: i64,
  pub run: String,
  pub agent: String,
  pub what: String,
  pub clock: String,
}

// One clock hold as the asker reads it back.
#[derive(Clone, Debug)]
struct Hold {
  id: i64,
  what: String,
  clock_ms: i64,
  started_at: i64,
  last_activity_at: Option<i64>,
  last_activity: Option<String>,
  asked_at: i64,
  answer_by: i64,
  instruction_id: Option<i64>,
  verdict: Option<String>,
  keep_until: Option<i64>,
  decided_by: Option<String>,
  why: Option<String>,
}

/// What a duration reads as in an order: whole minutes, seconds, or both.
pub fn spoken(ms: i64) -> String {
  let seconds = (ms + 500).div_euclid(1000);

  if seconds % 60 == 0 {
    return if seconds == 60 { "1 minute".to_owned() } else { format!("{} minutes", seconds / 60) };
  }

  if seconds < 60 {
    return format!("{seconds} seconds");
  }

  format!("{} minutes {} seconds", seconds.div_euclid(60), seconds % 60)
}

fn order_text(hold: &Hold, ask: &Ask, kept_before: Option<&Hold>) -> String {
  let activity = match hold.last_activity_at {
    None => hold.last_activity.clone().unwrap_or_else(|| "nothing recorded".to_owned()),
    Some(at) => format!(
      "{}, {} before this ask: {}",
      iso(at),
      spoken((hold.asked_at - at).max(0)),
      hold.last_activity.as_deref().unwrap_or("a line of unknown kind")
    ),
  };
  let mut lines = vec![
    "[A run reached its clock: keep it or cut it]".to_owned(),
    format!("Ask: #{}", hold.id),
    format!("What: {}", hold.what),
    format!("Agent: {}, run {}", ask.agent, ask.run),
    format!(
      "Running for: {}, since {}",
      spoken((hold.asked_at - hold.started_at).max(0)),
      iso(hold.started_at)
    ),
    format!("Clock: {}, {}", ask.clock, spoken(hold.clock_ms)),
    format!("Last activity: {activity}"),
  ];

  if let Some(kept) = kept_before {
    lines.push(format!(
      "You kept it before, until {}{}, and it is still running.",
      iso(kept.keep_until.unwrap_or(hold.asked_at)),
      kept.why.as_ref().map_or_else(String::new, |why| format!(" ({why})"))
    ));
  }

  lines.extend([
    format!(
      "Nothing has been cut: the run goes on until you answer. With no answer by {} the service cuts it itself and records that it did.",
      iso(hold.answer_by)
    ),
    String::new(),
    "What to do, after reading the procedure overtime:".to_owned(),
    "1. Decide yourself whether to keep it or cut it. Do not ask the owner; this is left to you.".to_owned(),
    format!(
      "2. Answer with `bmo overtime --keep {id} [--for <length>] --why '<one line>'` or `bmo overtime --cut {id} --why '<one line>'`.",
      id = hold.id
    ),
    "3. Anything in the system you see needs fixing is filed as a TODO item and fixed by a job.".to_owned(),
    "4. Then answer <<SKIP>>, unless you cut it or filed an item: then tell the owner so in one short line."
      .to_owned(),
  ]);

  lines.join("\n")
}

const COLUMNS: &str = r#""id", "what", "clock_ms", "started_at", "last_activity_at", "last_activity", "asked_at",
  "answer_by", "instruction_id", "verdict", "keep_until", "decided_by", "why""#;

async fn hold(db: &impl ConnectionTrait, id: i64) -> Result<Option<Hold>, Refusal> {
  let row =
    one(db, &format!(r#"SELECT {COLUMNS} FROM "clock_holds" WHERE "id" = $1"#), vec![id.into()]).await?;

  Ok(
    row
      .map(|row| -> Result<Hold, sea_orm::DbErr> {
        Ok(Hold {
          id: row.try_get("", "id")?,
          what: row.try_get("", "what")?,
          clock_ms: row.try_get("", "clock_ms")?,
          started_at: row.try_get("", "started_at")?,
          last_activity_at: row.try_get("", "last_activity_at")?,
          last_activity: row.try_get("", "last_activity")?,
          asked_at: row.try_get("", "asked_at")?,
          answer_by: row.try_get("", "answer_by")?,
          instruction_id: row.try_get("", "instruction_id")?,
          verdict: row.try_get("", "verdict")?,
          keep_until: row.try_get("", "keep_until")?,
          decided_by: row.try_get("", "decided_by")?,
          why: row.try_get("", "why")?,
        })
      })
      .transpose()?,
  )
}

fn gone() -> Refusal {
  sea_orm::DbErr::RecordNotFound("the clock's ask is gone".into()).into()
}

async fn close(db: &impl ConnectionTrait, id: i64, closed: &str, at: i64) -> Result<(), Refusal> {
  run(
    db,
    r#"UPDATE "clock_holds" SET "closed" = $1, "closed_at" = $2 WHERE "id" = $3 AND "closed" IS NULL"#,
    vec![closed.into(), at.into(), id.into()],
  )
  .await?;

  Ok(())
}

/// How long a wait sized on a run's clock is to go on standing: until the moment the run's open ask holds
/// it to — its answer's deadline while the agent has not answered, the end of the keep once it has — or
/// none where no ask on the run stands, or the one standing was answered with a cut.
pub async fn standing_until(db: &impl ConnectionTrait, run_name: &str) -> Result<Option<i64>, Refusal> {
  let row = one(
    db,
    r#"SELECT MAX(CASE WHEN "verdict" = 'keep' THEN COALESCE("keep_until", "answer_by") ELSE "answer_by" END) AS "until"
       FROM "clock_holds" WHERE "closed" IS NULL AND "run" = $1 AND ("verdict" IS NULL OR "verdict" <> 'cut')"#,
    vec![run_name.into()],
  )
  .await?;

  Ok(row.map(|row| row.try_get::<Option<i64>>("", "until")).transpose()?.flatten())
}

/// The policy a turn of work is handed as its overtime: the ask it has open, the one kept before it, and
/// the call that cut it where one did.
pub struct Asker<'a, C> {
  db: &'a C,
  units: &'a dyn Units,
  ask: Ask,
  current: Option<i64>,
  kept: Option<Hold>,
  cut: Option<Cut>,
}

impl<'a, C: ConnectionTrait> Asker<'a, C> {
  pub fn new(db: &'a C, units: &'a dyn Units, ask: Ask) -> Self {
    Self {
      db,
      units,
      ask,
      current: None,
      kept: None,
      cut: None,
    }
  }

  /// The call that cut the run, where one did.
  pub fn cut_by(&self) -> Option<&Cut> {
    self.cut.as_ref()
  }

  // An order the agent never took is withdrawn once there is nothing left for it to decide.
  async fn withdraw(&self, hold: &Hold) -> Result<(), Refusal> {
    if let Some(instruction) = hold.instruction_id
      && self.units.instruction_state(instruction).await? == Some(InstructionState::Pending)
    {
      self.units.withdraw_instruction(instruction).await?;
    }

    Ok(())
  }

  async fn open(&mut self, overdue: &Overrun) -> Result<Overtime, Refusal> {
    let at = now_ms();
    let id: i64 = one(
      self.db,
      r#"INSERT INTO "clock_holds" ("run", "agent", "what", "clock", "clock_ms", "started_at", "last_activity_at",
           "last_activity", "asked_at", "answer_by")
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING "id""#,
      vec![
        self.ask.run.as_str().into(),
        self.ask.agent.as_str().into(),
        self.ask.what.as_str().into(),
        self.ask.clock.as_str().into(),
        overdue.bound_ms.into(),
        overdue.started_at.into(),
        overdue.last_activity_at.into(),
        overdue.last_activity.clone().into(),
        at.into(),
        (at + ANSWER_WAIT_MS).into(),
      ],
    )
    .await?
    .ok_or_else(gone)?
    .try_get("", "id")?;
    let opened = hold(self.db, id).await?.ok_or_else(gone)?;
    let instruction = Instruction {
      agent: None,
      chat: Some(self.ask.owner_chat),
      said: Said::Order(order_text(&opened, &self.ask, self.kept.as_ref())),
      kind: Some(format!("overtime:{}", self.ask.run)),
      sort: OVERTIME_SORT.to_owned(),
      summary: None,
      expires_at: None,
      events: vec![],
      rides_until: None,
      via: None,
      run: None,
    };
    let queued = self.units.queue_instruction(&instruction).await?;

    run(
      self.db,
      r#"UPDATE "clock_holds" SET "instruction_id" = $1 WHERE "id" = $2"#,
      vec![queued.into(), id.into()],
    )
    .await?;
    self.current = Some(id);

    Ok(Overtime::AskAgainAt(at + LOOK_MS))
  }

  async fn cut_on(&mut self, held: &Hold, by: Decider, why: Option<String>) -> Result<Overtime, Refusal> {
    close(self.db, held.id, "cut", now_ms()).await?;
    self.withdraw(held).await?;
    self.cut = Some(Cut { by, why });
    self.current = None;

    Ok(Overtime::Cut)
  }

  /// What the turn past its bound is told: go on and look again, or be cut.
  pub async fn overtime(&mut self, overdue: &Overrun) -> Result<Overtime, Refusal> {
    let Some(current) = self.current else { return self.open(overdue).await };
    let Some(held) = hold(self.db, current).await? else { return Ok(Overtime::Cut) };
    let at = now_ms();

    if held.verdict.as_deref() == Some("cut") {
      let by = if held.decided_by.as_deref() == Some("service") {
        Decider::Service
      } else {
        Decider::Agent
      };

      return self.cut_on(&held, by, held.why.clone()).await;
    }

    if held.verdict.as_deref() == Some("keep")
      && let Some(until) = held.keep_until
    {
      if at < until {
        return Ok(Overtime::AskAgainAt(until.min(at + LOOK_MS)));
      }

      close(self.db, held.id, "kept", at).await?;
      self.kept = Some(held);
      self.current = None;

      return self.open(overdue).await;
    }

    if at >= held.answer_by {
      let why = format!("no answer from the agent within {}", spoken(held.answer_by - held.asked_at));

      run(
        self.db,
        r#"UPDATE "clock_holds" SET "verdict" = 'cut', "keep_until" = NULL, "decided_by" = 'service', "why" = $1, "decided_at" = $2
           WHERE "id" = $3 AND "closed" IS NULL"#,
        vec![why.as_str().into(), at.into(), held.id.into()],
      )
      .await?;

      return self.cut_on(&held, Decider::Service, Some(why)).await;
    }

    Ok(Overtime::AskAgainAt(held.answer_by.min(at + LOOK_MS)))
  }

  /// The ask a turn ended under on its own closed, and its order withdrawn where the agent never took it.
  pub async fn finish(&mut self) -> Result<(), Refusal> {
    let Some(current) = self.current.take() else { return Ok(()) };
    let held = hold(self.db, current).await?;

    close(self.db, current, "finished", now_ms()).await?;

    if let Some(held) = held {
      self.withdraw(&held).await?;
    }

    Ok(())
  }
}
