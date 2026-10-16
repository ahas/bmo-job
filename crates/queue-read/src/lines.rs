//! Which rows a listing shows and whose work an item is (docs/reading.md, "Which rows a listing shows" and
//! "Whose work an item is"), the same for a person's reading and a worker's.

use std::collections::HashSet;

use job_agency::Provider;

use crate::queue::{HoldKind, TodoItem, WorkerFace};

/// The worker a row naming none, or naming one no longer standing, is the work of.
pub const BUILTIN_WORKER: &str = "worker";

/// What a listing or an opened item is read against, beyond the rows themselves.
pub struct Facing {
  pub workers: Vec<WorkerFace>,
  /// Held items whose run is still going.
  pub stopping: HashSet<i64>,
  /// The providers as bmo-agency lists them, in the provider order.
  pub providers: Vec<Provider>,
}

/// Which rows a listing is asked for.
#[derive(Default)]
pub struct Wanted {
  pub all: bool,
  pub pending_only: bool,
  pub held_only: bool,
  pub merged_only: bool,
  pub chat_id: Option<i64>,
}

/// Whose work an item is: the worker it names, a support worker's base, or the built-in worker.
pub fn todo_worker(item: &TodoItem, known: &[WorkerFace]) -> String {
  let named = item.worker.as_ref().and_then(|worker| known.iter().find(|one| &one.name == worker));

  match named {
    None => BUILTIN_WORKER.to_owned(),
    Some(one) => match one.support_of.as_deref() {
      Some(base) if !base.is_empty() => base.to_owned(),
      _ => one.name.clone(),
    },
  }
}

/// The rows a listing shows, in the order the queue keeps them.
pub fn todo_list_items<'a>(items: &'a [TodoItem], wanted: &Wanted, facing: &Facing) -> Vec<&'a TodoItem> {
  items
    .iter()
    .filter(|item| wanted.all || !item.done)
    .filter(|item| !wanted.pending_only || !item.approved)
    .filter(|item| {
      if wanted.merged_only {
        item.merged()
      } else if wanted.held_only {
        item.held_at.is_some()
      } else {
        !item.merged()
          && (item.held_at.is_none()
            || facing.stopping.contains(&item.id)
            || item.held_kind == Some(HoldKind::Pause))
      }
    })
    .filter(|item| wanted.chat_id.is_none() || item.chat_id == wanted.chat_id)
    .collect()
}
