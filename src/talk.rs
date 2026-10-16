//! The owner's talk about an item (docs/api.md, "The talk about an item"): his words kept the moment they
//! arrive, the turn handed to bmo-receiver's agent and answered later as a line of the talk, one turn at a
//! time per item, and where each talk stands held by the running service.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use job_base::Refusal;
use job_codes::talk::Unanswered;
use job_codes::{JobError, TalkRefusal};
use job_receiver::{ItemTopic, Receiver, ReceiverClient, ReceiverError, Talk, Topic, Turn};
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::Serialize;

use crate::events::{Events, News};
use crate::{all, now_ms, one, run};

/// Who said a turn of the talk, as `todo_talk` keeps it.
const OWNER: &str = "owner";
const AGENT: &str = "agent";

/// Where one item's talk stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TalkState {
  /// Whether a turn is being answered.
  pub answering: bool,
  /// Why the last turn went unanswered, where it did.
  pub failure: Option<Unanswered>,
}

/// Where every item's talk stands, as the running service holds it.
#[derive(Default)]
pub struct Talks {
  states: Mutex<HashMap<i64, TalkState>>,
}

impl Talks {
  fn states(&self) -> MutexGuard<'_, HashMap<i64, TalkState>> {
    self.states.lock().unwrap_or_else(PoisonError::into_inner)
  }

  /// Where the item's talk stands; one never begun stands answering nothing and failing nothing.
  pub fn state(&self, item: i64) -> TalkState {
    self.states().get(&item).copied().unwrap_or_default()
  }

  // A turn begun, where none is being answered.
  fn begin(&self, item: i64) -> bool {
    let mut states = self.states();
    let state = states.entry(item).or_default();

    if state.answering {
      return false;
    }

    *state = TalkState {
      answering: true,
      failure: None,
    };

    true
  }

  fn end(&self, item: i64, failure: Option<Unanswered>) {
    self.states().insert(
      item,
      TalkState {
        answering: false,
        failure,
      },
    );
  }
}

/// What a turn of the talk stands on: the database its words are kept in, the receiver its turn goes to,
/// the stream the talk's news goes out on and where every talk stands.
#[derive(Clone)]
pub struct Talking {
  pub db: DatabaseConnection,
  pub receiver: ReceiverClient,
  pub events: Events,
  pub talks: Arc<Talks>,
}

impl Talking {
  /// One turn: his words kept, the turn handed to bmo-receiver's agent, and the request answered at once;
  /// the answer is kept as it comes.
  pub async fn take_turn(&self, item: i64, said: &str) -> Result<(), Refusal> {
    let Some(topic) = topic(&self.db, item).await? else {
      return Err(JobError::from(TalkRefusal::TalkItemAbsent { id: item }).into());
    };
    let earlier = turns(&self.db, item).await?;

    if !self.talks.begin(item) {
      return Err(JobError::from(TalkRefusal::TalkBusy).into());
    }

    if let Err(failed) = say(&self.db, item, OWNER, said).await {
      self.talks.end(item, None);

      return Err(failed);
    }

    self.events.publish(&News::TalkMoved {
      item,
      failure: None,
    });

    let talk = Talk {
      topic: Topic::Item(topic),
      turns: earlier,
      said: said.to_owned(),
    };
    let talking = self.clone();

    tokio::spawn(async move { talking.answered(item, talk).await });

    Ok(())
  }

  // The agent's answer kept as a line of the talk, or why it came back with none.
  async fn answered(&self, item: i64, talk: Talk) {
    let failure = match self.receiver.talk(&talk).await {
      Ok(Some(answer)) => match say(&self.db, item, AGENT, &answer).await {
        Ok(()) => None,
        Err(why) => {
          eprintln!("talk on #{item}: the answer could not be kept: {why}");
          Some(Unanswered::Other)
        }
      },
      Ok(None) => Some(Unanswered::Silent),
      Err(failed) => {
        eprintln!("talk on #{item} failed: {failed}");
        Some(unanswered(&failed))
      }
    };

    self.talks.end(item, failure);
    self.events.publish(&News::TalkMoved { item, failure });
  }
}

// Why a talk that failed went unanswered, by the code bmo-receiver refused it with.
fn unanswered(failed: &ReceiverError) -> Unanswered {
  match failed {
    ReceiverError::Refused { code, .. } => match code.as_str() {
      "resting" => Unanswered::Resting,
      "limit" => Unanswered::Limit,
      "outage" => Unanswered::Outage,
      _ => Unanswered::Other,
    },
    _ => Unanswered::Other,
  }
}

// The item as the talk's topic names it: whether a run of it still ends while it is held.
async fn topic(db: &impl ConnectionTrait, id: i64) -> Result<Option<ItemTopic>, Refusal> {
  let Some(row) = one(
    db,
    r#"SELECT "title", "done", "merged_into", "held_at", "held_kind", "approved", "status" FROM "todos" WHERE "id" = $1"#,
    vec![id.into()],
  )
  .await?
  else {
    return Ok(None);
  };
  let done = row.try_get::<i64>("", "done")? == 1;
  let held = row.try_get::<Option<i64>>("", "held_at")?.is_some();
  let running = one(
    db,
    r#"SELECT 1 AS "one" FROM "jobs" WHERE "todo_id" = $1 AND "finished_at" IS NULL"#,
    vec![id.into()],
  )
  .await?
  .is_some();
  let ordered = all(db, r#"SELECT "body" FROM "work_orders" WHERE "todo_id" = $1"#, vec![id.into()])
    .await?
    .iter()
    .filter_map(|row| row.try_get::<String>("", "body").ok())
    .filter_map(|body| serde_json::from_str::<serde_json::Value>(&body).ok())
    .any(|order| order["status"] == "active");
  let merged_into: Option<i64> = row.try_get("", "merged_into")?;

  Ok(Some(ItemTopic {
    id,
    title: row.try_get("", "title")?,
    done,
    merged_into: merged_into.filter(|_| !done),
    held,
    hold: if held { row.try_get("", "held_kind")? } else { None },
    stopping: held && !done && (running || ordered),
    approved: row.try_get::<i64>("", "approved")? == 1,
    status: row.try_get("", "status")?,
  }))
}

// The talk's turns so far, oldest first.
async fn turns(db: &impl ConnectionTrait, item: i64) -> Result<Vec<Turn>, Refusal> {
  let rows = all(
    db,
    r#"SELECT "speaker", "text" FROM "todo_talk" WHERE "todo_id" = $1 ORDER BY "id""#,
    vec![item.into()],
  )
  .await?;

  rows
    .iter()
    .map(|row| {
      Ok(Turn {
        owner: row.try_get::<String>("", "speaker")? == OWNER,
        text: row.try_get("", "text")?,
      })
    })
    .collect()
}

// One line of the talk kept.
async fn say(db: &impl ConnectionTrait, item: i64, speaker: &str, text: &str) -> Result<(), Refusal> {
  run(
    db,
    r#"INSERT INTO "todo_talk" ("todo_id", "speaker", "text", "said_at") VALUES ($1, $2, $3, $4)"#,
    vec![item.into(), speaker.into(), text.into(), now_ms().into()],
  )
  .await
  .map(drop)
}
