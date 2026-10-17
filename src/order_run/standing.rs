//! The work-order runner standing in the service: its tick on its own clock, its units started through the
//! host's systemd-run, a unit stopped the way a stop of the queue stops one, an order that stopped told to the
//! agent and the queue, the screens told an order moved, and a stopped order's change checked through
//! bmo-testbed.

use std::sync::Arc;
use std::time::Duration;

use job_policy::Policy;
use sea_orm::DatabaseConnection;
use serde_json::Value;

use super::Ground;
use super::hold::{Hold, TestbedRepoCheck};
use super::launch::Spawn;
use super::tick::{Hands, Runner, paused_fact};
use crate::events::Events;
use crate::now_ms;
use crate::queue::ground::QueueGround;
use crate::queue::job_start::start_unit;
use crate::queue::units::Units;

// How often the runner looks at the orders.
const TICK: Duration = Duration::from_secs(5);

// How long the runs a stop's cancel reached are given to reach their own ending before systemd is asked.
const STOP_SETTLE: Duration = Duration::from_secs(3);

/// What the runner stands on in the service, owned for as long as the process runs.
pub struct Standing {
  pub queue: Arc<QueueGround>,
  pub db: DatabaseConnection,
  pub policy: Arc<Policy>,
  pub events: Events,
  /// The owner's chat the runner's facts go into.
  pub owner_chat: i64,
}

/// What the runner reaches past the database with, out of the service it stands in.
pub struct ServiceHands<'a> {
  ground: &'a Ground<'a>,
  queue: &'a QueueGround,
  units: &'a dyn Units,
  events: &'a Events,
}

impl Spawn for ServiceHands<'_> {
  async fn spawn(&self, args: &[String]) -> Result<(), String> {
    let (started, said) = start_unit(&self.queue.host, args).await;

    if started {
      Ok(())
    } else {
      Err(format!("systemd-run did not start the unit: {}", said.trim()))
    }
  }
}

impl ServiceHands<'_> {
  async fn tell_standing(&self, moment: &str) {
    let teller = self.queue.teller(
      self.ground.db,
      self.units,
      self.ground.policy,
      self.ground.owner_chat,
      Some(self.events),
    );

    if let Err(failed) = teller.tell_queue_standing(moment, false, None, now_ms()).await {
      eprintln!("bmo-job: the queue's standing was not told: {}", failed.error);
    }
  }
}

impl Hands for ServiceHands<'_> {
  async fn stop(&self, unit: &str) {
    let stops = self.queue.stops(self.ground.db, self.units, STOP_SETTLE);

    match stops.stop(unit).await {
      Ok(None) => {}
      Ok(Some(said)) => eprintln!("bmo-job: {unit} was not stopped: {said}"),
      Err(failed) => eprintln!("bmo-job: {unit} was not stopped: {}", failed.error),
    }
  }

  async fn notify(&self, order: &Value) {
    if order.get("status").and_then(Value::as_str) == Some("paused") {
      self.ground.tell(paused_fact(self.ground.policy, order)).await;
    }

    self.tell_standing("work-order-paused").await;
  }

  async fn freed(&self) {
    self.tell_standing("work-order-freed").await;
  }

  async fn changed(&self, todo_id: i64, order: &Value) {
    self.events.order_changed(todo_id, order);
  }
}

/// The runner's tick on its clock for as long as the process runs, a tick still going when the next is due
/// putting that one off. The tick is driven on a thread of its own on the service's runtime: the queue's turn
/// judgement and the pool's admission hold borrows a task the runtime may move between its threads cannot.
pub fn stand(standing: Standing) {
  let runtime = tokio::runtime::Handle::current();
  let stood = std::thread::Builder::new().name("work-orders".into()).spawn(move || {
    runtime.block_on(async move {
      let queue = &*standing.queue;
      let ports = queue.units();
      let ground = Ground {
        db: &standing.db,
        units: &ports,
        policy: &standing.policy,
        host: &queue.host,
        documents: queue.documents.clone(),
        tree: queue.tree.clone(),
        source: queue.source.clone(),
        systemctl: Some(queue.systemctl.clone()),
        owner_chat: standing.owner_chat,
      };
      let hands = ServiceHands {
        ground: &ground,
        queue,
        units: &ports,
        events: &standing.events,
      };
      let hold = Hold::new(Arc::new(TestbedRepoCheck(queue.testbed.clone())));
      let runner = Runner::new(&ground, hold, hands);
      let mut ticks = tokio::time::interval(TICK);

      ticks.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

      loop {
        ticks.tick().await;

        if let Err(why) = runner.tick().await {
          eprintln!("bmo-job: work orders: {why}");
        }
      }
    });
  });

  if let Err(why) = stood {
    eprintln!("bmo-job: the work-order runner's thread did not start: {why}");
  }
}
