//! The service's event stream (docs/api.md, "The event stream"): the ring of the last events, the run
//! their ids count, and every screen listening. Nothing here is stored: the database keeps what was
//! decided, and this keeps the last stretch of news, so a screen that attaches late picks up from the id
//! it last saw.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use job_codes::talk::Unanswered;
use serde::Serialize;
use serde_json::{Value, json};
use tokio::sync::broadcast;

use crate::now_ms;

/// How many events the ring holds: long enough that a screen reconnecting after a dropped stream still
/// finds where it was.
pub const RING: usize = 500;

/// One piece of news, by its kind.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum News {
  /// The queue moved; what it holds is read from the database, so the news carries none of it.
  QueueMoved { detail: Option<String> },
  /// One work order's body changed.
  OrderChanged { item: i64, order: OrderNews },
  /// The talk about an item moved: a turn began, or ended with what failed where it did.
  TalkMoved {
    item: i64,
    failure: Option<Unanswered>,
  },
}

/// A work order as its news gives it: its state and every task it holds, each cut to what a screen
/// follows it by.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OrderNews {
  pub status: Value,
  pub phase: Value,
  pub stage: Value,
  pub updated_at: Value,
  pub tasks: Vec<TaskNews>,
}

/// One task of a work order's news.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TaskNews {
  pub id: Value,
  pub state: Value,
  pub attempt: Value,
  pub job: Value,
  pub model: Value,
  pub error: Value,
}

impl OrderNews {
  /// The news of an order's body: its history, its stages' tasks, its build steps and its control turn.
  pub fn of(order: &Value) -> Self {
    let tasks = queue_read::work_order::places(order)
      .into_iter()
      .map(|(_, task)| TaskNews {
        id: task["id"].clone(),
        state: task["state"].clone(),
        attempt: task["attempt"].clone(),
        job: task["jobId"].clone(),
        model: task["model"].clone(),
        error: task["error"].clone(),
      })
      .collect();

    Self {
      status: order["status"].clone(),
      phase: order["phase"].clone(),
      stage: order["stage"].clone(),
      updated_at: order["updatedAt"].clone(),
      tasks,
    }
  }
}

/// The stream and everything listening to it. Cloned freely: every clone is the one stream.
#[derive(Clone)]
pub struct Events {
  hub: Arc<Mutex<Hub>>,
}

struct Hub {
  run: String,
  next_id: i64,
  ring: VecDeque<Value>,
  sender: broadcast::Sender<Value>,
}

/// What a screen attaching is handed: the run, the events after its cursor, and everything that comes
/// after them.
pub struct Listening {
  pub run: String,
  pub replay: Vec<Value>,
  pub coming: broadcast::Receiver<Value>,
}

impl Default for Events {
  fn default() -> Self {
    Self::new()
  }
}

impl Events {
  /// A stream whose run is this moment.
  pub fn new() -> Self {
    let (sender, _) = broadcast::channel(RING);

    Self {
      hub: Arc::new(Mutex::new(Hub {
        run: now_ms().to_string(),
        next_id: 1,
        ring: VecDeque::new(),
        sender,
      })),
    }
  }

  // A listener that panicked while holding the lock left nothing half written that matters more than the
  // stream going on.
  fn hub(&self) -> MutexGuard<'_, Hub> {
    self.hub.lock().unwrap_or_else(PoisonError::into_inner)
  }

  /// The run the ids count.
  pub fn run(&self) -> String {
    self.hub().run.clone()
  }

  /// One piece of news put into the stream, numbered and stamped here, and told to every listener.
  pub fn publish(&self, news: &News) -> Value {
    let mut hub = self.hub();
    let mut event = serde_json::to_value(news).unwrap_or_else(|_| json!({}));

    event["id"] = json!(hub.next_id);
    event["at"] = json!(now_ms());
    hub.next_id += 1;
    hub.ring.push_back(event.clone());

    while hub.ring.len() > RING {
      hub.ring.pop_front();
    }

    // No screen listening is no failure: the ring keeps the event for the next one.
    let _ = hub.sender.send(event.clone());

    event
  }

  /// The screens told the queue moved, with who moved it where a caller named itself.
  pub fn queue_moved(&self, detail: Option<&str>) {
    self.publish(&News::QueueMoved {
      detail: detail.map(str::to_owned),
    });
  }

  /// One work order's screen told its body changed.
  pub fn order_changed(&self, item: i64, order: &Value) {
    self.publish(&News::OrderChanged {
      item,
      order: OrderNews::of(order),
    });
  }

  /// A screen attaching: what the ring holds after its cursor where the cursor is this run's, everything
  /// it holds where it is not, and what comes next. Taken under the one lock, so nothing published falls
  /// between the replay and the listening.
  pub fn listen(&self, after: i64, run: Option<&str>) -> Listening {
    let hub = self.hub();
    let after = if run == Some(hub.run.as_str()) { after } else { 0 };
    let replay = hub
      .ring
      .iter()
      .filter(|event| event["id"].as_i64().is_some_and(|id| id > after))
      .cloned()
      .collect();

    Listening {
      run: hub.run.clone(),
      replay,
      coming: hub.sender.subscribe(),
    }
  }
}
