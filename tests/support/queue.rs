//! What the queue's tests stand on: a migrated database of the test's own, on SQLite in a scratch directory
//! or on the test PostgreSQL server in a scratch schema; the policy as a deployment named Owner and Agent
//! reads it; and units that answer what the test set and record every call that writes.

use std::future::Future;
use std::sync::{Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use bmo_job::queue::units::Units;
use bmo_job::{Settings, run};
use futures_util::future::BoxFuture;
use job_agency::{
  Execution, JobLines, LaunchHeld, LogLine, Mapping, Node, PinChoice, PlaceAsk, PlaceRecord,
  PlacesRetirement, PoolAsking, PoolRequest, PoolRole, PoolState, Provider, Reservation, ReservationAsking,
  ReservationState, Room as MachineRoom, RouteAsk, Run, TakenPlace, Tier, Workbench, WorkerRuntime,
};
use job_base::Refusal;
use job_policy::Policy;
use job_receiver::{Answerer, Answerers, Instruction, InstructionState, Notice, Room};
use job_testbed::{Pending, Relinked};
use job_web::{Checked, ObjectCheck};
use migration::{Migrator, MigratorTrait};
use sea_orm::DatabaseConnection;
use tempfile::TempDir;

/// The owner's private chat.
pub const OWNER: i64 = 100;

/// The moment every test stands at, in epoch milliseconds: 2026-09-20 11:09 UTC.
pub const NOW: i64 = 1_789_902_540_000;

const DEPLOYMENT: &str = r#"
[deployment]
owner_name = "Owner"
agent_name = "Agent"
agent_key = "agent"
sender_name = "Agent"
receiver_unit = "receiver"
owner_chat = 100
"#;

fn settings(url: &str) -> Settings {
  Settings::read(&format!("[database]\nurl = \"{url}\"\n{DEPLOYMENT}")).expect("the test settings read")
}

async fn connect(url: &str) -> DatabaseConnection {
  settings(url)
    .database
    .url
    .connect(2)
    .await
    .unwrap_or_else(|error| panic!("{url}: {error}"))
}

/// The policy as the deployment reads it.
pub fn policy() -> Policy {
  Policy::load(&settings("sqlite::memory:")).expect("the policy loads")
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().expect("a test's record is not poisoned")
}

/// Units answering what the test set, recording the instructions, notices and launch holds they are
/// given.
pub struct Fake {
  pub providers: Vec<Provider>,
  pub pins: Mutex<Vec<(String, job_agency::Pin)>>,
  pub resting: bool,
  pub held: Vec<String>,
  pub executions: Vec<Execution>,
  pub instructions: Mutex<Vec<Instruction>>,
  pub notices: Mutex<Vec<(i64, Notice)>>,
  pub launch_held: Mutex<Vec<LaunchHeld>>,
  pub cancelled: Mutex<Vec<String>>,
  /// The names whose cancels were dropped.
  pub dropped: Mutex<Vec<String>>,
  /// Each pool request finished: its token, the outcome and whether it was cancelled.
  pub finished: Mutex<Vec<(String, String, bool)>>,
  pub states: Mutex<Vec<(i64, InstructionState)>>,
  /// The rooms whose registration is gone, their work told in the owner's chat.
  pub removed_rooms: Vec<i64>,
  /// The rooms answered by others than the one agent every other room is.
  pub rooms: Vec<(i64, Answerers)>,
  /// The runs the agency knows.
  pub runs: Vec<Run>,
  pub last_lines: Mutex<Vec<(i64, LogLine)>>,
  /// How often each run has started, by its name; none named is a run that started once.
  pub starts: Vec<(String, u64)>,
  pub tiers: Vec<Tier>,
  pub roles: Vec<PoolRole>,
  pub requests: Mutex<Vec<PoolRequest>>,
  pub reservations: Mutex<Vec<Reservation>>,
  /// The place every take answers; a take answered with no directory leaves `cwd` out.
  pub place: TakenPlace,
  /// Each place asked for, as it was sent.
  pub place_asks: Mutex<Vec<PlaceAsk>>,
  /// Each place laid out: the checkout and the place's directory.
  pub prepared: Mutex<Vec<(String, String)>>,
  /// Each finished branch's place recorded.
  pub remembered: Mutex<Vec<PlaceRecord>>,
  /// Each ended order's places handed back.
  pub retired: Mutex<Vec<PlacesRetirement>>,
}

impl Default for Fake {
  fn default() -> Self {
    let provider = |name: &str| Provider {
      name: name.into(),
      enabled: true,
      registered: false,
      coworker: true,
      lines: vec!["worker".into()],
    };

    Self {
      providers: vec![provider("claude-code"), provider("codex")],
      pins: Mutex::default(),
      resting: false,
      held: vec![],
      executions: vec![],
      instructions: Mutex::default(),
      notices: Mutex::default(),
      launch_held: Mutex::default(),
      cancelled: Mutex::default(),
      dropped: Mutex::default(),
      finished: Mutex::default(),
      states: Mutex::default(),
      removed_rooms: vec![],
      rooms: vec![],
      runs: vec![],
      last_lines: Mutex::default(),
      starts: vec![],
      tiers: vec![],
      roles: vec![],
      requests: Mutex::default(),
      reservations: Mutex::default(),
      place: TakenPlace {
        slot: 1,
        cwd: Some(std::env::temp_dir().join("standing-places").join("1")),
      },
      place_asks: Mutex::default(),
      prepared: Mutex::default(),
      remembered: Mutex::default(),
      retired: Mutex::default(),
    }
  }
}

impl Fake {
  pub fn instructions(&self) -> Vec<Instruction> {
    locked(&self.instructions).clone()
  }

  pub fn notices(&self) -> Vec<(i64, Notice)> {
    locked(&self.notices).clone()
  }

  /// The executions asked cancelled, in order.
  pub fn cancelled(&self) -> Vec<String> {
    locked(&self.cancelled).clone()
  }

  /// The names whose cancels were dropped, in order.
  pub fn dropped(&self) -> Vec<String> {
    locked(&self.dropped).clone()
  }

  /// The pool requests finished, in order.
  pub fn finished(&self) -> Vec<(String, String, bool)> {
    locked(&self.finished).clone()
  }

  pub fn pin(&self, worker: &str, provider: &str, model: Option<&str>) {
    locked(&self.pins).push((
      worker.to_owned(),
      job_agency::Pin {
        provider: provider.to_owned(),
        model: model.map(str::to_owned),
      },
    ));
  }
}

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, Result<T, Refusal>> {
  Box::pin(async move { Ok(value) })
}

impl Units for Fake {
  fn providers(&self) -> BoxFuture<'_, Result<Vec<Provider>, Refusal>> {
    ready(self.providers.clone())
  }

  fn resolve_route<'a>(&'a self, asked: &'a RouteAsk) -> BoxFuture<'a, Result<Option<Mapping>, Refusal>> {
    let provider = asked.provider.clone().unwrap_or_default();

    ready(Some(Mapping {
      model: format!("{provider}-{}", asked.tier.to_lowercase()),
      provider,
      tier: asked.tier.clone(),
      effort: None,
      answer_tokens: None,
    }))
  }

  fn worker_runtime<'a>(
    &'a self,
    worker: &'a str,
  ) -> BoxFuture<'a, Result<Option<WorkerRuntime>, Refusal>> {
    let pin = locked(&self.pins).iter().find(|(name, _)| name == worker).map(|(_, pin)| pin.clone());

    ready(Some(WorkerRuntime {
      worker: worker.to_owned(),
      pin,
      keepalive_paused_at: None,
      context_tokens: None,
    }))
  }

  fn check_pin<'a>(&'a self, value: &'a str) -> BoxFuture<'a, Result<PinChoice, Refusal>> {
    let (provider, model) = value
      .split_once("::")
      .map_or((value, None), |(provider, model)| (provider, Some(model)));

    ready(PinChoice::Pinned(job_agency::Pin {
      provider: provider.to_owned(),
      model: model.map(str::to_owned),
    }))
  }

  fn open_executions(&self) -> BoxFuture<'_, Result<Vec<Execution>, Refusal>> {
    ready(self.executions.clone())
  }

  fn cancel_execution<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.cancelled).push(id.to_owned());

    ready(())
  }

  fn drop_cancels<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<Vec<String>, Refusal>> {
    locked(&self.dropped).push(id.to_owned());

    ready(vec![id.to_owned()])
  }

  fn deployment_resting(&self) -> BoxFuture<'_, Result<bool, Refusal>> {
    ready(self.resting)
  }

  fn nodes(&self) -> BoxFuture<'_, Result<Vec<Node>, Refusal>> {
    ready(vec![Node {
      name: "node-a".into(),
      label: "Node A".into(),
      kind: "ssh".into(),
      role: None,
      state: "up".into(),
      checked_at: None,
    }])
  }

  fn finish_request<'a>(
    &'a self,
    token: &'a str,
    outcome: &'a str,
    cancel: bool,
  ) -> BoxFuture<'a, Result<bool, Refusal>> {
    locked(&self.finished).push((token.to_owned(), outcome.to_owned(), cancel));

    ready(true)
  }

  fn queue_instruction<'a>(&'a self, instruction: &'a Instruction) -> BoxFuture<'a, Result<i64, Refusal>> {
    let mut kept = locked(&self.instructions);

    kept.push(instruction.clone());

    ready(i64::try_from(kept.len()).unwrap_or_default())
  }

  fn instruction_state(&self, id: i64) -> BoxFuture<'_, Result<Option<InstructionState>, Refusal>> {
    let state = locked(&self.states)
      .iter()
      .rev()
      .find(|(one, _)| *one == id)
      .map_or(InstructionState::Pending, |(_, state)| *state);

    ready(Some(state))
  }

  fn withdraw_instruction(&self, _: i64) -> BoxFuture<'_, Result<bool, Refusal>> {
    ready(true)
  }

  fn voice_notice<'a>(&'a self, chat: i64, notice: &'a Notice) -> BoxFuture<'a, Result<i64, Refusal>> {
    locked(&self.notices).push((chat, notice.clone()));

    ready(1)
  }

  fn room(&self, chat: i64) -> BoxFuture<'_, Result<Room, Refusal>> {
    let removed = self.removed_rooms.contains(&chat);

    ready(Room {
      chat,
      owner_room: chat == OWNER,
      registered: !removed,
      work_chat: if removed { OWNER } else { chat },
    })
  }

  fn answerers(&self, chat: i64) -> BoxFuture<'_, Result<Answerers, Refusal>> {
    let named = self
      .rooms
      .iter()
      .find(|(room, _)| *room == chat)
      .map(|(_, answerers)| answerers.clone());

    ready(named.unwrap_or_else(|| Answerers {
      default: "agent".into(),
      voice: "agent".into(),
      answerers: vec![Answerer {
        agent: "agent".into(),
        companion: false,
      }],
    }))
  }

  fn run(&self, run: i64) -> BoxFuture<'_, Result<Option<Run>, Refusal>> {
    ready(self.runs.iter().find(|one| one.id == run).cloned())
  }

  fn claude_account_held<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<bool, Refusal>> {
    ready(true)
  }

  fn model_in_force(&self) -> BoxFuture<'_, Result<bool, Refusal>> {
    ready(true)
  }

  fn user_of_sender(&self, sender: i64) -> BoxFuture<'_, Result<Option<String>, Refusal>> {
    ready(Some(format!("user-{sender}")))
  }

  fn check_object<'a>(&'a self, check: &'a ObjectCheck) -> BoxFuture<'a, Result<Checked, Refusal>> {
    ready(Checked {
      allowed: check.relation == "run",
      grant: format!("{}#{}", check.object, check.relation),
    })
  }

  fn machine_room(&self) -> BoxFuture<'_, Result<MachineRoom, Refusal>> {
    ready(MachineRoom {
      held: self.held.clone(),
      jobs: 0,
      browser_jobs: 0,
      browsers: 0,
    })
  }

  fn tell_launch_held<'a>(&'a self, notice: &'a LaunchHeld) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.launch_held).push(notice.clone());

    ready(())
  }

  fn pending_deploys(&self, _: i64) -> BoxFuture<'_, Result<Vec<Pending>, Refusal>> {
    ready(vec![])
  }

  fn run_starts<'a>(&'a self, _: &'a str, run: &'a str) -> BoxFuture<'a, Result<u64, Refusal>> {
    ready(self.starts.iter().find(|(name, _)| name == run).map_or(1, |(_, starts)| *starts))
  }

  fn job_last_line(&self, job: i64) -> BoxFuture<'_, Result<Option<LogLine>, Refusal>> {
    let line = locked(&self.last_lines)
      .iter()
      .find(|(one, _)| *one == job)
      .map(|(_, line)| line.clone());

    ready(line)
  }

  fn job_lines(&self, _: i64) -> BoxFuture<'_, Result<JobLines, Refusal>> {
    ready(JobLines {
      calls: vec![],
      results: vec![],
      requests: 0,
      compactions: 0,
    })
  }

  fn workbench<'a>(&'a self, worker: &'a str) -> BoxFuture<'a, Result<Workbench, Refusal>> {
    ready(Workbench {
      root: std::env::temp_dir().join(worker),
      scratches: vec![],
    })
  }

  fn tiers(&self) -> BoxFuture<'_, Result<Vec<Tier>, Refusal>> {
    ready(self.tiers.clone())
  }

  fn pool_roles(&self) -> BoxFuture<'_, Result<Vec<PoolRole>, Refusal>> {
    ready(self.roles.clone())
  }

  fn enqueue_request<'a>(&'a self, asking: &'a PoolAsking) -> BoxFuture<'a, Result<PoolRequest, Refusal>> {
    let mut requests = locked(&self.requests);
    let held = requests.iter().find(|held| held.asking.request_key == asking.request_key).cloned();
    let request = held.unwrap_or_else(|| {
      let request = PoolRequest {
        asking: asking.clone(),
        token: format!("token-{}", requests.len() + 1),
        state: PoolState::Waiting,
        enqueued_at: NOW,
        leased_at: None,
        child: None,
        ended_at: None,
        outcome: None,
      };

      requests.push(request.clone());
      request
    });

    ready(request)
  }

  fn pool_request<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Option<PoolRequest>, Refusal>> {
    ready(locked(&self.requests).iter().find(|held| held.token == token).cloned())
  }

  fn open_requests<'a>(
    &'a self,
    owner: Option<&'a str>,
  ) -> BoxFuture<'a, Result<Vec<PoolRequest>, Refusal>> {
    let open = locked(&self.requests)
      .iter()
      .filter(|held| held.state != PoolState::Ended && owner.is_none_or(|owner| held.asking.owner == owner))
      .cloned()
      .collect();

    ready(open)
  }

  fn lease_place<'a>(&'a self, token: &'a str) -> BoxFuture<'a, Result<Option<PoolRequest>, Refusal>> {
    let leased = locked(&self.requests).iter_mut().find(|held| held.token == token).map(|held| {
      if held.state == PoolState::Waiting {
        held.state = PoolState::Active;
        held.leased_at = Some(NOW);
      }

      held.clone()
    });

    Box::pin(async move {
      leased
        .map(Some)
        .ok_or_else(|| Refusal::absent("no_pool_request", "there is no such pool request"))
    })
  }

  fn bind_child<'a>(
    &'a self,
    token: &'a str,
    child: &'a str,
  ) -> BoxFuture<'a, Result<PoolRequest, Refusal>> {
    let bound = locked(&self.requests).iter_mut().find(|held| held.token == token).map(|held| {
      held.child.get_or_insert_with(|| child.to_owned());
      held.clone()
    });

    Box::pin(async move {
      bound.ok_or_else(|| Refusal::absent("no_pool_request", "there is no such pool request"))
    })
  }

  fn reserve_seats<'a>(
    &'a self,
    asking: &'a ReservationAsking,
  ) -> BoxFuture<'a, Result<Reservation, Refusal>> {
    let mut reservations = locked(&self.reservations);
    let held = reservations
      .iter_mut()
      .find(|held| held.asking.reservation_key == asking.reservation_key);
    let reservation = match held {
      Some(held) => {
        held.asking.wanted = asking.wanted;
        held.clone()
      }
      None => {
        let reservation = Reservation {
          asking: asking.clone(),
          token: format!("seats-{}", reservations.len() + 1),
          state: ReservationState::Held,
          created_at: NOW,
        };

        reservations.push(reservation.clone());
        reservation
      }
    };

    ready(reservation)
  }

  fn held_reservations<'a>(
    &'a self,
    owner: Option<&'a str>,
  ) -> BoxFuture<'a, Result<Vec<Reservation>, Refusal>> {
    let held = locked(&self.reservations)
      .iter()
      .filter(|held| {
        held.state == ReservationState::Held && owner.is_none_or(|owner| held.asking.owner == owner)
      })
      .cloned()
      .collect();

    ready(held)
  }

  fn release_reservation<'a>(&'a self, token: &'a str, _: &'a str) -> BoxFuture<'a, Result<bool, Refusal>> {
    let released = locked(&self.reservations)
      .iter_mut()
      .find(|held| held.token == token && held.state == ReservationState::Held)
      .map(|held| held.state = ReservationState::Released)
      .is_some();

    ready(released)
  }

  fn all_executions(&self) -> BoxFuture<'_, Result<Vec<Execution>, Refusal>> {
    ready(self.executions.clone())
  }

  fn take_place<'a>(&'a self, ask: &'a PlaceAsk) -> BoxFuture<'a, Result<TakenPlace, Refusal>> {
    locked(&self.place_asks).push(ask.clone());
    ready(self.place.clone())
  }

  fn prepare_place<'a>(&'a self, source: &'a str, cwd: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.prepared).push((source.to_owned(), cwd.to_owned()));
    ready(())
  }

  fn remember_place<'a>(&'a self, record: &'a PlaceRecord) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.remembered).push(record.clone());
    ready(())
  }

  fn retire_places<'a>(&'a self, retirement: &'a PlacesRetirement) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.retired).push(retirement.clone());
    ready(())
  }

  fn relink_units<'a>(&'a self, _: &'a std::path::Path) -> BoxFuture<'a, Result<Relinked, Refusal>> {
    ready(Relinked {
      relinked: vec![],
      dangling: vec![],
      unmatched: vec![],
    })
  }

  fn english<'a>(&'a self, texts: &'a [String]) -> BoxFuture<'a, Result<Vec<String>, Refusal>> {
    ready(texts.to_vec())
  }
}

/// A migrated database of the test's own, with the scratch directory it stands in.
pub struct Scene {
  _scratch: Option<TempDir>,
  pub db: DatabaseConnection,
  pub policy: Policy,
  pub units: Fake,
}

impl Scene {
  /// A scene on a migrated SQLite database in a scratch directory, gone with the scene.
  pub async fn new() -> Self {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let db = connect(&format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display())).await;

    Migrator::up(&db, None).await.expect("the migrations run up");

    Self::on(db, Some(scratch))
  }

  /// A scene on a database already migrated.
  pub fn on(db: DatabaseConnection, scratch: Option<TempDir>) -> Self {
    Self {
      _scratch: scratch,
      db,
      policy: policy(),
      units: Fake::default(),
    }
  }

  /// A statement run on the scene's database.
  pub async fn run(&self, sql: &str, values: Vec<sea_orm::Value>) {
    run(&self.db, sql, values).await.unwrap_or_else(|failed| panic!("{sql}: {failed}"));
  }

  /// One item, approved and waiting in the owner's room.
  pub async fn item(&self, id: i64, title: &str) {
    self
      .run(
        r#"INSERT INTO "todos" ("id", "title", "instruction", "requester", "chat_id", "approved", "priority", "status", "created_at")
           VALUES ($1, $2, 'What to do.', 'Owner', $3, 1, $1, 'waiting', $4)"#,
        vec![id.into(), title.into(), OWNER.into(), NOW.into()],
      )
      .await;
  }

  /// One job of an item, launched at a moment on a worker; its row's number.
  pub async fn job(&self, todo: Option<i64>, worker: &str, unit: &str, launched_at: i64) -> i64 {
    let row = job_base::one(
      &self.db,
      r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "todo_id", "worker") VALUES ($1, $2, $3, 'Do it.', $4, $5) RETURNING "id""#,
      vec![OWNER.into(), unit.into(), launched_at.into(), todo.into(), worker.into()],
    )
    .await
    .expect("the job is written")
    .expect("the job is numbered");

    row.try_get("", "id").expect("the job's number reads")
  }
}

/// The machine's own PostgreSQL server, as every PostgreSQL test of the service reaches it.
fn test_postgres() -> String {
  std::env::var("BMO_TEST_POSTGRES")
    .unwrap_or_else(|_| "postgres:///bmo_test?host=/var/run/postgresql".into())
}

/// Runs `check` on a migrated database in a scratch schema of its own on the test PostgreSQL server, and
/// drops the schema whatever the check came to.
pub async fn in_postgres<F, Fut>(name: &str, check: F)
where
  F: FnOnce(DatabaseConnection) -> Fut + Send + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  let admin = connect(&test_postgres()).await;
  let seconds = SystemTime::now().duration_since(UNIX_EPOCH).expect("the clock reads").as_secs();
  let schema = format!("t_{seconds}_{}_{name}", std::process::id());

  run(&admin, &format!("CREATE SCHEMA \"{schema}\""), vec![])
    .await
    .expect("the schema is made");

  let separator = if test_postgres().contains('?') { '&' } else { '?' };
  let db = connect(&format!("{}{separator}options[search_path]={schema}", test_postgres())).await;
  let checked = tokio::spawn(async move {
    Migrator::up(&db, None).await.expect("the migrations run up");
    check(db).await;
  })
  .await;

  run(&admin, &format!("DROP SCHEMA \"{schema}\" CASCADE"), vec![])
    .await
    .expect("the schema is dropped");

  if let Err(failure) = checked {
    std::panic::resume_unwind(failure.into_panic());
  }
}
