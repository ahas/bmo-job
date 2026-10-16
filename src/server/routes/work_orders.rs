//! An order's body changed (docs/api.md, "Work orders"): each action read with what it takes, a field that
//! is not a string refused `field_not_text` and a missing one `field_missing`, and the change handed to the
//! work order's verb, which makes it under the order's lock.

use std::sync::Arc;

use axum::Router;
use axum::routing::post;
use job_base::Refusal;
use job_codes::{ControlRefusal, WorkOrderRefusal};
use serde_json::Value;

use super::{Answered, State, body};
use crate::server::Server;
use crate::server::request::{Body, refused};
use crate::verbs::{OrderAction, Verbs};

pub fn routes<V: Verbs>() -> Router<Arc<Server<V>>> {
  Router::new().route("/work-order", post(change::<V>))
}

// The largest whole number a JavaScript number holds exactly, which a caller in the console writes.
const SAFE: i64 = (1 << 53) - 1;

/// A body as a work order's edit reads it.
struct Order<'a>(&'a Value);

impl Order<'_> {
  /// A number the body carries, where it is whole and within what a JavaScript number holds exactly.
  fn safe(&self, name: &str) -> Option<i64> {
    let value = self.0.get(name)?;

    value
      .as_i64()
      .or_else(|| value.as_f64().filter(|number| number.fract() == 0.0).map(|number| number as i64))
      .filter(|number| number.abs() <= SAFE)
  }

  /// A field the body carries as a string, none where it carries nothing.
  fn field(&self, name: &str) -> Result<Option<String>, Refusal> {
    match self.0.get(name) {
      None | Some(Value::Null) => Ok(None),
      Some(Value::String(text)) => Ok(Some(text.clone())),
      Some(_) => Err(refused(WorkOrderRefusal::FieldNotText {
        name: name.to_owned(),
      })),
    }
  }

  /// A field the body must carry with something in it, as it was written.
  fn required(&self, name: &str) -> Result<String, Refusal> {
    self.field(name)?.filter(|text| !text.trim().is_empty()).ok_or_else(|| {
      refused(WorkOrderRefusal::FieldMissing {
        name: name.to_owned(),
      })
    })
  }

  /// The task a move or an addition is placed beside: before it, or after it where `after` is true.
  fn anchor(&self) -> Result<Option<(String, bool)>, Refusal> {
    match (self.field("before")?, self.field("after")?) {
      (Some(_), Some(_)) => Err(refused(WorkOrderRefusal::BeforeAndAfter)),
      (Some(before), None) => Ok(Some((before, false))),
      (None, Some(after)) => Ok(Some((after, true))),
      (None, None) => Ok(None),
    }
  }

  fn action(&self, asked: Option<&str>) -> Result<OrderAction, Refusal> {
    Ok(match asked {
      Some("resume") => OrderAction::Resume,
      Some(asked @ ("save" | "reopen")) => {
        let (intent, instruction, model) =
          (self.field("intent")?, self.field("instruction")?, self.field("model")?);

        if asked == "save" {
          OrderAction::Save {
            intent,
            instruction,
            model,
          }
        } else {
          OrderAction::Reopen {
            intent,
            instruction,
            model,
          }
        }
      }
      Some(asked @ ("edit" | "rerun")) => {
        let task = self.required("task")?;
        let (instruction, model) = (self.field("instruction")?, self.field("model")?);

        if asked == "edit" {
          OrderAction::Edit {
            task,
            instruction,
            model,
          }
        } else {
          OrderAction::Rerun {
            task,
            instruction,
            model,
          }
        }
      }
      Some("skip") => {
        let why = self.required("why")?;

        OrderAction::Skip {
          task: self.required("task")?,
          why,
        }
      }
      Some("restore") => OrderAction::Restore {
        task: self.required("task")?,
      },
      Some("move") => {
        let Some((anchor, after)) = self.anchor()? else {
          return Err(refused(WorkOrderRefusal::BeforeAndAfter));
        };

        OrderAction::Move {
          task: self.required("task")?,
          anchor,
          after,
        }
      }
      Some("add") => {
        let anchor = self.anchor()?;

        OrderAction::Add {
          title: self.required("title")?,
          instruction: self.required("instruction")?,
          anchor,
        }
      }
      Some("checkout") => OrderAction::Checkout {
        cwd: self.required("cwd")?,
      },
      Some("recover") => match (self.safe("from"), self.safe("to")) {
        (Some(from), Some(to)) if from <= to => OrderAction::Recover { from, to },
        _ => return Err(refused(WorkOrderRefusal::JobRangeForm)),
      },
      Some("close") => OrderAction::Close {
        why: self.required("why")?,
      },
      Some("start") => {
        let instruction = self.field("instruction")?;
        let model = self.field("model")?;
        let chat = self.safe("chat").ok_or_else(|| refused(WorkOrderRefusal::ChatWrong))?;

        OrderAction::Start {
          chat,
          cwd: self.required("cwd")?,
          worker: self.required("worker")?,
          instruction,
          model,
        }
      }
      _ => return Err(refused(ControlRefusal::ActionWrong)),
    })
  }
}

async fn change<V: Verbs>(state: State<V>, Body(asked): Body) -> Answered {
  let order = Order(&asked);
  let id = order.safe("id").ok_or_else(|| refused(WorkOrderRefusal::ItemNumberWrong))?;
  let action = order.action(asked.get("action").and_then(Value::as_str))?;

  body(state.verbs.change_order(id, action).await?)
}
