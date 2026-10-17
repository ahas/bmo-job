//! The queue's items changed from the console (docs/api.md, "The TODO"), through the TODO's verbs where one is
//! the action's own (docs/verbs.md, "The server's verbs"), the closing among them. Only a drop removes a row;
//! every other action leaves the item where it can still be found.

use std::collections::HashSet;

use job_codes::{JobError, TodoRefusal};
use queue_read::blocks::{SAID_INSTRUCTION, render_block};
use queue_read::dates::iso;
use queue_rows::blocks::SOURCE;
use queue_rows::closing::reopen_settled_sources;
use queue_rows::filing::{self, Append, Approving};
use queue_rows::todos::{self, Approval, Hand, Raised, Road, Who};
use queue_rows::{Ground, find};

use super::{Built, ServerHands};
use crate::verbs::todo::Beside;
use crate::verbs::todo::edits;
use crate::verbs::todo::holds::{self, Holding};
use crate::verbs::{Done, TodoAction, TodoChanged, Todos};
use crate::{Refusal, now_ms};

fn refused(refusal: TodoRefusal) -> Refusal {
  JobError::from(refusal).into()
}

fn moved(moved: bool) -> TodoChanged {
  TodoChanged {
    moved,
    ..TodoChanged::default()
  }
}

/// One unfinished note a raised row came out of: the item that left it, its title and the note whole.
struct Source {
  id: i64,
  title: String,
  note: String,
}

/// The instruction of a row raised from the console: the owner's words, then each unfinished note it came
/// out of whole, since the note is the other half of what the work is.
fn raised_instruction(owner: &str, said: &str, raised_at: i64, sources: &[Source]) -> String {
  let from: Vec<String> = sources.iter().map(|source| format!("#{}", source.id)).collect();
  let line = format!(
    "{owner}, {}, from the console over the unfinished notes of {}",
    iso(raised_at),
    from.join(", ")
  );
  let mut blocks = vec![render_block(SAID_INSTRUCTION, &line, said)];

  blocks.extend(
    sources
      .iter()
      .map(|source| render_block(SOURCE, &format!("item #{} {}", source.id, source.title), &source.note)),
  );

  blocks.join("\n\n")
}

impl Built {
  // An item's place in the queue moved: a swap with its neighbour, or the head or the end of the queue.
  // False where it moved nothing.
  async fn shift(&self, id: i64, action: &TodoAction) -> Result<bool, Refusal> {
    let queue = todos::open_items(&self.db).await?;
    let Some(at) = queue.iter().position(|row| row.id == id) else {
      return Ok(false);
    };
    let other = match action {
      TodoAction::Move { up: true } => at.checked_sub(1).and_then(|at| queue.get(at)),
      TodoAction::Move { up: false } => queue.get(at + 1),
      TodoAction::Top => queue.first().filter(|first| first.id != id),
      _ => queue.last().filter(|last| last.id != id),
    };
    let Some(other) = other else {
      return Ok(false);
    };
    let priority = queue[at].priority;

    match action {
      TodoAction::Move { .. } => {
        todos::set_priority(&self.db, id, other.priority).await?;
        todos::set_priority(&self.db, other.id, priority).await?;
      }
      TodoAction::Top => todos::set_priority(&self.db, id, other.priority - 1).await?,
      _ => todos::set_priority(&self.db, id, other.priority + 1).await?,
    }

    Ok(true)
  }

  // The whole queue put in the order given; an order that is not the queue standing now is refused rather
  // than applied, since one read a moment before something moved would put it somewhere nobody asked for.
  async fn reorder(&self, order: &[i64]) -> Result<(), Refusal> {
    let queue: HashSet<i64> = todos::open_items(&self.db).await?.iter().map(|row| row.id).collect();
    let named: HashSet<i64> = order.iter().copied().collect();

    if order.len() != queue.len() || named != queue {
      return Err(refused(TodoRefusal::ReorderStale));
    }

    for (at, row) in (1..).zip(order) {
      todos::set_priority(&self.db, *row, at).await?;
    }

    Ok(())
  }

  // A row raised out of the unfinished notes of the item and of every source named beside it, with the
  // owner's line about it; it waits with the other requests, since raising is not approving.
  async fn raise(
    &self,
    ground: &Ground<'_>,
    id: i64,
    title: &str,
    said: &str,
    named: &[i64],
  ) -> Result<i64, Refusal> {
    let item = find(ground, &id.to_string()).await?;
    let mut sources: Vec<i64> = vec![item.id];

    for source in named {
      if !sources.contains(source) {
        sources.push(*source);
      }
    }

    let mut from = Vec::with_capacity(sources.len());

    for &source in &sources {
      let found = todos::item(&self.db, source)
        .await?
        .ok_or_else(|| refused(TodoRefusal::SourceNotItem { id: source }))?;
      let Some(note) = found.unfinished else {
        return Err(refused(TodoRefusal::SourceLeftNothing { id: source }));
      };

      from.push(Source {
        id: found.id,
        title: found.title,
        note,
      });
    }

    let now = now_ms();
    let instruction = raised_instruction(&self.names.owner, said, now, &from);
    let raised = todos::add_raised(
      &self.db,
      &Raised {
        title,
        instruction: &instruction,
        requester: &self.names.owner,
        requester_id: Some(ground.owner_sender),
        chat_id: self.ground.owner_chat,
        created_at: now,
        sources: &sources,
      },
    )
    .await?;

    // Breaking a strand off a note that was already settled makes it outstanding again.
    reopen_settled_sources(&self.db, &sources).await?;

    Ok(raised)
  }
}

impl Built {
  // An approval in the owner's word, or one taken back, under the hand of the person signed in to the
  // console; BMO is told how the queue stands where it was given.
  async fn approve(
    &self,
    ground: &Ground<'_>,
    selector: &str,
    approve: bool,
    note: Option<&str>,
    by: Option<&str>,
  ) -> Result<(), Refusal> {
    let hand = Hand {
      who: Who::Person(by.unwrap_or(&self.names.owner)),
      road: Road::Console,
    };
    let asked = Approving { approve, note };

    filing::approve_as(ground, selector, &asked, Approval::Owner, &hand).await.map(drop)
  }

  // Every action on the rows' ground of the owner's own hand; a stop needs nothing of it and is taken first.
  async fn change_row(&self, id: i64, action: TodoAction) -> Result<TodoChanged, Refusal> {
    let owner_sender = self.owner_sender().await?;
    let units = self.rows_units();
    let hands = ServerHands { built: self };
    let ground = self.rows_ground(owner_sender, &units, &hands);
    let selector = id.to_string();

    match action {
      TodoAction::Approve { note, by } => {
        self.approve(&ground, &selector, true, note.as_deref(), by.as_deref()).await?;
      }
      TodoAction::Unapprove { note, by } => {
        self.approve(&ground, &selector, false, note.as_deref(), by.as_deref()).await?;
      }
      action @ (TodoAction::Move { .. } | TodoAction::Top | TodoAction::Bottom) => {
        find(&ground, &selector).await?;

        if !self.shift(id, &action).await? {
          return Ok(moved(false));
        }

        self.events.queue_moved(None);
      }
      TodoAction::Reorder { order } => {
        self.reorder(&order).await?;
        self.events.queue_moved(None);
      }
      TodoAction::Drop => edits::drop(&ground, &selector).await.map(drop)?,
      TodoAction::Done => edits::close_as_owner(&ground, id).await.map(drop)?,
      TodoAction::Resolve { why } => edits::resolve(&ground, &selector, &why).await.map(drop)?,
      TodoAction::Raise {
        title,
        said,
        sources,
      } => {
        let raised = self.raise(&ground, id, &title, &said, &sources).await?;

        self.events.queue_moved(None);

        return Ok(TodoChanged {
          moved: true,
          stopped: None,
          raised: Some(raised),
        });
      }
      TodoAction::Append { text } => {
        let asked = Append {
          text: &text,
          by: Some(&self.names.owner),
        };

        filing::append(&ground, &selector, &asked).await.map(drop)?;
      }
      TodoAction::Unappend { note } => {
        filing::unappend(&ground, &selector, Some(note), None).await.map(drop)?
      }
      TodoAction::Hold { why } => {
        let beside = Beside {
          source: &self.ground.source,
          testbed: &self.ground.testbed,
        };
        let asked = Holding {
          why,
          ..Holding::default()
        };

        holds::hold(&ground, &beside, &selector, &asked).await.map(drop)?;
      }
      TodoAction::Release => holds::unhold(&ground, &selector).await.map(drop)?,
      TodoAction::Worker { worker } => {
        edits::worker(&ground, &self.ground.source, &selector, worker.as_deref().unwrap_or("-"))
          .await
          .map(drop)?;
      }
      TodoAction::Cowork { on } => {
        edits::cowork(&ground, &selector, if on { "on" } else { "off" }).await.map(drop)?;
      }
      TodoAction::Stop { why } => return self.stop_change(id, &why).await,
    }

    Ok(moved(true))
  }

  // Stopping is the hold that also ends what runs, through the same lock as every change to a work order; it
  // answers once the item is held, without waiting for the runs to end.
  async fn stop_change(&self, id: i64, why: &str) -> Result<TodoChanged, Refusal> {
    let runs = self.stop_item(id, why, "todo-stopped").await?;

    Ok(TodoChanged {
      moved: true,
      stopped: Some(runs),
      raised: None,
    })
  }
}

impl Todos for Built {
  async fn change_todo(&self, id: i64, action: TodoAction) -> Done<TodoChanged> {
    match action {
      TodoAction::Stop { why } => self.stop_change(id, &why).await,
      action => self.change_row(id, action).await,
    }
  }
}
