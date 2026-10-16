//! One read of the database (docs/reading.md, "One read of the database"): the queue, the rows' sources,
//! the workers and which items still have a run going, read whole as it opens, and an item's own records
//! once that item is opened.

use std::collections::{HashMap, HashSet};

use job_base::{Refusal, all, one};
use sea_orm::{ConnectionTrait, QueryResult, Value};
use serde::Deserialize;
use strum::EnumString;

use crate::impact::ImpactGrounds;
use crate::time::{Run, Span};

/// What a read of the database answers: a failure of the database is the refusal `database`.
pub type Read<T> = Result<T, Refusal>;

/// One item of the queue, as the todos table holds it.
#[derive(Clone, Debug, PartialEq)]
pub struct TodoItem {
  pub id: i64,
  pub title: String,
  pub instruction: String,
  /// The organized instruction the agent keeps for the item's jobs, in English, once a job was launched or
  /// the agent rewrote it.
  pub brief: Option<String>,
  pub requester: String,
  pub requester_id: Option<i64>,
  pub chat_id: Option<i64>,
  pub approved: bool,
  pub done: bool,
  pub priority: i64,
  pub status: String,
  pub model: Option<String>,
  pub worker: Option<String>,
  pub cowork: bool,
  /// The grounds its co-work setting was weighed on, where it was filed with any that read.
  pub impact: Option<ImpactGrounds>,
  /// The grounds a job of the item weighed again, where one did.
  pub job_impact: Option<ImpactGrounds>,
  pub note: Option<String>,
  pub report: Option<String>,
  pub unfinished: Option<String>,
  pub unfinished_resolved_at: Option<i64>,
  pub unfinished_resolution: Option<String>,
  pub held_at: Option<i64>,
  pub held_reason: Option<String>,
  pub held_kind: Option<HoldKind>,
  pub merged_into: Option<i64>,
  pub merged_at: Option<i64>,
  pub repairs_todo_id: Option<i64>,
  /// The road its work last went out on: a co-work item's as its work order records it, any other item's
  /// latest job that names one. None until one has gone out.
  pub ran_on: Option<RanOn>,
}

/// The provider and model a run went out on, and whether the item's runs went out on more than one.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct RanOn {
  pub provider: String,
  pub model: String,
  #[serde(default)]
  pub changed: bool,
}

impl TodoItem {
  /// Whether its work is carried by another item that is not finished yet.
  pub fn merged(&self) -> bool {
    self.merged_into.is_some() && !self.done
  }
}

/// Which kind of setting aside an item is under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumString)]
#[strum(serialize_all = "snake_case")]
pub enum HoldKind {
  Stop,
  Pause,
  Failure,
}

/// A row named under another: its number, its title and whether it is finished.
#[derive(Clone, Debug, PartialEq)]
pub struct RaisedTodo {
  pub id: i64,
  pub title: String,
  pub done: bool,
}

/// What a work order's body says that a reading of its item needs.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct WorkOrder {
  pub status: String,
  pub intent: String,
  pub instruction: String,
}

/// A question a job stopped to ask.
#[derive(Clone, Debug, PartialEq)]
pub struct JobQuestion {
  pub question: String,
  pub options: Vec<String>,
  pub recommended: i64,
  pub chosen: Option<i64>,
  pub answer: Option<String>,
  pub settled_at: Option<i64>,
  pub settled_by: Option<String>,
}

/// Something added to an item after it was filed.
#[derive(Clone, Debug, PartialEq)]
pub struct TodoNote {
  pub id: i64,
  pub author: String,
  pub text: String,
  pub added_at: i64,
  /// What kind of words these are: the agent's change to the brief, or none for the words people and jobs
  /// add.
  pub kind: Option<String>,
  /// When a running job was handed them, where one was.
  pub delivered_at: Option<i64>,
  /// Whether the job they came with is one of the item's own: the job that wrote them, or the one at work
  /// on the item when somebody else added them.
  pub with_items_job: bool,
}

/// A worker as a line needs it: the name it answers to, and the base a support worker's work is.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkerFace {
  pub name: String,
  pub support_of: Option<String>,
}

/// One item's own records beyond its row, read once the item is opened.
#[derive(Default)]
struct ItemRecords {
  orders: HashMap<i64, Option<WorkOrder>>,
  questions: HashMap<i64, Vec<JobQuestion>>,
  notes: HashMap<i64, Vec<TodoNote>>,
  runs: HashMap<i64, Vec<(Run, Vec<Span>)>>,
}

/// The queue as one read of the database holds it: the items, the rows' sources, the workers and which
/// items still have a run going, read whole as it opens, and an item's own records as that item is opened.
pub struct Queue {
  todos: Vec<TodoItem>,
  sources: Vec<(i64, i64)>,
  workers: Vec<WorkerFace>,
  running: HashSet<i64>,
  active: HashSet<i64>,
  records: ItemRecords,
}

// A yes-or-no column, kept as 0 or 1.
fn flag(row: &QueryResult, column: &str) -> Read<bool> {
  Ok(row.try_get::<Option<i64>>("", column)? == Some(1))
}

fn grounds(row: &QueryResult, column: &str) -> Read<Option<ImpactGrounds>> {
  Ok(
    row
      .try_get::<Option<String>>("", column)?
      .and_then(|written| ImpactGrounds::read(&written).ok()),
  )
}

fn todo_of(row: &QueryResult) -> Read<TodoItem> {
  Ok(TodoItem {
    id: row.try_get("", "id")?,
    title: row.try_get("", "title")?,
    instruction: row.try_get("", "instruction")?,
    brief: row.try_get("", "brief")?,
    requester: row.try_get("", "requester")?,
    requester_id: row.try_get("", "requester_id")?,
    chat_id: row.try_get("", "chat_id")?,
    approved: flag(row, "approved")?,
    done: flag(row, "done")?,
    priority: row.try_get("", "priority")?,
    status: row.try_get("", "status")?,
    model: row.try_get("", "model")?,
    worker: row.try_get("", "worker")?,
    cowork: flag(row, "cowork")?,
    impact: grounds(row, "impact")?,
    job_impact: grounds(row, "job_impact")?,
    note: row.try_get("", "note")?,
    report: row.try_get("", "report")?,
    unfinished: row.try_get("", "unfinished")?,
    unfinished_resolved_at: row.try_get("", "unfinished_resolved_at")?,
    unfinished_resolution: row.try_get("", "unfinished_resolution")?,
    held_at: row.try_get("", "held_at")?,
    held_reason: row.try_get("", "held_reason")?,
    held_kind: row.try_get::<Option<String>>("", "held_kind")?.and_then(|kind| kind.parse().ok()),
    merged_into: row.try_get("", "merged_into")?,
    merged_at: row.try_get("", "merged_at")?,
    repairs_todo_id: row.try_get("", "repairs_todo_id")?,
    ran_on: None,
  })
}

fn raised(item: &TodoItem) -> RaisedTodo {
  RaisedTodo {
    id: item.id,
    title: item.title.clone(),
    done: item.done,
  }
}

impl Queue {
  /// The queue in its own order, what is still open first and each part by priority and number, and
  /// what every line is read against.
  pub async fn open<C: ConnectionTrait>(db: &C) -> Read<Self> {
    let mut todos = all(db, "SELECT * FROM todos ORDER BY done ASC, priority ASC, id ASC", vec![])
      .await?
      .iter()
      .map(todo_of)
      .collect::<Read<Vec<_>>>()?;
    let sources = all(db, "SELECT todo_id, source_todo_id FROM todo_sources", vec![])
      .await?
      .iter()
      .map(|row| Ok((row.try_get("", "todo_id")?, row.try_get("", "source_todo_id")?)))
      .collect::<Read<Vec<_>>>()?;
    let workers = all(db, "SELECT name, support_of FROM workers WHERE resting_since IS NULL", vec![])
      .await?
      .iter()
      .map(|row| {
        Ok(WorkerFace {
          name: row.try_get("", "name")?,
          support_of: row.try_get("", "support_of")?,
        })
      })
      .collect::<Read<Vec<_>>>()?;
    let running =
      all(db, "SELECT todo_id FROM jobs WHERE finished_at IS NULL AND todo_id IS NOT NULL", vec![])
        .await?
        .iter()
        .map(|row| Ok(row.try_get("", "todo_id")?))
        .collect::<Read<HashSet<i64>>>()?;
    // A work order's state is read off its body here rather than in the statement, which each database
    // would write its own way.
    let mut active = HashSet::new();
    let mut order_roads = HashMap::new();

    for row in all(db, "SELECT todo_id, body FROM work_orders", vec![]).await? {
      let todo: i64 = row.try_get("", "todo_id")?;
      let body: String = row.try_get("", "body")?;
      let order = serde_json::from_str::<serde_json::Value>(&body).ok();

      if order.as_ref().and_then(|order| order.get("status")) == Some(&serde_json::Value::from("active")) {
        active.insert(todo);
      }

      if let Some(road) = order
        .as_ref()
        .and_then(|order| order.get("ranOn"))
        .and_then(|road| serde_json::from_value::<RanOn>(road.clone()).ok())
      {
        order_roads.insert(todo, road);
      }
    }

    // Each item's latest job that went out, the later row taking the earlier's place.
    let mut job_roads = HashMap::new();

    for row in all(
      db,
      "SELECT todo_id, execution_mode, execution_model FROM jobs \
       WHERE todo_id IS NOT NULL AND execution_mode IS NOT NULL AND execution_model IS NOT NULL ORDER BY id",
      vec![],
    )
    .await?
    {
      job_roads.insert(
        row.try_get::<i64>("", "todo_id")?,
        RanOn {
          provider: row.try_get("", "execution_mode")?,
          model: row.try_get("", "execution_model")?,
          changed: false,
        },
      );
    }

    for item in &mut todos {
      item.ran_on = if item.cowork { order_roads.remove(&item.id) } else { job_roads.remove(&item.id) };
    }

    Ok(Self {
      todos,
      sources,
      workers,
      running,
      active,
      records: ItemRecords::default(),
    })
  }

  /// Every record of one item, and of each job it ran, read for what opening it needs.
  pub async fn read_item<C: ConnectionTrait>(&mut self, db: &C, todo: i64) -> Read<()> {
    let body: Option<String> =
      one(db, "SELECT body FROM work_orders WHERE todo_id = $1", vec![todo.into()])
        .await?
        .map(|row| row.try_get("", "body"))
        .transpose()?;
    let questions =
      all(db, "SELECT * FROM job_questions WHERE todo_id = $1 ORDER BY id", vec![todo.into()])
        .await?
        .iter()
        .map(|row| {
          Ok(JobQuestion {
            question: row.try_get("", "question")?,
            options: row.try_get::<String>("", "options")?.split('\n').map(str::to_owned).collect(),
            recommended: row.try_get("", "recommended")?,
            chosen: row.try_get("", "chosen")?,
            answer: row.try_get("", "answer")?,
            settled_at: row.try_get("", "settled_at")?,
            settled_by: row.try_get("", "settled_by")?,
          })
        })
        .collect::<Read<Vec<_>>>()?;
    // The words added to the item and to every row merged into it, which opening it lists under each.
    let merged: Vec<i64> = self
      .todos
      .iter()
      .filter(|row| row.merged_into == Some(todo))
      .map(|row| row.id)
      .collect();

    for noted in std::iter::once(todo).chain(merged) {
      let notes = notes_of(db, noted).await?;

      self.records.notes.insert(noted, notes);
    }

    let runs = runs_of(db, todo).await?;

    self.records.orders.insert(todo, body.and_then(|body| serde_json::from_str(&body).ok()));
    self.records.questions.insert(todo, questions);
    self.records.runs.insert(todo, runs);

    Ok(())
  }

  /// Every item: what is still open first, each part in the queue's own order.
  pub fn all_todos(&self) -> &[TodoItem] {
    &self.todos
  }

  pub fn get_todo(&self, id: i64) -> Option<&TodoItem> {
    self.todos.iter().find(|item| item.id == id)
  }

  /// The unfinished item a title names; a title finished long ago is not what a caller means.
  pub fn find_todo_by_title(&self, title: &str) -> Option<&TodoItem> {
    self.todos.iter().find(|item| item.title == title && !item.done)
  }

  /// The items merged into this one, in number order.
  pub fn todos_merged_into(&self, id: i64) -> Vec<RaisedTodo> {
    let mut merged: Vec<RaisedTodo> =
      self.todos.iter().filter(|item| item.merged_into == Some(id)).map(raised).collect();

    merged.sort_by_key(|item| item.id);
    merged
  }

  /// The rows an unfinished note was broken into, in the order they were raised.
  pub fn todos_raised_from(&self, source: i64) -> Vec<RaisedTodo> {
    let mut rows: Vec<RaisedTodo> = self
      .sources
      .iter()
      .filter(|(_, from)| *from == source)
      .filter_map(|(todo, _)| self.get_todo(*todo).map(raised))
      .collect();

    rows.sort_by_key(|item| item.id);
    rows
  }

  /// The items filed to repair this one, in number order.
  pub fn todo_repairs(&self, id: i64) -> Vec<&TodoItem> {
    let mut repairs: Vec<&TodoItem> =
      self.todos.iter().filter(|item| item.repairs_todo_id == Some(id)).collect();

    repairs.sort_by_key(|item| item.id);
    repairs
  }

  /// The item's work order, where its records were read and it has one.
  pub fn work_order(&self, todo: i64) -> Option<&WorkOrder> {
    self.records.orders.get(&todo).and_then(Option::as_ref)
  }

  /// The questions the item's jobs asked, in the order they were asked.
  pub fn todo_questions(&self, todo: i64) -> &[JobQuestion] {
    self.records.questions.get(&todo).map_or(&[], Vec::as_slice)
  }

  /// The item's runs, oldest first, each with the spans it wrote down; none until the item's records are
  /// read.
  pub fn item_runs(&self, todo: i64) -> &[(Run, Vec<Span>)] {
    self.records.runs.get(&todo).map_or(&[], Vec::as_slice)
  }

  /// What was added to the item and not taken back, oldest first.
  pub fn todo_notes(&self, todo: i64) -> &[TodoNote] {
    self.records.notes.get(&todo).map_or(&[], Vec::as_slice)
  }

  /// The workers that stand, resting ones left out.
  pub fn workers(&self) -> &[WorkerFace] {
    &self.workers
  }

  /// The held items whose run is still going: a job of the item not yet finished, or its work order
  /// still active.
  pub fn hold_pending_ids(&self, items: &[TodoItem]) -> HashSet<i64> {
    items
      .iter()
      .filter(|item| {
        item.held_at.is_some()
          && !item.done
          && (self.running.contains(&item.id) || self.active.contains(&item.id))
      })
      .map(|item| item.id)
      .collect()
  }
}

// The runs of an item, oldest first, each with the spans it wrote down as it ended, the longest first.
async fn runs_of<C: ConnectionTrait>(db: &C, todo: i64) -> Read<Vec<(Run, Vec<Span>)>> {
  let jobs = all(
    db,
    "SELECT id, launched_at, finished_at, outcome FROM jobs WHERE todo_id = $1 ORDER BY id",
    vec![todo.into()],
  )
  .await?;
  let mut runs = vec![];

  for job in jobs {
    let run = Run {
      id: job.try_get("", "id")?,
      launched_at: job.try_get("", "launched_at")?,
      finished_at: job.try_get("", "finished_at")?,
      outcome: job.try_get("", "outcome")?,
    };
    let spans = all(
      db,
      "SELECT kind, name, seconds, times, tokens, input_tokens, output_tokens, cache_read_tokens, \
       cache_write_tokens FROM job_spans WHERE job_id = $1 ORDER BY seconds DESC, rowid",
      vec![Value::from(run.id)],
    )
    .await?
    .iter()
    .map(|row| {
      Ok(Span {
        kind: row.try_get("", "kind")?,
        name: row.try_get("", "name")?,
        seconds: row.try_get("", "seconds")?,
        times: row.try_get("", "times")?,
        tokens: row.try_get("", "tokens")?,
        input_tokens: row.try_get("", "input_tokens")?,
        output_tokens: row.try_get("", "output_tokens")?,
        cache_read_tokens: row.try_get("", "cache_read_tokens")?,
        cache_write_tokens: row.try_get("", "cache_write_tokens")?,
      })
    })
    .collect::<Read<Vec<_>>>()?;

    runs.push((run, spans));
  }

  Ok(runs)
}

async fn notes_of<C: ConnectionTrait>(db: &C, todo: i64) -> Read<Vec<TodoNote>> {
  all(
    db,
    "SELECT todo_notes.id, todo_notes.author, todo_notes.text, todo_notes.added_at, todo_notes.kind, \
     todo_notes.delivered_at, jobs.todo_id AS job_todo FROM todo_notes \
     LEFT JOIN jobs ON jobs.id = todo_notes.job_id \
     WHERE todo_notes.todo_id = $1 AND todo_notes.removed_at IS NULL ORDER BY todo_notes.id",
    vec![todo.into()],
  )
  .await?
  .iter()
  .map(|row| {
    Ok(TodoNote {
      id: row.try_get("", "id")?,
      author: row.try_get("", "author")?,
      text: row.try_get("", "text")?,
      added_at: row.try_get("", "added_at")?,
      kind: row.try_get("", "kind")?,
      delivered_at: row.try_get("", "delivered_at")?,
      with_items_job: row.try_get::<Option<i64>>("", "job_todo")? == Some(todo),
    })
  })
  .collect()
}
