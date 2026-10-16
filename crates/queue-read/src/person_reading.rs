//! A person's reading of an item (docs/reading.md, "A person's reading"): one line of it in a listing, and
//! the whole of it once it is opened, as structured values that carry no sentence. Whoever shows them lays
//! them out in the reader's language.

use job_codes::todo_reading::{HeldWord, LineMark, ModelMark, QuestionState, RowState};
use serde::Serialize;

use crate::blocks::{draft_repeats_item, ordered_instruction};
use crate::impact::ImpactGrounds;
use crate::lines::{Facing, Wanted, todo_list_items, todo_worker};
use crate::queue::{HoldKind, JobQuestion, Queue, RaisedTodo, TodoItem, TodoNote};
use crate::ran_model::item_ran_model;
use crate::time::{TimeReading, Toolbook, time_reading};

/// The model an item ran on, with its marks.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ModelReading {
  pub shown: String,
  pub marks: Vec<ModelMark>,
}

/// What holds an item, and the reason given.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hold {
  pub word: HeldWord,
  pub reason: Option<String>,
}

/// One item as a listing carries it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ItemLine {
  pub id: i64,
  pub title: String,
  /// Its state where it is merged, finished or waiting for approval; otherwise `status` says it.
  pub state: Option<LineMark>,
  pub status: String,
  pub marks: Vec<LineMark>,
  pub model: Option<ModelReading>,
  /// Whose work it is, only where it names a worker.
  pub worker: Option<String>,
  pub hold: Option<Hold>,
}

/// The word a held item wears, by who set it aside and whether its run still ends.
pub fn held_word(kind: Option<HoldKind>, pending: bool) -> HeldWord {
  match (kind, pending) {
    (Some(HoldKind::Stop), true) => HeldWord::Stopping,
    (Some(HoldKind::Stop), false) => HeldWord::Stop,
    (Some(HoldKind::Pause), true) => HeldWord::Pausing,
    (Some(HoldKind::Pause), false) => HeldWord::Pause,
    (_, true) => HeldWord::Pending,
    (Some(HoldKind::Failure), false) => HeldWord::Failure,
    (None, false) => HeldWord::Plain,
  }
}

fn model_reading(item: &TodoItem, facing: &Facing, with_request: bool) -> Option<ModelReading> {
  item_ran_model(item.model.as_deref(), item.ran_on.as_ref(), &facing.providers).map(|model| ModelReading {
    marks: model.marks(with_request),
    shown: model.shown,
  })
}

fn hold(item: &TodoItem, facing: &Facing) -> Option<Hold> {
  item.held_at.map(|_| Hold {
    word: held_word(item.held_kind, facing.stopping.contains(&item.id)),
    reason: item.held_reason.clone(),
  })
}

/// One item as a listing carries it.
pub fn item_line(item: &TodoItem, facing: &Facing) -> ItemLine {
  let state = if item.merged() {
    item.merged_into.map(|id| LineMark::MergingInto { id })
  } else if item.done {
    Some(LineMark::Done)
  } else if item.approved {
    None
  } else {
    Some(LineMark::PendingApproval {
      requester: item.requester.clone(),
    })
  };
  let marks = [
    item.merged_into.filter(|_| item.done).map(|id| LineMark::MergedInto { id }),
    item.cowork.then_some(LineMark::Cowork),
    item.note.as_ref().map(|_| LineMark::OwnerNote),
    item.report.as_ref().map(|_| LineMark::Report),
    item.unfinished.as_ref().map(|_| match item.unfinished_resolved_at {
      None => LineMark::Unfinished,
      Some(_) => LineMark::UnfinishedResolved,
    }),
  ]
  .into_iter()
  .flatten()
  .collect();

  ItemLine {
    id: item.id,
    title: item.title.clone(),
    state,
    status: item.status.clone(),
    marks,
    model: model_reading(item, facing, false),
    worker: item.worker.as_ref().map(|_| todo_worker(item, &facing.workers)),
    hold: hold(item, facing),
  }
}

/// The lines a listing carries, in the queue's order.
pub fn item_lines(items: &[TodoItem], wanted: &Wanted, facing: &Facing) -> Vec<ItemLine> {
  todo_list_items(items, wanted, facing)
    .into_iter()
    .map(|item| item_line(item, facing))
    .collect()
}

/// A row linked to an opened item, with where it stands.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LinkedRow {
  pub id: i64,
  pub title: String,
  pub state: RowState,
}

fn raised_row(row: RaisedTodo, open: RowState) -> LinkedRow {
  LinkedRow {
    id: row.id,
    title: row.title,
    state: if row.done { RowState::Done } else { open },
  }
}

/// The item an opened item is merged into.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MergedInto {
  pub id: i64,
  pub title: Option<String>,
  pub at: Option<i64>,
  /// Whether it finishes when that item does.
  pub follows: bool,
}

/// The item an opened item repairs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Repairs {
  pub id: i64,
  pub title: Option<String>,
}

/// A work order's draft, where it says something the item's own blocks do not.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Draft {
  pub intent: String,
  pub instruction: String,
}

/// Words added to an item.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NoteReading {
  pub id: i64,
  pub author: String,
  pub text: String,
  pub added_at: i64,
}

fn note_reading(note: &TodoNote) -> NoteReading {
  NoteReading {
    id: note.id,
    author: note.author.clone(),
    text: note.text.clone(),
    added_at: note.added_at,
  }
}

/// The work of an open item merged into an opened one.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MergedWork {
  pub id: i64,
  pub title: String,
  pub instruction: String,
  pub owner_note: Option<String>,
  pub notes: Vec<NoteReading>,
}

/// The option a question was settled on, numbered from one.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Settled {
  pub number: i64,
  pub option: Option<String>,
}

/// A question a job asked, with where it stands.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct QuestionReading {
  pub question: String,
  pub state: QuestionState,
  pub settled: Option<Settled>,
}

/// Where one question stands: still waiting, the words the owner wrote, or the option it was settled on
/// and whether the owner chose it.
pub fn question_reading(question: &JobQuestion) -> QuestionReading {
  let (state, settled) = match (question.settled_at, &question.answer) {
    (None, _) => (QuestionState::Waiting, None),
    (Some(_), Some(answer)) => (
      QuestionState::Typed {
        answer: answer.clone(),
      },
      None,
    ),
    (Some(_), None) => {
      let chosen = question.chosen.unwrap_or(question.recommended);
      let option = usize::try_from(chosen).ok().and_then(|at| question.options.get(at)).cloned();
      let state = if question.settled_by.as_deref() == Some("owner") {
        QuestionState::Chosen
      } else {
        QuestionState::Defaulted
      };

      (
        state,
        Some(Settled {
          number: chosen + 1,
          option,
        }),
      )
    }
  };

  QuestionReading {
    question: question.question.clone(),
    state,
    settled,
  }
}

/// An item's unfinished note, with its resolution and the rows raised from it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Unfinished {
  pub text: String,
  pub resolved_at: Option<i64>,
  pub resolution: Option<String>,
  pub raised: Vec<LinkedRow>,
}

/// The whole of one opened item.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ItemReading {
  /// Its line, the model's request included.
  pub line: ItemLine,
  pub held_at: Option<i64>,
  pub merged_into: Option<MergedInto>,
  pub merged_here: Vec<LinkedRow>,
  pub repairs: Option<Repairs>,
  pub repaired_by: Vec<LinkedRow>,
  pub requester: String,
  pub requester_id: Option<i64>,
  pub approved: bool,
  pub priority: i64,
  /// Whose work it is, and whether the item names that worker.
  pub worker: String,
  pub worker_assigned: bool,
  pub cowork: bool,
  pub impact: Option<ImpactGrounds>,
  pub job_impact: Option<ImpactGrounds>,
  /// Its instruction in the reading order.
  pub instruction: String,
  pub draft: Option<Draft>,
  pub owner_note: Option<String>,
  pub notes: Vec<NoteReading>,
  pub merged_work: Vec<MergedWork>,
  pub questions: Vec<QuestionReading>,
  pub time: Option<TimeReading>,
  pub report: Option<String>,
  pub unfinished: Option<Unfinished>,
}

fn draft(queue: &Queue, item: &TodoItem) -> Option<Draft> {
  queue
    .work_order(item.id)
    .filter(|order| {
      order.status == "draft" && !draft_repeats_item(&item.instruction, &order.intent, &order.instruction)
    })
    .map(|order| Draft {
      intent: order.intent.clone(),
      instruction: order.instruction.clone(),
    })
}

fn merged_work(queue: &Queue, item: &TodoItem) -> Vec<MergedWork> {
  queue
    .todos_merged_into(item.id)
    .into_iter()
    .filter(|row| !row.done)
    .filter_map(|row| queue.get_todo(row.id))
    .map(|open| MergedWork {
      id: open.id,
      title: open.title.clone(),
      instruction: ordered_instruction(&open.instruction),
      owner_note: open.note.clone(),
      notes: queue.todo_notes(open.id).iter().map(note_reading).collect(),
    })
    .collect()
}

/// The whole of one item as opening it shows it; the item's own records are read into the queue first.
pub fn item_reading(
  queue: &Queue,
  item: &TodoItem,
  facing: &Facing,
  toolbook: Toolbook<'_>,
) -> ItemReading {
  let mut line = item_line(item, facing);

  line.model = model_reading(item, facing, true);

  ItemReading {
    line,
    held_at: item.held_at,
    merged_into: item.merged_into.map(|id| MergedInto {
      id,
      title: queue.get_todo(id).map(|target| target.title.clone()),
      at: item.merged_at,
      follows: item.merged(),
    }),
    merged_here: queue
      .todos_merged_into(item.id)
      .into_iter()
      .map(|row| raised_row(row, RowState::Merged))
      .collect(),
    repairs: item.repairs_todo_id.map(|id| Repairs {
      id,
      title: queue.get_todo(id).map(|target| target.title.clone()),
    }),
    repaired_by: queue
      .todo_repairs(item.id)
      .into_iter()
      .map(|row| LinkedRow {
        id: row.id,
        title: row.title.clone(),
        state: if row.done {
          RowState::Done
        } else if row.held_at.is_some() {
          RowState::Held
        } else {
          RowState::Opened
        },
      })
      .collect(),
    requester: item.requester.clone(),
    requester_id: item.requester_id,
    approved: item.approved,
    priority: item.priority,
    worker: todo_worker(item, &facing.workers),
    worker_assigned: item.worker.is_some(),
    cowork: item.cowork,
    impact: item.impact.clone(),
    job_impact: item.job_impact.clone(),
    instruction: ordered_instruction(&item.instruction),
    draft: draft(queue, item),
    owner_note: item.note.clone(),
    notes: queue.todo_notes(item.id).iter().map(note_reading).collect(),
    merged_work: merged_work(queue, item),
    questions: queue.todo_questions(item.id).iter().map(question_reading).collect(),
    time: time_reading(queue.item_runs(item.id), toolbook),
    report: item.report.clone(),
    unfinished: item.unfinished.as_ref().map(|text| Unfinished {
      text: text.clone(),
      resolved_at: item.unfinished_resolved_at,
      resolution: item.unfinished_resolution.clone(),
      raised: queue
        .todos_raised_from(item.id)
        .into_iter()
        .map(|row| raised_row(row, RowState::Open))
        .collect(),
    }),
  }
}
