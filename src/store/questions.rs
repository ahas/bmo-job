//! Questions, their clocks and their replies (docs/api.md, "Questions and replies").

use std::collections::HashSet;

use bmo_job_client::{
  Code, EventKind, NewQuestion, NewReply, Page, Question, QuestionAddressee, QuestionChange, QuestionQuery,
  QuestionRecommendation, QuestionState, Reply, ReplySource,
};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::events::{EventKeys, emit, key_alone};
use super::items::find_item;
use super::{Store, Tx, filled, from_json, listed, page_limit, paged, stored, to_json};
use crate::access::Caller;
use crate::refusal::Refusal;

const QUESTION_COLUMNS: &str = "id, item_id, asker_name, scope, addressee, text, choices, recommendation, state, \
  due_at, remaining_ms, due_told, told_by_asker, heard_through, wait_ms, readdressed_by, readdressed_at, told_as, \
  created_at";

/// The wait a question gets where the asker names none, and the least and longest it may name.
const DEFAULT_ASK_WAIT_MS: i64 = 60_000;
const LEAST_ASK_WAIT_MS: i64 = 1_000;
const LONGEST_ASK_WAIT_MS: i64 = 86_400_000;

/// A question as it is kept: the member, the time its clock had left when it was paused, and the wait it was
/// asked with, which its clock takes when the supervisor puts it to a person.
struct Kept {
  question: Question,
  remaining_ms: Option<i64>,
  due_told: bool,
  wait_ms: i64,
}

fn kept_from(row: &SqliteRow) -> Result<Kept, Refusal> {
  Ok(Kept {
    question: Question {
      id: row.try_get("id")?,
      item_id: row.try_get("item_id")?,
      asker_name: row.try_get("asker_name")?,
      scope: row.try_get("scope")?,
      addressee: stored(row.try_get("addressee")?, QuestionAddressee::parse)?,
      text: row.try_get("text")?,
      choices: from_json(row.try_get::<&str, _>("choices")?)?,
      recommendation: match row.try_get::<Option<&str>, _>("recommendation")? {
        Some(recommendation) => Some(from_json(recommendation)?),
        None => None,
      },
      state: stored(row.try_get("state")?, QuestionState::parse)?,
      due_at: row.try_get("due_at")?,
      told_by_asker: row.try_get::<i64, _>("told_by_asker")? != 0,
      told_as: row.try_get("told_as")?,
      heard_through: row.try_get("heard_through")?,
      readdressed_by: row.try_get("readdressed_by")?,
      readdressed_at: row.try_get("readdressed_at")?,
      created_at: row.try_get("created_at")?,
    },
    remaining_ms: row.try_get("remaining_ms")?,
    due_told: row.try_get::<i64, _>("due_told")? != 0,
    wait_ms: row.try_get("wait_ms")?,
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
    source: stored(row.try_get("source")?, ReplySource::parse)?,
    why: row.try_get("why")?,
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
    "UPDATE questions SET state = $1, due_at = $2, remaining_ms = $3, due_told = $4, recommendation = $5, \
     text = $6, choices = $7, heard_through = $8, addressee = $9, readdressed_by = $10, readdressed_at = $11, \
     told_as = $12 WHERE id = $13",
  )
  .bind(kept.question.state.as_str())
  .bind(kept.question.due_at)
  .bind(kept.remaining_ms)
  .bind(i64::from(due_told))
  .bind(kept.question.recommendation.as_ref().map(to_json))
  .bind(&kept.question.text)
  .bind(to_json(&kept.question.choices))
  .bind(kept.question.heard_through)
  .bind(kept.question.addressee.as_str())
  .bind(&kept.question.readdressed_by)
  .bind(kept.question.readdressed_at)
  .bind(&kept.question.told_as)
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

/// What a question's choices must be: each filled, none twice.
fn judged_choices(choices: &[String]) -> Result<(), Refusal> {
  let mut seen = HashSet::new();
  for choice in choices {
    filled("choice", choice)?;
    if !seen.insert(choice.as_str()) {
      return Err(Refusal::bad_request(format!("The choice {choice:?} stands twice.")));
    }
  }
  Ok(())
}

/// What a recommendation must hold: an answer and a reason, the answer one of the choices word for word or a free
/// answer that neither opens nor closes on a choice's words, since it settles the question by itself.
fn judged_recommendation(
  recommendation: &QuestionRecommendation,
  choices: &[String],
) -> Result<(), Refusal> {
  filled("recommendation.answer", &recommendation.answer)?;
  filled("recommendation.why", &recommendation.why)?;
  let answer = &recommendation.answer;
  if choices.contains(answer) {
    return Ok(());
  }
  match choices.iter().find(|choice| holds_words_of(answer, choice)) {
    Some(choice) => Err(Refusal::new(
      Code::NotAChoice,
      format!(
        "The recommendation {answer:?} is the choice {choice:?} with other words beside it; name the choice \
         word for word, or give a free answer that does not open or close on a choice."
      ),
    )),
    None => Ok(()),
  }
}

/// Whether `answer` is the words of `choice` with space around them, or opens or closes on those words with more
/// beside them past a break in the word.
fn holds_words_of(answer: &str, choice: &str) -> bool {
  let (answer, choice) = (answer.trim(), choice.trim());
  let breaks = |ch: char| !ch.is_alphanumeric();
  answer == choice
    || answer.strip_prefix(choice).is_some_and(|rest| rest.starts_with(breaks))
    || answer.strip_suffix(choice).is_some_and(|rest| rest.ends_with(breaks))
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

/// The actor a reply the due time settled carries; its source says the recommendation gave it.
const DUE_TIME_ACTOR: &str = "due time";

/// Settles each open question whose due time has passed by `now` with its recommendation, one told of as due before
/// it was given one among them, and tells once of each such question that has none, which stays open for a person.
pub(crate) async fn tell_due_questions(tx: &mut Tx, now: i64) -> Result<usize, Refusal> {
  let rows = sqlx::query(&format!(
    "SELECT {QUESTION_COLUMNS} FROM questions WHERE state = 'open' AND due_at IS NOT NULL AND due_at <= $1 \
     AND (due_told = 0 OR recommendation IS NOT NULL)"
  ))
  .bind(now)
  .fetch_all(&mut **tx)
  .await?;
  for row in &rows {
    let mut kept = kept_from(row)?;
    match kept.question.recommendation.clone() {
      Some(recommendation) => settle_on_recommendation(tx, &mut kept, recommendation, now).await?,
      None => {
        save_question(tx, &kept, true).await?;
        told_question(tx, now, EventKind::QuestionDue, &kept.question).await?;
      }
    }
  }
  Ok(rows.len())
}

/// Settles a question with its recommendation: the answer as the choice where it is one, else as a free answer.
async fn settle_on_recommendation(
  tx: &mut Tx,
  kept: &mut Kept,
  recommendation: QuestionRecommendation,
  now: i64,
) -> Result<(), Refusal> {
  let QuestionRecommendation { answer, why } = recommendation;
  let choice = kept.question.choices.contains(&answer).then(|| answer.clone());
  let actor = DUE_TIME_ACTOR.to_owned();
  write_reply(tx, kept, choice, answer, actor, true, ReplySource::Recommendation, Some(why), now).await?;
  Ok(())
}

impl Store {
  /// Questions, filtered as the query asks.
  pub async fn questions(&self, query: &QuestionQuery) -> Result<Page<Question>, Refusal> {
    let limit = page_limit(query.limit)?;
    let states = listed("state", query.state.as_deref(), QuestionState::parse)?;
    let states: Vec<&str> = states.iter().map(|state| state.as_str()).collect();
    let addressees = listed("addressee", query.addressee.as_deref(), QuestionAddressee::parse)?;
    let addressees: Vec<&str> = addressees.iter().map(|addressee| addressee.as_str()).collect();
    let mut tx = self.begin().await?;
    let rows = sqlx::query(&format!(
      "SELECT {QUESTION_COLUMNS} FROM questions WHERE id > $1 AND ($2 IS NULL OR scope = $2) \
       AND ($3 IS NULL OR item_id = $3) AND ($4 = '[]' OR state IN (SELECT value FROM json_each($4))) \
       AND ($5 = '[]' OR addressee IN (SELECT value FROM json_each($5))) ORDER BY id LIMIT $6"
    ))
    .bind(query.after.unwrap_or(0))
    .bind(&query.scope)
    .bind(query.item_id)
    .bind(to_json(&states))
    .bind(to_json(&addressees))
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

  /// The questions under `scope` put to a person and still waiting, the one asked last first: those open, whether
  /// or not their due time has passed, and those paused.
  pub async fn open_questions_in(&self, scope: &str) -> Result<Vec<Question>, Refusal> {
    let mut tx = self.begin().await?;
    let rows = sqlx::query(&format!(
      "SELECT {QUESTION_COLUMNS} FROM questions WHERE scope = $1 AND state IN ($2, $3) AND addressee = $4 \
       ORDER BY id DESC"
    ))
    .bind(scope)
    .bind(QuestionState::Paused.as_str())
    .bind(QuestionState::Open.as_str())
    .bind(QuestionAddressee::Person.as_str())
    .fetch_all(&mut *tx)
    .await?;
    tx.commit().await?;
    rows.iter().map(|row| kept_from(row).map(|kept| kept.question)).collect()
  }

  /// Asks a question, its scope the item's where none is given. Only a caller that supervises the work asks a
  /// person; any other caller's question goes to the supervisor, which has no due time while it holds it.
  pub async fn ask(&self, new: NewQuestion, caller: &Caller) -> Result<Question, Refusal> {
    filled("askerName", &new.asker_name)?;
    filled("text", &new.text)?;
    judged_choices(&new.choices)?;
    if let Some(recommendation) = &new.recommendation {
      judged_recommendation(recommendation, &new.choices)?;
    }
    let wait = new.wait_ms.unwrap_or(DEFAULT_ASK_WAIT_MS);
    if !(LEAST_ASK_WAIT_MS..=LONGEST_ASK_WAIT_MS).contains(&wait) {
      return Err(Refusal::bad_request(format!(
        "A question's wait is {LEAST_ASK_WAIT_MS} to {LONGEST_ASK_WAIT_MS} milliseconds."
      )));
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
    let addressee = match new.addressee.unwrap_or_default() {
      QuestionAddressee::Person if !caller.supervises => QuestionAddressee::Supervisor,
      named => named,
    };
    let due_at = (addressee == QuestionAddressee::Person).then_some(now + wait);
    let id: i64 = sqlx::query_scalar(
      "INSERT INTO questions (item_id, asker_name, scope, addressee, text, choices, recommendation, state, \
       due_at, due_told, told_by_asker, wait_ms, created_at) \
       VALUES ($1, $2, $3, $4, $5, $6, $7, 'open', $8, 0, $9, $10, $11) RETURNING id",
    )
    .bind(new.item_id)
    .bind(&new.asker_name)
    .bind(&scope)
    .bind(addressee.as_str())
    .bind(&new.text)
    .bind(to_json(&new.choices))
    .bind(new.recommendation.as_ref().map(to_json))
    .bind(due_at)
    .bind(i64::from(new.told_by_asker))
    .bind(wait)
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

  /// Pauses a question's clock, starts it again, withdraws the question or closes it unsent; puts it again in
  /// other words; moves what its asker was handed; puts a question the supervisor holds to a person, where the
  /// caller supervises the work.
  pub async fn change_question(
    &self,
    id: i64,
    change: QuestionChange,
    caller: &Caller,
  ) -> Result<Question, Refusal> {
    let QuestionChange {
      state,
      text,
      choices,
      recommendation,
      heard_through,
      addressee,
      told_as,
      if_addressee,
    } = change;
    if state.is_none()
      && text.is_none()
      && choices.is_none()
      && recommendation.is_none()
      && heard_through.is_none()
      && addressee.is_none()
      && told_as.is_none()
    {
      return Err(Refusal::bad_request(
        "A change names a state, words, what the asker was handed, who the question is put to, or how it was told.",
      ));
    }
    let now = self.now();
    let mut tx = self.begin().await?;
    let mut kept = kept_at(&mut tx, id).await?;
    if let Some(expected) = if_addressee
      && kept.question.addressee != expected
    {
      return Err(Refusal::conflict(format!(
        "Question {id} is put to the {}, not to the {expected}, and is left as it stands.",
        kept.question.addressee
      )));
    }
    if let Some(told_as) = told_as {
      filled("toldAs", &told_as)?;
      kept.question.told_as = Some(told_as);
    }
    let mut due_told = kept.due_told;
    let mut moved = false;
    if let Some(addressee) = addressee {
      readdressed(&mut kept, addressee, caller, now)?;
      due_told = false;
      moved = true;
    }
    if text.is_some() || choices.is_some() || recommendation.is_some() {
      if !matches!(kept.question.state, QuestionState::Open | QuestionState::Paused) {
        return Err(Refusal::conflict(format!(
          "The question is {} and is not put again.",
          kept.question.state
        )));
      }
      let answers_moved = choices.is_some() || recommendation.is_some();
      if let Some(text) = text {
        filled("text", &text)?;
        kept.question.text = text;
      }
      if let Some(choices) = choices {
        judged_choices(&choices)?;
        kept.question.choices = choices;
      }
      if let Some(recommendation) = recommendation {
        kept.question.recommendation = Some(recommendation);
      }
      // The recommendation is judged against the choices it now stands beside, whichever of the two moved.
      if answers_moved && let Some(recommendation) = &kept.question.recommendation {
        judged_recommendation(recommendation, &kept.question.choices)?;
      }
      moved = true;
    }
    if let Some(heard) = heard_through {
      kept.question.heard_through = Some(kept.question.heard_through.map_or(heard, |had| had.max(heard)));
    }
    if let Some(state) = state.filter(|state| *state != kept.question.state) {
      let from = kept.question.state;
      match (from, state) {
        // A pause and an opening again keep whether the due time was told: a question is told of as due once
        // each time it is put to a person.
        (QuestionState::Open, QuestionState::Paused) => {
          kept.remaining_ms = kept.question.due_at.map(|due| (due - now).max(0));
          kept.question.due_at = None;
        }
        // A clock that had run out when it stopped runs a whole wait again rather than passing at once.
        (QuestionState::Paused, QuestionState::Open) => {
          kept.question.due_at =
            kept.remaining_ms.map(|left| now + if left > 0 { left } else { kept.wait_ms });
          kept.remaining_ms = None;
        }
        (QuestionState::Open | QuestionState::Paused, QuestionState::Withdrawn | QuestionState::Unsent) => {
          due_told = true;
        }
        _ => return Err(Refusal::conflict(format!("A question {from} cannot move to {state}."))),
      }
      kept.question.state = state;
      moved = true;
    }
    save_question(&mut tx, &kept, due_told).await?;
    if moved {
      told_question(&mut tx, now, EventKind::QuestionChanged, &kept.question).await?;
    }
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
      "SELECT id, question_id, choice, text, actor_name, settles, source, why, created_at FROM replies \
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

  /// Replies to a question: a settling reply answers it, a word is added in any state. Only a caller that
  /// supervises the work settles a question the supervisor holds.
  pub async fn reply(&self, question_id: i64, new: NewReply, caller: &Caller) -> Result<Reply, Refusal> {
    let now = self.now();
    let mut tx = self.begin().await?;
    let reply = reply_in(&mut tx, question_id, new, caller, now).await?;
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
      let due_told = kept.due_told;
      save_question(&mut tx, &kept, due_told).await?;
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
      &Caller::shared(),
      now,
    )
    .await?;
    self.commit(tx).await?;
    Ok(reply)
  }
}

/// Puts a question the supervisor holds to a person: only a caller that supervises the work may, only while it
/// waits, and never back to the supervisor. Its clock starts then with the wait it was asked with.
fn readdressed(
  kept: &mut Kept,
  addressee: QuestionAddressee,
  caller: &Caller,
  now: i64,
) -> Result<(), Refusal> {
  let question = &mut kept.question;
  if !matches!(question.state, QuestionState::Open | QuestionState::Paused) {
    return Err(Refusal::conflict(format!(
      "The question is {} and is put to nobody else.",
      question.state
    )));
  }
  if question.addressee == addressee {
    return Err(Refusal::conflict(format!("The question is put to the {addressee} already.")));
  }
  if !caller.supervises {
    return Err(Refusal::new(
      Code::NotTheAddressee,
      format!(
        "Only a caller that supervises the work puts question {} to the {addressee}.",
        question.id
      ),
    ));
  }
  question.addressee = addressee;
  question.state = QuestionState::Open;
  kept.remaining_ms = None;
  match addressee {
    // Put to a person: his clock counts from now.
    QuestionAddressee::Person => {
      question.readdressed_by = caller.name.clone();
      question.readdressed_at = Some(now);
      question.due_at = Some(now + kept.wait_ms);
    }
    // Taken back while nobody settled it: the supervisor's again, with no clock.
    QuestionAddressee::Supervisor => {
      question.readdressed_by = None;
      question.readdressed_at = None;
      question.due_at = None;
    }
  }
  Ok(())
}

fn not_the_addressee(question: &Question) -> Refusal {
  let id = question.id;
  let sentence = match question.addressee {
    QuestionAddressee::Supervisor => format!(
      "Question {id} is put to the supervisor, and only a caller that supervises the work settles it or puts it elsewhere."
    ),
    QuestionAddressee::Person => {
      format!("Question {id} is put to a person, and a caller that supervises the work does not settle it.")
    }
  };
  Refusal::new(Code::NotTheAddressee, sentence)
}

/// Writes a reply to a question in the transaction given: a settling reply answers it, a word is added in any
/// state. A settling reply to a question the supervisor holds is taken from a caller that supervises the work
/// alone.
async fn reply_in(
  tx: &mut Tx,
  question_id: i64,
  new: NewReply,
  caller: &Caller,
  now: i64,
) -> Result<Reply, Refusal> {
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
  if settles && (kept.question.addressee == QuestionAddressee::Supervisor) != caller.supervises {
    return Err(not_the_addressee(&kept.question));
  }
  let source = if caller.supervises { ReplySource::Supervisor } else { ReplySource::Person };
  write_reply(tx, &mut kept, new.choice, new.text, new.actor_name, settles, source, None, now).await
}

/// Writes a reply to a question in the transaction given, telling it and answering the question where it
/// settles.
#[allow(clippy::too_many_arguments)]
async fn write_reply(
  tx: &mut Tx,
  kept: &mut Kept,
  choice: Option<String>,
  text: String,
  actor_name: String,
  settles: bool,
  source: ReplySource,
  why: Option<String>,
  now: i64,
) -> Result<Reply, Refusal> {
  let id: i64 = sqlx::query_scalar(
    "INSERT INTO replies (question_id, choice, text, actor_name, settles, source, why, created_at) \
     VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING id",
  )
  .bind(kept.question.id)
  .bind(&choice)
  .bind(&text)
  .bind(&actor_name)
  .bind(i64::from(settles))
  .bind(source.as_str())
  .bind(&why)
  .bind(now)
  .fetch_one(&mut **tx)
  .await?;
  let reply = Reply {
    id,
    question_id: kept.question.id,
    choice,
    text,
    actor_name,
    settles,
    source,
    why,
    created_at: now,
  };
  let keys = question_keys(tx, &kept.question).await?;
  emit(tx, now, EventKind::ReplyCreated.as_str(), &reply, &keys).await?;
  if settles {
    kept.question.state = QuestionState::Answered;
    kept.remaining_ms = None;
    save_question(tx, kept, true).await?;
    told_question(tx, now, EventKind::QuestionChanged, &kept.question).await?;
  }
  Ok(reply)
}
