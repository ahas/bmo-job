//! The questions jobs put to the owner, as the `job_questions` and `question_talk` tables hold them
//! (docs/queue.md, "A job's question"): the row opened, read, talked over and settled, the fact the room
//! is told at each step, and the wait the asker sits through until something settles it. The first
//! settlement is the one that stands, so each write here is conditional on the row's state and says
//! whether it landed.

use std::time::Duration;

use job_codes::QuestionFact;
use job_codes::job_questions::Lapse;
use sea_orm::{ConnectionTrait, QueryResult, TransactionTrait};
use tokio::sync::watch;

use crate::{Refusal, all, now_ms, one, run};

/// Who settles a question by answering it.
pub const SETTLED_BY_OWNER: &str = "owner";

/// What settles a question whose wait ran out.
pub const SETTLED_BY_DEADLINE: &str = "deadline";

/// Who says a line of talk that is the asking job's.
pub const TALK_BY_JOB: &str = "job";

/// What a question was settled on: one of its options by number from 0, or the owner's own words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Answer {
  Chosen(i64),
  Words(String),
}

/// One question as its row holds it. Its times are epoch milliseconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Question {
  pub id: i64,
  pub job_id: Option<i64>,
  pub todo_id: i64,
  pub chat_id: i64,
  pub question: String,
  pub options: Vec<String>,
  pub recommended: i64,
  pub why: String,
  pub asked_at: i64,
  pub due_at: i64,
  pub chosen: Option<i64>,
  pub answer: Option<String>,
  pub paused_left: Option<i64>,
  pub settled_at: Option<i64>,
  pub settled_by: Option<String>,
  /// Whether the owner may talk it over with the asker rather than only choose.
  pub talks: bool,
}

fn count(value: i64) -> u32 {
  u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

fn seconds(ms: i64) -> u64 {
  u64::try_from((ms.max(0) + 500) / 1000).unwrap_or(0)
}

impl Question {
  fn of(row: &QueryResult) -> Result<Self, sea_orm::DbErr> {
    Ok(Self {
      id: row.try_get("", "id")?,
      job_id: row.try_get("", "job_id")?,
      todo_id: row.try_get("", "todo_id")?,
      chat_id: row.try_get("", "chat_id")?,
      question: row.try_get("", "question")?,
      // The options are held one to a line.
      options: row.try_get::<String>("", "options")?.split('\n').map(str::to_owned).collect(),
      recommended: row.try_get("", "recommended")?,
      why: row.try_get("", "why")?,
      asked_at: row.try_get("", "asked_at")?,
      due_at: row.try_get("", "due_at")?,
      chosen: row.try_get("", "chosen")?,
      answer: row.try_get("", "answer")?,
      paused_left: row.try_get("", "paused_left")?,
      settled_at: row.try_get("", "settled_at")?,
      settled_by: row.try_get("", "settled_by")?,
      talks: row.try_get::<i64>("", "talks")? != 0,
    })
  }

  /// What the question was settled on, as words: the owner's own, or the option taken. It is the one line
  /// the asking job carries on with, the same shape however the question was answered.
  pub fn answer_text(&self) -> String {
    if let Some(words) = &self.answer {
      return words.clone();
    }

    self.option(self.chosen.unwrap_or(self.recommended))
  }

  fn option(&self, at: i64) -> String {
    usize::try_from(at)
      .ok()
      .and_then(|at| self.options.get(at))
      .cloned()
      .unwrap_or_default()
  }

  /// What the room is told when the question is put: its options with the recommendation, the one line
  /// of why it is being asked at all, and how long it waits. `removed_room` names the room it was filed
  /// from where that room is gone and the owner's chat is told instead.
  pub fn asked(&self, removed_room: Option<String>) -> QuestionFact {
    QuestionFact::QuestionAsked {
      item: self.todo_id,
      question_id: self.id,
      question: self.question.clone(),
      options: self.options.clone(),
      recommended: count(self.recommended + 1),
      why: self.why.clone(),
      wait_seconds: seconds(self.due_at - self.asked_at),
      talk: self.talks,
      removed_room,
    }
  }

  /// What the room is told the first time the owner talks instead of choosing: the clock has stopped while
  /// the job answers, so a silence while it writes is not read as the minute running.
  pub fn talk_held(&self) -> QuestionFact {
    QuestionFact::QuestionTalkHeld { item: self.todo_id }
  }

  /// The job's own words in the talk, and, where the talk changed the choice, the options put again with
  /// the hold one line of talk gives.
  pub fn talk_line(&self, said: &str, revised: bool, hold_ms: i64) -> Vec<QuestionFact> {
    let mut told = vec![QuestionFact::JobReplied {
      item: self.todo_id,
      said: said.to_owned(),
    }];

    if revised {
      told.push(QuestionFact::QuestionRevised {
        item: self.todo_id,
        options: self.options.clone(),
        recommended: count(self.recommended + 1),
        wait_seconds: seconds(hold_ms),
      });
    }

    told
  }

  /// What the room is told once the question is settled, whichever way it went: the owner's own answer,
  /// or the wait that ran out and the recommendation it went with, saying which silence it was. A talk
  /// that went silent is one silence and a stop the owner asked for another; each ran for the hold
  /// `stop_ms` names.
  pub fn settled(&self, talked: bool, stop_ms: i64) -> QuestionFact {
    let chosen = self.chosen.unwrap_or(self.recommended);
    let (option, named) = (count(chosen + 1), self.option(chosen));

    if self.settled_by.as_deref() == Some(SETTLED_BY_OWNER) {
      return match &self.answer {
        None => QuestionFact::QuestionChosen {
          item: self.todo_id,
          option,
          named,
        },
        Some(answer) => QuestionFact::QuestionTyped {
          item: self.todo_id,
          answer: answer.clone(),
        },
      };
    }

    let (lapsed, wait_ms) = match (talked, self.paused_left.is_some()) {
      (true, true) => (Lapse::Talking, stop_ms),
      (false, true) => (Lapse::Stopped, stop_ms),
      _ => (Lapse::Waiting, self.due_at - self.asked_at),
    };

    QuestionFact::QuestionLapsed {
      item: self.todo_id,
      lapsed,
      option,
      named,
      wait_seconds: seconds(wait_ms),
    }
  }
}

/// One line said over a question.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Talk {
  pub id: i64,
  pub by: String,
  pub text: String,
}

/// A question as it is to be opened; its times are the wait's own.
pub struct NewQuestion<'a> {
  pub job_id: Option<i64>,
  pub todo_id: i64,
  pub chat_id: i64,
  pub question: &'a str,
  pub options: &'a [String],
  pub recommended: i64,
  pub why: &'a str,
  pub asked_at: i64,
  pub due_at: i64,
  pub talks: bool,
}

/// The question opened before it is asked, so it is on record from the moment it exists, read back.
pub async fn open(db: &impl ConnectionTrait, new: &NewQuestion<'_>) -> Result<Question, Refusal> {
  let id: i64 = one(
    db,
    r#"INSERT INTO "job_questions" ("job_id", "todo_id", "chat_id", "question", "options", "recommended", "why", "asked_at", "due_at", "talks")
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING "id""#,
    vec![
      new.job_id.into(),
      new.todo_id.into(),
      new.chat_id.into(),
      new.question.into(),
      new.options.join("\n").into(),
      new.recommended.into(),
      new.why.into(),
      new.asked_at.into(),
      new.due_at.into(),
      i64::from(new.talks).into(),
    ],
  )
  .await?
  .map_or(Ok(0), |row| row.try_get("", "id"))?;

  question(db, id).await?.ok_or_else(gone)
}

fn gone() -> Refusal {
  sea_orm::DbErr::RecordNotFound("the question is gone".into()).into()
}

pub async fn question(db: &impl ConnectionTrait, id: i64) -> Result<Option<Question>, Refusal> {
  let row = one(db, r#"SELECT * FROM "job_questions" WHERE "id" = $1"#, vec![id.into()]).await?;

  Ok(row.as_ref().map(Question::of).transpose()?)
}

/// The owner's answer written at `now`, where nothing settled the question first.
pub async fn settle(
  db: &impl ConnectionTrait,
  id: i64,
  answer: &Answer,
  now: i64,
) -> Result<bool, Refusal> {
  let (chosen, words) = match answer {
    Answer::Chosen(chosen) => (Some(*chosen), None),
    Answer::Words(words) => (None, Some(words.clone())),
  };
  let settled = run(
    db,
    r#"UPDATE "job_questions" SET "chosen" = $1, "answer" = $2, "settled_at" = $3, "settled_by" = $4
       WHERE "id" = $5 AND "settled_at" IS NULL"#,
    vec![chosen.into(), words.into(), now.into(), SETTLED_BY_OWNER.into(), id.into()],
  )
  .await?;

  Ok(settled > 0)
}

/// The wait running out at `now`, settled on the recommendation. It lands only on a row whose due time has
/// actually come: a stop that reached the row a moment before moved that time on, and the asker that lost
/// the race to it goes back to waiting instead of settling over it.
pub async fn settle_unanswered(
  db: &impl ConnectionTrait,
  id: i64,
  chosen: i64,
  now: i64,
) -> Result<bool, Refusal> {
  let settled = run(
    db,
    r#"UPDATE "job_questions" SET "chosen" = $1, "answer" = NULL, "settled_at" = $2, "settled_by" = $3
       WHERE "id" = $4 AND "settled_at" IS NULL AND "due_at" <= $2"#,
    vec![chosen.into(), now.into(), SETTLED_BY_DEADLINE.into(), id.into()],
  )
  .await?;

  Ok(settled > 0)
}

/// The clock stopped at `now` for `stop_ms`: what the countdown had left is kept, and the due time moves
/// to the end of the stop. Only a running countdown on an open question stops.
pub async fn pause(db: &impl ConnectionTrait, id: i64, stop_ms: i64, now: i64) -> Result<bool, Refusal> {
  let paused = run(
    db,
    r#"UPDATE "job_questions" SET "paused_left" = "due_at" - $1, "due_at" = $2
       WHERE "id" = $3 AND "settled_at" IS NULL AND "paused_left" IS NULL AND "due_at" > $1"#,
    vec![now.into(), (now + stop_ms).into(), id.into()],
  )
  .await?;

  Ok(paused > 0)
}

/// The clock started again at `now`, counting down what it had when it was stopped.
pub async fn resume(db: &impl ConnectionTrait, id: i64, now: i64) -> Result<bool, Refusal> {
  let resumed = run(
    db,
    r#"UPDATE "job_questions" SET "due_at" = $1 + "paused_left", "paused_left" = NULL
       WHERE "id" = $2 AND "settled_at" IS NULL AND "paused_left" IS NOT NULL"#,
    vec![now.into(), id.into()],
  )
  .await?;

  Ok(resumed > 0)
}

/// One line of talk over an open question at `now`, and the clock held for it: stopped where it ran,
/// keeping what it had as a stop from the console keeps it, and its end moved to a hold of `hold_ms`
/// counted from this line where that is later. Nothing is written on a settled question, and whether the
/// line landed comes back. A line the job says is heard as it is said; the owner's waits for the asker to
/// hear it.
pub async fn talk(
  db: &impl TransactionTrait,
  id: i64,
  by: &str,
  text: &str,
  hold_ms: i64,
  now: i64,
) -> Result<bool, Refusal> {
  let unit = db.begin().await?;
  let held = run(
    &unit,
    r#"UPDATE "job_questions"
       SET "paused_left" = COALESCE("paused_left", CASE WHEN "due_at" > $1 THEN "due_at" - $1 ELSE 0 END),
           "due_at" = CASE WHEN "due_at" > $2 THEN "due_at" ELSE $2 END
       WHERE "id" = $3 AND "settled_at" IS NULL"#,
    vec![now.into(), (now + hold_ms).into(), id.into()],
  )
  .await?;

  if held == 0 {
    return Ok(false);
  }

  run(
    &unit,
    r#"INSERT INTO "question_talk" ("question_id", "by", "text", "said_at", "heard_at") VALUES ($1, $2, $3, $4, $5)"#,
    vec![id.into(), by.into(), text.into(), now.into(), (by == TALK_BY_JOB).then_some(now).into()],
  )
  .await?;
  unit.commit().await?;

  Ok(true)
}

fn talk_of(row: &QueryResult) -> Result<Talk, sea_orm::DbErr> {
  Ok(Talk {
    id: row.try_get("", "id")?,
    by: row.try_get("", "by")?,
    text: row.try_get("", "text")?,
  })
}

/// The owner's lines the asker has not been handed yet, oldest first, marked heard at `now` as they are
/// handed over by the one statement that returns them, so a line is never handed twice and one written
/// meanwhile is left for the next hearing.
pub async fn hear_talk(db: &impl ConnectionTrait, id: i64, now: i64) -> Result<Vec<Talk>, Refusal> {
  let rows = all(
    db,
    r#"UPDATE "question_talk" SET "heard_at" = $1 WHERE "question_id" = $2 AND "heard_at" IS NULL RETURNING *"#,
    vec![now.into(), id.into()],
  )
  .await?;
  let mut heard = rows.iter().map(talk_of).collect::<Result<Vec<_>, _>>()?;

  heard.sort_by_key(|line| line.id);

  Ok(heard)
}

/// Whether anything was said over the question at all.
pub async fn talked(db: &impl ConnectionTrait, id: i64) -> Result<bool, Refusal> {
  Ok(
    one(
      db,
      r#"SELECT 1 AS "said" FROM "question_talk" WHERE "question_id" = $1 LIMIT 1"#,
      vec![id.into()],
    )
    .await?
    .is_some(),
  )
}

/// The question put again in other words after talk changed it; only an open question changes.
pub async fn revise(
  db: &impl ConnectionTrait,
  id: i64,
  question: &str,
  options: &[String],
  recommended: i64,
  why: &str,
) -> Result<bool, Refusal> {
  let revised = run(
    db,
    r#"UPDATE "job_questions" SET "question" = $1, "options" = $2, "recommended" = $3, "why" = $4
       WHERE "id" = $5 AND "settled_at" IS NULL"#,
    vec![question.into(), options.join("\n").into(), recommended.into(), why.into(), id.into()],
  )
  .await?;

  Ok(revised > 0)
}

/// How a wait ended: the question settled, by the asker's own stop or otherwise, or the owner talked and
/// the question is still open, with what he said that the asker had not been handed yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Waited {
  Settled {
    question: Question,
    stopped: bool,
    talked: bool,
  },
  Talk {
    question: Question,
    heard: Vec<Talk>,
  },
}

/// An asker's own reason to stop waiting before anything settled the question: once the signal reads
/// true, the row is settled as the owner's with these words.
pub struct WaitStop<'a> {
  pub signal: watch::Receiver<bool>,
  pub answer: &'a str,
}

/// The row read again every `poll` until something settles it or the owner talks. A stop or a talk moves
/// the row's due time on and never earlier, so the later of it and the asker's own deadline is the time to
/// settle by. The settled row is read back rather than assumed: an answer that landed in the last moment is
/// what settled it.
pub async fn wait_on(
  db: &(impl ConnectionTrait + TransactionTrait),
  id: i64,
  deadline: i64,
  mut stop: Option<WaitStop<'_>>,
  poll: Duration,
) -> Result<Waited, Refusal> {
  let mut stopped = false;

  loop {
    let Some(row) = question(db, id).await? else { break };

    if row.settled_at.is_some() {
      break;
    }

    if let Some(stop) = stop.as_ref().filter(|stop| *stop.signal.borrow()) {
      stopped = settle(db, id, &Answer::Words(stop.answer.to_owned()), now_ms()).await?;

      if stopped {
        break;
      }

      continue;
    }

    let heard = if row.talks { hear_talk(db, id, now_ms()).await? } else { vec![] };

    if !heard.is_empty() {
      let question = question(db, id).await?.unwrap_or(row);

      return Ok(Waited::Talk { question, heard });
    }

    let at = now_ms();

    if at >= deadline.max(row.due_at) && settle_unanswered(db, id, row.recommended, at).await? {
      break;
    }

    match stop.as_mut() {
      Some(stop) => {
        tokio::select! {
          () = tokio::time::sleep(poll) => {}
          _ = stop.signal.changed() => {}
        }
      }
      None => tokio::time::sleep(poll).await,
    }
  }

  let question = question(db, id).await?.ok_or_else(gone)?;
  let talked = talked(db, id).await?;

  Ok(Waited::Settled {
    question,
    stopped,
    talked,
  })
}
