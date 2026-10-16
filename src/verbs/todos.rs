//! The queue's items changed from the console (docs/api.md, "The TODO").

use std::future::Future;

use serde::Serialize;

use super::{Done, Unbuilt};

/// One change to an item, with what it takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TodoAction {
  Approve {
    note: Option<String>,
    by: Option<String>,
  },
  Unapprove {
    note: Option<String>,
    by: Option<String>,
  },
  Move {
    up: bool,
  },
  Top,
  Bottom,
  Reorder {
    order: Vec<i64>,
  },
  Drop,
  Done,
  Resolve {
    why: String,
  },
  Raise {
    title: String,
    said: String,
    sources: Vec<i64>,
  },
  Append {
    text: String,
  },
  Unappend {
    note: i64,
  },
  Hold {
    why: Option<String>,
  },
  Release,
  Stop {
    why: String,
  },
  Worker {
    worker: Option<String>,
  },
  Cowork {
    on: bool,
  },
}

/// What a change came to.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct TodoChanged {
  /// False where the change moved nothing.
  pub moved: bool,
  /// The runs a stop asked to end.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub stopped: Option<u32>,
  /// The row a raise filed.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub raised: Option<i64>,
}

/// The items changed.
pub trait Todos {
  fn change_todo(&self, id: i64, action: TodoAction) -> impl Future<Output = Done<TodoChanged>> + Send;
}

impl Todos for Unbuilt {
  fn change_todo(&self, _: i64, _: TodoAction) -> impl Future<Output = Done<TodoChanged>> + Send {
    Self::refused("change_todo")
  }
}
