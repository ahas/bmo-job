//! The verbs the binary is built on (docs/verbs.md, "The server's verbs"): one type carrying every trait the
//! server stands on, over the queue's ground, the policy, the deployment's names and the server's own event
//! stream, so what a verb moves is news to the screens listening.

mod orders;
mod pool;
mod questions;
mod runners;
mod stops;
mod task;
mod telling;
mod todos;
mod words;
mod workers;

use std::sync::Arc;
use std::time::Duration;

use futures_util::future::BoxFuture;
use job_agency::{AgencyClient, Provider};
use job_base::{Refusal, Settings};
use job_brain::BrainClient;
use job_codes::QueueTold;
use job_policy::{Names, Policy, PolicyError};
use job_receiver::{Receiver, ReceiverClient};
use queue_rows::closing::Reader;
use queue_rows::{Filer, Ground, Hands, Ports, WordsAdded, Written, work_orders};
use sea_orm::DatabaseConnection;
use tokio::sync::OnceCell;

use crate::events::Events;
use crate::now_ms;
use crate::queue::ground::{GroundError, QueueGround, QueuePorts};
use crate::queue::stops::Stops;
use crate::queue::telling::Teller;
use crate::queue::units::{Units, agency_failed, receiver_failed};
use crate::queue::worker_rules::Judge;

// How long the runs a stop's cancel reached are given to reach their own ending before systemd is asked.
const STOP_SETTLE: Duration = Duration::from_secs(3);

/// Why the verbs could not be built out of the settings.
#[derive(Debug, thiserror::Error)]
pub enum BuiltError {
  #[error(transparent)]
  Ground(#[from] GroundError),
  #[error(transparent)]
  Policy(#[from] PolicyError),
  #[error("the policy's worker name pattern does not build: {0}")]
  WorkerRules(#[from] regex::Error),
}

/// Every verb the server is built on, over the ported queue.
pub struct Built {
  pub db: DatabaseConnection,
  pub ground: Arc<QueueGround>,
  pub policy: Arc<Policy>,
  pub reader: Reader,
  pub names: Names,
  /// The server's own stream, which the news of every verb goes out on.
  pub events: Events,
  /// How long a stop gives the runs its cancel reached before systemd is asked.
  pub settle: Duration,
  /// The owner as a sender, asked of bmo-receiver once and kept.
  owner: OnceCell<i64>,
}

impl Built {
  /// The verbs the settings name, on the event stream given, the process's own variables reaching the
  /// user's systemd manager.
  pub fn of(db: DatabaseConnection, settings: &Settings, events: Events) -> Result<Self, BuiltError> {
    Self::on(db, QueueGround::of_process(settings)?, settings, events)
  }

  /// The verbs on the queue's ground given, the policy and the names read out of the settings.
  pub fn on(
    db: DatabaseConnection,
    ground: QueueGround,
    settings: &Settings,
    events: Events,
  ) -> Result<Self, BuiltError> {
    let policy = Policy::load(settings)?;
    let names = Names::of(&settings.deployment);

    Judge::new(&policy.workers)?;

    Ok(Self {
      db,
      ground: Arc::new(ground),
      reader: Reader::new(&policy.closing, &names),
      policy: Arc::new(policy),
      names,
      events,
      settle: STOP_SETTLE,
      owner: OnceCell::new(),
    })
  }

  /// The queue's calls on other units.
  fn units(&self) -> QueuePorts<'_> {
    self.ground.units()
  }

  /// The telling, its news on the server's stream.
  fn teller<'a>(&'a self, units: &'a dyn Units) -> Teller<'a> {
    self.ground.teller(&self.db, units, &self.policy, Some(&self.events))
  }

  /// The stops.
  fn stops<'a>(&'a self, units: &'a dyn Units) -> Stops<'a, DatabaseConnection> {
    self.ground.stops(&self.db, units, self.settle)
  }

  /// The queue's standing told at a moment that moved it.
  async fn tell_standing(&self, moment: &str, repeat: bool) -> Result<QueueTold, Refusal> {
    let units = self.units();

    self.teller(&units).tell_queue_standing(moment, repeat, None, now_ms()).await
  }

  /// One work order's screen told its body as the database now holds it.
  async fn order_moved(&self, item: i64) {
    match work_orders::read(&self.db, item).await {
      Ok(Some(order)) => self.events.order_changed(item, &order),
      Ok(None) => {}
      Err(failed) => eprintln!("bmo-job: order #{item} was not read for its news: {}", failed.error),
    }
  }

  /// The owner as a sender, as bmo-receiver names him.
  async fn owner_sender(&self) -> Result<i64, Refusal> {
    self
      .owner
      .get_or_try_init(|| async {
        self.ground.receiver.owner().await.map(|owner| owner.sender).map_err(receiver_failed)
      })
      .await
      .copied()
  }

  /// The providers bmo-agency lists.
  async fn providers(&self) -> Result<Vec<Provider>, Refusal> {
    job_agency::Agency::providers(&self.ground.agency).await.map_err(agency_failed)
  }

  /// The rows' calls on other units.
  fn rows_units(&self) -> Ports<'_, AgencyClient, BrainClient, ReceiverClient> {
    Ports {
      agency: &self.ground.agency,
      brain: &self.ground.brain,
      receiver: &self.ground.receiver,
      agent_key: &self.ground.agent_key,
    }
  }

  /// The rows' ground of a console action: the owner's own hand, no job and no agent's turn.
  fn rows_ground<'a>(
    &'a self,
    owner_sender: i64,
    units: &'a dyn queue_rows::Units,
    hands: &'a dyn Hands,
  ) -> Ground<'a> {
    Ground {
      db: &self.db,
      owner_sender,
      owner_chat: self.ground.owner_chat,
      names: &self.names,
      agent_key: &self.ground.agent_key,
      policy: &self.policy,
      reader: &self.reader,
      banned: &self.ground.banned,
      providers: &[],
      in_job: false,
      run_name: None,
      filer: Filer::default(),
      person: Some(&self.names.owner),
      hands,
      units,
      now: now_ms(),
    }
  }
}

/// What the TODO's verbs reach beyond the rows, carried by the server itself: the screens told on its
/// stream, the queue's standing told at once, words added told in the owner's chat, and an item stopped.
struct ServerHands<'a> {
  built: &'a Built,
}

impl Hands for ServerHands<'_> {
  fn moved(&self, work_order: Option<i64>, _written: Option<Written>) -> BoxFuture<'_, ()> {
    Box::pin(async move {
      self.built.events.queue_moved(None);

      if let Some(item) = work_order {
        self.built.order_moved(item).await;
      }
    })
  }

  fn queue_standing<'a>(
    &'a self,
    moment: &'a str,
    repeat: bool,
  ) -> BoxFuture<'a, Result<QueueTold, Refusal>> {
    Box::pin(self.built.tell_standing(moment, repeat))
  }

  fn words_added<'a>(&'a self, added: &'a WordsAdded<'a>) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move {
      let units = self.built.units();

      self.built.teller(&units).tell_words_added(added).await
    })
  }

  fn stop<'a>(&'a self, id: i64, why: &'a str) -> BoxFuture<'a, Result<u32, Refusal>> {
    Box::pin(self.built.stop_item(id, why, "todo-stopped"))
  }
}
