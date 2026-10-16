//! The TODO's rows and work orders, written (docs/rows.md): an item filed, added to, approved, weighed,
//! changed, held, merged, closed and opened again, a work order's body under its lock, and the closing's
//! readings of an item's unfinished note. What a verb does beyond the database goes through the hands and
//! the units its caller gives it.

pub mod blocks;
pub mod closing;
pub mod filing;
pub mod impact;
pub mod instructions;
pub mod js;
pub mod order_lock;
pub mod store;
pub mod todos;
pub mod units;
pub mod wording;
pub mod work_orders;

use futures_util::future::BoxFuture;
use job_base::Refusal;
use job_codes::{JobError, QueueTold, TodoAnswer, TodoFact, TodoRefusal};
use job_policy::{Names, Policy};
use sea_orm::DatabaseConnection;

pub use units::{Ports, Units};

use crate::closing::Reader;
use crate::wording::BannedTerm;

/// What a verb answers: what it came to, or why it refused.
pub type Answered = Result<TodoAnswer, Refusal>;

/// A refusal of the TODO's verbs, as every verb answers one.
pub fn refused(refusal: TodoRefusal) -> Refusal {
  JobError::from(refusal).into()
}

/// What a job wrote onto one item in one call, which is rendered into the owner's language for him to read
/// (docs/rows.md, "Where a verb stands"): the item, its parts written and the notes added.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Written {
  pub todo: i64,
  pub texts: Vec<&'static str>,
  pub notes: Vec<i64>,
}

/// Words added to an item by somebody other than chat BMO's own turn and the item's own job, which BMO is
/// told of: the item, the note they stand in, who added them and the job at work on the item.
#[derive(Clone, Copy, Debug)]
pub struct WordsAdded<'a> {
  pub todo: i64,
  pub title: &'a str,
  pub note: i64,
  pub by: &'a str,
  pub running: Option<i64>,
}

impl WordsAdded<'_> {
  /// The fact BMO is told in the owner's chat.
  pub fn fact(&self) -> TodoFact {
    TodoFact::WordsAppended {
      id: self.todo,
      title: self.title.to_owned(),
      by: self.by.to_owned(),
      job: self.running,
    }
  }
}

/// What a verb does beyond the database, which its caller carries: the screens told the queue moved, BMO
/// told how the queue stands and of words somebody else added, and an item's runs stopped. A test stands in
/// for each.
pub trait Hands: Sync {
  /// The screens told the queue moved, with the work order whose body moved and what a job wrote, where
  /// either.
  fn moved(&self, work_order: Option<i64>, written: Option<Written>) -> BoxFuture<'_, ()>;
  /// BMO told how the queue stands at a moment that changed it.
  fn queue_standing<'a>(
    &'a self,
    moment: &'a str,
    repeat: bool,
  ) -> BoxFuture<'a, Result<QueueTold, Refusal>>;
  /// BMO told of words added to an item by somebody other than its own turn and the item's own job.
  fn words_added<'a>(&'a self, added: &'a WordsAdded<'a>) -> BoxFuture<'a, Result<(), Refusal>>;
  /// The item's runs stopped and the item held as stopped; how many runs were asked to stop.
  fn stop<'a>(&'a self, id: i64, why: &'a str) -> BoxFuture<'a, Result<u32, Refusal>>;
}

/// Whose turn files an item: the agent, which the item keeps so its ending is told to it, and the label its
/// reply stands under where that is not the deployment's agent.
#[derive(Clone, Copy, Debug, Default)]
pub struct Filer<'a> {
  pub agent: Option<&'a str>,
  pub label: Option<&'a str>,
}

/// Where a verb stands (docs/rows.md, "Where a verb stands").
pub struct Ground<'a> {
  pub db: &'a DatabaseConnection,
  /// The owner as a sender, which words he adds are kept under.
  pub owner_sender: i64,
  /// The owner's private chat, where BMO is told and an item filed in no room stands.
  pub owner_chat: i64,
  pub names: &'a Names,
  /// The deployment's agent as the records name it.
  pub agent_key: &'a str,
  pub policy: &'a Policy,
  /// The closing's patterns, built once from the policy.
  pub reader: &'a Reader,
  /// The terms the owner has set aside.
  pub banned: &'a [BannedTerm],
  /// The providers bmo-agency lists, by name, which a job's model may name.
  pub providers: &'a [String],
  pub in_job: bool,
  pub run_name: Option<&'a str>,
  pub filer: Filer<'a>,
  /// The person at a shell, where neither a job nor an agent's turn runs the verb.
  pub person: Option<&'a str>,
  pub hands: &'a dyn Hands,
  pub units: &'a dyn Units,
  /// Now, in epoch milliseconds.
  pub now: i64,
}

impl Ground<'_> {
  /// Whether chat BMO's own turn runs the verb, and no job.
  pub fn chat_bmos(&self) -> bool {
    !self.in_job && self.filer.agent == Some(self.agent_key)
  }

  /// What a job wrote onto an item, named for rendering; a person's words, and the owner's words chat BMO
  /// files, are kept as said and name nothing.
  pub fn wrote(&self, todo: i64, texts: Vec<&'static str>, notes: Vec<i64>) -> Option<Written> {
    (self.in_job && (!texts.is_empty() || !notes.is_empty())).then_some(Written { todo, texts, notes })
  }
}

/// The refusal of an item no row answers to.
pub fn no_item(selector: &str) -> Refusal {
  refused(TodoRefusal::NoSuchItem {
    selector: selector.to_owned(),
  })
}

/// An item as a caller names it, or the refusal of one that is not there.
pub async fn find(ground: &Ground<'_>, selector: &str) -> Result<store::Row, Refusal> {
  store::named_row(ground.db, selector).await?.ok_or_else(|| no_item(selector))
}
