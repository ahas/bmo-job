//! An order's body changed by the console, the command line or the agent's own launch (docs/api.md, "Work
//! orders").

use std::future::Future;

use serde_json::Value;

use super::{Done, Unbuilt};

/// One change to an order, with what it takes. A field left out is none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrderAction {
  Resume,
  Save {
    intent: Option<String>,
    instruction: Option<String>,
    model: Option<String>,
  },
  Reopen {
    intent: Option<String>,
    instruction: Option<String>,
    model: Option<String>,
  },
  Edit {
    task: String,
    instruction: Option<String>,
    model: Option<String>,
  },
  Rerun {
    task: String,
    instruction: Option<String>,
    model: Option<String>,
  },
  Skip {
    task: String,
    why: String,
  },
  Restore {
    task: String,
  },
  /// A task moved before another, or after it where `after` is true.
  Move {
    task: String,
    anchor: String,
    after: bool,
  },
  /// A step added at the end, or before or after an anchor where one is named.
  Add {
    title: String,
    instruction: String,
    anchor: Option<(String, bool)>,
  },
  Checkout {
    cwd: String,
  },
  Recover {
    from: i64,
    to: i64,
  },
  Close {
    why: String,
  },
  Start {
    chat: i64,
    cwd: String,
    worker: String,
    instruction: Option<String>,
    model: Option<String>,
  },
}

/// The orders changed.
pub trait WorkOrders {
  /// The change applied under the order's lock, answering the order as it now stands.
  fn change_order(&self, id: i64, action: OrderAction) -> impl Future<Output = Done<Value>> + Send;
}

impl WorkOrders for Unbuilt {
  fn change_order(&self, _: i64, _: OrderAction) -> impl Future<Output = Done<Value>> + Send {
    Self::refused("change_order")
  }
}
