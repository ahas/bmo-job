//! Questions, their clocks and their replies (docs/api.md, "Questions and replies").

use std::collections::HashSet;

use bmo_job_client::{
  Code, EventKind, NewQuestion, NewReply, Page, Question, QuestionQuery, QuestionState, Reply,
};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::events::{EventKeys, emit, key_alone};
use super::items::find_item;
use super::{Store, Tx, filled, from_json, listed, page_limit, paged, stored, to_json};
use crate::refusal::Refusal;

const QUESTION_COLUMNS: &str = "id, item_id, asker_name, scope, text, choices, state, due_at, remaining_ms, due_told, \
  created_at";

/// A question as it is kept: the member, and the time its clock had left when it was paused.
struct Kept {
  question: Question,
  remaining_ms: Option<i64>,
}

fn kept_from(row: &SqliteRow) -> Result<Kept, Refusal> {
  Ok(Kept {
    question: Question {
      id: row.try_get("id")?,
      item_id: row.try_get("item_id")?,
      asker_name: row.try_get("asker_name")?,
      scope: row.try_get("scope")?,
      text: row.try_get("text")?,
      choices: from_json(row.try_get::<&str, _>("choices")?)?,
      state: stored(row.try_get("state")?, QuestionState::parse)?,
      due_at: row.try_get("due_at")?,
      created_at: row.try_get("created_at")?,
    },
    remaining_ms: row.try_get("remaining_ms")?,
  })
}

fn reply_from(row: &SqliteRow) -> Result<Reply, Refusal> {
  Ok(Reply {
    id: row.try_get("id")?,
    question_id: row.try_get("question_id")?,
    choice: row.try_get("choice")?,
    text: row.try_get("text")?,
    actor_name: row.try_get("actor_name")?,
    settles: row.try_get::<i64, _>("settles")? != 0,
    created_at: row.try_get("created_at")?,
  })
}

async fn kept_at(tx: &mut Tx, id: i64) -> Result<Kept, Refusal> {
  let row = sqlx::query(&format!("SELECT {QUESTION_COLUMNS} FROM questions WHERE id = $1"))
    .bind(id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Refusal::not_found(format!("There is no question {id}.")))?;
  kept_from(&row)
}

/// What an event about the question, or a reply to it, is narrowed by.
pub(crate) async fn question_keys(tx: &mut Tx, question: &Question) -> Result<EventKeys, Refusal> {
  let queue_name = match question.item_id {
    Some(item_id) => find_item(tx, item_id).await?.map(|item| item.queue_name),
    None => None,
  };
  Ok(EventKeys {
    queue_name,
    scope: question.scope.clone(),
    item_id: question.item_id,
  })
}

async fn save_question(tx: &mut Tx, kept: &Kept, due_told: bool) -> Result<(), Refusal> {
  sqlx::query(
    "UPDATE questions SET state = $1, due_at = $2, remaining_ms = $3, due_told = $4 WHERE id = $5",
  )
  .bind(kept.question.state.as_str())
  .bind(kept.question.due_at)
  .bind(kept.remaining_ms)
  .bind(i64::from(due_told))
  .bind(kept.question.id)
  .execute(&mut **tx)
  .await?;
  Ok(())
}

async fn told_question(tx: &mut Tx, at: i64, kind: EventKind, question: &Question) -> Result<(), Refusal> {
  let keys = question_keys(tx, question).await?;
  emit(tx, at, kind.as_str(), question, &keys).await?;
  Ok(())
}

/// Removes a question and its replies, telling it as deleted.
pub(crate) async fn delete_question(tx: &mut Tx, id: i64, now: i64) -> Result<(), Refusal> {
  let kept = kept_at(tx, id).await?;
  let keys = question_keys(tx, &kept.question).await?;
  sqlx::query("DELETE FROM replies WHERE question_id = $1")
    .bind(id)
    .execute(&mut **tx)
    .await?;
  sqlx::query("DELETE FROM questions WHERE id = $1").bind(id).execute(&mut **tx).await?;
  emit(tx, now, EventKind::QuestionDeleted.as_str(), &key_alone(id, None), &keys).await?;
  Ok(())
}

/// Tells once of each open question whose due time has passed by `now`.
pub(crate) async fn tell_due_questions(tx: &mut Tx, now: i64) -> Result<usize, Refusal> {
  let rows = sqlx::query(&format!(
    "SELECT {QUESTION_COLUMNS} FROM questions WHERE state = 'open' AND due_told = 0 AND due_at IS NOT NULL \
     AND due_at <= $1"
  ))
  .bind(now)
  .fetch_all(&mut **tx)
  .await?;
  for row in &rows {
    let kept = kept_from(row)?;
    save_question(tx, &kept, true).await?;
    told_question(tx, now, EventKind::QuestionDue, &kept.question).await?;
  }
  Ok(rows.len())
}

impl Store {
  /// Questions, filtered as the query asks.
  pub async fn questions(&self, query: &QuestionQuery) -> Result<Page<Question>, Refusal> {
    let limit = page_limit(query.limit)?;
    let states = listed("state", query.state.as_deref(), QuestionState::parse)?;
    let states: Vec<&str> = states.iter().map(|state| state.as_str()).collect();
    let mut tx = self.begin().await?;
    let rows = sqlx::query(&format!(
      "SELECT {QUESTION_COLUMNS} FROM questions WHERE id > $1 AND ($2 IS NULL OR scope = $2) \
       AND ($3 IS NULL OR item_id = $3) AND ($4 = '[]' OR state IN (SELECT value FROM json_each($4))) \
       ORDER BY id LIMIT $5"
    ))
    .bind(query.after.unwrap_or(0))
    .bind(&query.scope)
    .bind(query.item_id)
    .bind(to_json(&states))
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let questions = rows
      .iter()
      .map(|row| kept_from(row).map(|kept| kept.question))
      .collect::<Result<Vec<_>, _>>()?;
    Ok(paged(questions, limit))
  }

  /// How many questions under `scope` still wait for an answer: open or paused.
  pub async fn questions_waiting_in(&self, scope: &str) -> Result<i64, Refusal> {
    let mut tx = self.begin().await?;
    let count = sqlx::query_scalar("SELECT count(*) FROM questions WHERE scope = $1 AND state IN ($2, $3)")
      .bind(scope)
      .bind(QuestionState::Open.as_str())
      .bind(QuestionState::Paused.as_str())
      .fetch_one(&mut *tx)
      .await?;
    tx.commit().await?;
    Ok(count)
  }

  /// The questions under `scope` still waiting at `now`, the one asked last first: those open with no due
  /// time or one not yet passed, and those paused.
  pub async fn open_questions_in(&self, scope: &str, now: i64) -> Result<Vec<Question>, Refusal> {
    let mut tx = self.begin().await?;
    let rows = sqlx::query(&format!(
      "SELECT {QUESTION_COLUMNS} FROM questions WHERE scope = $1 \
       AND (state = $2 OR (state = $3 AND (due_at IS NULL OR due_at > $4))) ORDER BY id DESC"
    ))
    .bind(scope)
    .bind(QuestionState::Paused.as_str())
    .bind(QuestionState::Open.as_str())
    .bind(now)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter().map(|row| kept_from(row).map(|kept| kept.question)).collect()
  }

  /// Asks a question, its scope the item's where none is given.
  pub async fn ask(&self, new: NewQuestion) -> Result<Question, Refusal> {
    filled("askerName", &new.asker_name)?;
    filled("text", &new.text)?;
    let mut seen = HashSet::new();
    for choice in &new.choices {
      filled("choice", choice)?;
      if !seen.insert(choice.as_str()) {
        return Err(Refusal::bad_request(format!("The choice {choice:?} stands twice.")));
      }
    }
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut scope = new.scope;
    if let Some(item_id) = new.item_id {
      let Some(item) = find_item(&mut tx, item_id).await? else {
        return Err(Refusal::new(Code::UnknownItem, format!("There is no item {item_id}.")));
      };
      if scope.is_none() {
        scope = item.scope;
      }
    }
    let id: i64 = sqlx::query_scalar(
      "INSERT INTO questions (item_id, asker_name, scope, text, choices, state, due_at, due_told, created_at) \
       VALUES ($1, $2, $3, $4, $5, 'open', $6, 0, $7) RETURNING id",
    )
    .bind(new.item_id)
    .bind(&new.asker_name)
    .bind(&scope)
    .bind(&new.text)
    .bind(to_json(&new.choices))
    .bind(new.due_at)
    .bind(now)
    .fetch_one(&mut *tx)
    .await?;
    let question = kept_at(&mut tx, id).await?.question;
    told_question(&mut tx, now, EventKind::QuestionCreated, &question).await?;
    self.commit(tx).await?;
    Ok(question)
  }

  pub async fn question(&self, id: i64) -> Result<Question, Refusal> {
    let mut tx = self.begin().await?;
    let question = kept_at(&mut tx, id).await?.question;
    tx.commit().await?;
    Ok(question)
  }

  /// Pauses a question's clock, starts it again, or withdraws the question.
  pub async fn change_question(&self, id: i64, state: QuestionState) -> Result<Question, Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut kept = kept_at(&mut tx, id).await?;
    let from = kept.question.state;
    if from == state {
      tx.commit().await?;
      return Ok(kept.question);
    }
    let mut due_told = false;
    match (from, state) {
      (QuestionState::Open, QuestionState::Paused) => {
        kept.remaining_ms = kept.question.due_at.map(|due| (due - now).max(0));
        kept.question.due_at = None;
      }
      (QuestionState::Paused, QuestionState::Open) => {
        kept.question.due_at = kept.remaining_ms.map(|left| now + left);
        kept.remaining_ms = None;
      }
      (QuestionState::Open | QuestionState::Paused, QuestionState::Withdrawn) => due_told = true,
      _ => return Err(Refusal::conflict(format!("A question {from} cannot move to {state}."))),
    }
    kept.question.state = state;
    save_question(&mut tx, &kept, due_told).await?;
    told_question(&mut tx, now, EventKind::QuestionChanged, &kept.question).await?;
    self.commit(tx).await?;
    Ok(kept.question)
  }

  /// The question's replies past `after`.
  pub async fn replies(
    &self,
    question_id: i64,
    after: Option<i64>,
    limit: Option<i64>,
  ) -> Result<Page<Reply>, Refusal> {
    let limit = page_limit(limit)?;
    let mut tx = self.begin().await?;
    kept_at(&mut tx, question_id).await?;
    let rows = sqlx::query(
      "SELECT id, question_id, choice, text, actor_name, settles, created_at FROM replies \
       WHERE question_id = $1 AND id > $2 ORDER BY id LIMIT $3",
    )
    .bind(question_id)
    .bind(after.unwrap_or(0))
    .bind(limit + 1)
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    let replies = rows.iter().map(reply_from).collect::<Result<Vec<_>, _>>()?;
    Ok(paged(replies, limit))
  }

  /// Replies to a question: a settling reply answers it, a word is added in any state.
  pub async fn reply(&self, question_id: i64, new: NewReply) -> Result<Reply, Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let reply = reply_in(&mut tx, question_id, new, now).await?;
    self.commit(tx).await?;
    Ok(reply)
  }

  /// Writes a line of talk on a question: its clock is stopped where it runs, keeping what it had left, and
  /// the words are a reply that does not settle. A question already settled takes no talk and is refused
  /// `conflict`.
  pub async fn talk(&self, question_id: i64, text: String, actor_name: String) -> Result<Reply, Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut kept = kept_at(&mut tx, question_id).await?;
    if !matches!(kept.question.state, QuestionState::Open | QuestionState::Paused) {
      return Err(Refusal::conflict(format!("The question is {} and takes no talk.", kept.question.state)));
    }
    if kept.question.state == QuestionState::Open {
      kept.remaining_ms = kept.question.due_at.map(|due| (due - now).max(0));
      kept.question.due_at = None;
      kept.question.state = QuestionState::Paused;
      save_question(&mut tx, &kept, false).await?;
      told_question(&mut tx, now, EventKind::QuestionChanged, &kept.question).await?;
    }
    let reply = reply_in(
      &mut tx,
      question_id,
      NewReply {
        choice: None,
        text,
        actor_name,
        settles: Some(false),
      },
      now,
    )
    .await?;
    self.commit(tx).await?;
    Ok(reply)
  }
}

/// Writes a reply to a question in the transaction given: a settling reply answers it, a word is added in any
/// state.
async fn reply_in(tx: &mut Tx, question_id: i64, new: NewReply, now: i64) -> Result<Reply, Refusal> {
  filled("text", &new.text)?;
  filled("actorName", &new.actor_name)?;
  let mut kept = kept_at(tx, question_id).await?;
  if let Some(choice) = &new.choice
    && !kept.question.choices.contains(choice)
  {
    return Err(Refusal::new(
      Code::NotAChoice,
      format!("{choice:?} is not one of the question's choices."),
    ));
  }
  let settles = new.settles.unwrap_or(new.choice.is_some() || kept.question.choices.is_empty());
  if settles && !matches!(kept.question.state, QuestionState::Open | QuestionState::Paused) {
    return Err(Refusal::conflict(format!(
      "The question is {} and takes no answer.",
      kept.question.state
    )));
  }
  let id: i64 = sqlx::query_scalar(
    "INSERT INTO replies (question_id, choice, text, actor_name, settles, created_at) \
     VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
  )
  .bind(question_id)
  .bind(&new.choice)
  .bind(&new.text)
  .bind(&new.actor_name)
  .bind(i64::from(settles))
  .bind(now)
  .fetch_one(&mut **tx)
  .await?;
  let reply = Reply {
    id,
    question_id,
    choice: new.choice,
    text: new.text,
    actor_name: new.actor_name,
    settles,
    created_at: now,
  };
  let keys = question_keys(tx, &kept.question).await?;
  emit(tx, now, EventKind::ReplyCreated.as_str(), &reply, &keys).await?;
  if settles {
    kept.question.state = QuestionState::Answered;
    kept.remaining_ms = None;
    save_question(tx, &kept, true).await?;
    told_question(tx, now, EventKind::QuestionChanged, &kept.question).await?;
  }
  Ok(reply)
}
