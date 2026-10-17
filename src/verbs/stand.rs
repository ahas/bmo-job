//! What every verb stands on (docs/verbs.md, "Where a verb stands"): the stand built once from the
//! settings, the markers that tell a job's own process, the server told the queue moved, and what a verb
//! answers.

use std::collections::BTreeMap;
use std::time::Duration;

use job_base::{NodeSettings, Settings};
use job_codes::Fact;
use job_policy::{Names, Policy, PolicyError};
use job_receiver::{Instruction, Owner, Receiver, Said};
use queue_rows::closing::Reader;
use sea_orm::DatabaseConnection;
use serde::Serialize;
use serde_json::{Value, json};
use url::Url;

use crate::job_run::WORK_ORDER_STEP_VARIABLE;
use crate::queue::ground::{GroundError, QueueGround};
use crate::queue::telling::JOB_SORT;
use crate::{Refusal, one};

/// The variable the program a work turn starts sets of itself.
pub const PROGRAM_VARIABLE: &str = "CLAUDECODE";

/// The variable a work turn of a job sets, so a process knows it works for a job.
pub const JOB_TURN_VARIABLE: &str = "BMO_TASK_LOCKED";

/// The variable naming the run a work turn was asked under: a job's unit.
pub const RUN_NAME_VARIABLE: &str = "BMO_RUN_NAME";

// How many database connections a verb holds open at most.
const CONNECTIONS: u32 = 2;

// Short enough that a verb never sits on a server that is restarting.
const MOVED_WAIT: Duration = Duration::from_secs(5);

/// Why no stand was built.
#[derive(Debug, thiserror::Error)]
pub enum StandError {
  #[error("the settings have no [server] section, which the verbs require")]
  NoServer,
  #[error("the database cannot be reached: {0}")]
  Database(#[from] sea_orm::DbErr),
  #[error(transparent)]
  Ground(#[from] GroundError),
  #[error(transparent)]
  Policy(#[from] PolicyError),
  #[error("the owner cannot be read from bmo-receiver: {0}")]
  Owner(String),
}

/// Everything a verb stands on that the settings name.
pub struct Stand {
  pub db: DatabaseConnection,
  pub ground: QueueGround,
  pub policy: Policy,
  pub reader: Reader,
  pub names: Names,
  /// The owner: his sender as bmo-receiver names him, and his private chat from the settings.
  pub owner: Owner,
  /// The server's own address.
  pub server: Url,
  /// The work node this machine is and how the operations machine is reached; none on the operations machine.
  pub node: Option<NodeSettings>,
}

impl Stand {
  /// The stand the settings name, the process's own variables reaching the user's systemd manager.
  pub async fn of(settings: &Settings) -> Result<Self, StandError> {
    let server = settings.server.as_ref().ok_or(StandError::NoServer)?.address.url().clone();
    let ground = QueueGround::of_process(settings)?;
    let policy = Policy::load(settings)?;
    let names = Names::of(&settings.deployment);
    let reader = Reader::new(&policy.closing, &names);
    let sender = ground.receiver.owner().await.map_err(|why| StandError::Owner(why.to_string()))?.sender;
    let owner = Owner {
      sender,
      chat: ground.owner_chat,
    };
    let db = settings.database.url.connect(CONNECTIONS).await?;

    Ok(Self {
      db,
      ground,
      policy,
      reader,
      names,
      owner,
      server,
      node: settings.node.clone(),
    })
  }

  /// The screens told the queue moved, so one showing it reads it again at once; a word that did not land
  /// is said on the error stream and fails nothing.
  pub async fn moved(&self, verb: &str) {
    let missed = match self.server.join("queue/moved") {
      Err(why) => Some(why.to_string()),
      Ok(url) => {
        let told = reqwest::Client::new()
          .post(url)
          .json(&json!({ "detail": format!("{verb}[{}]", std::process::id()) }))
          .timeout(MOVED_WAIT)
          .send()
          .await;

        match told {
          Ok(answer) if answer.status().is_success() => None,
          Ok(answer) => Some(answer.status().to_string()),
          Err(why) => Some(why.to_string()),
        }
      }
    };

    if let Some(missed) = missed {
      eprintln!("{verb}: the screens were not told the queue moved: {missed}");
    }
  }

  /// A fact told to the agent in a room; a fact that was not queued is said on the error stream and fails
  /// nothing.
  pub async fn tell(&self, verb: &str, chat: i64, fact: Fact) {
    let instruction = Instruction {
      agent: None,
      chat: Some(chat),
      said: Said::Fact(fact),
      kind: None,
      sort: JOB_SORT.to_owned(),
      summary: None,
      expires_at: None,
      events: vec![],
      rides_until: None,
      via: None,
      run: None,
    };

    if let Err(why) = self.ground.receiver.queue_instruction(&instruction).await {
      eprintln!("{verb}: the room was not told: {why}");
    }
  }

  /// A request to one of the server's own routes, waited for no longer than `wait`: the body it answered
  /// with, or why it was refused or never answered.
  pub async fn ask_server(&self, path: &str, body: &Value, wait: Duration) -> Result<Value, ServerRefused> {
    let url = self.server.join(path).map_err(|why| ServerRefused::unreached(why.to_string()))?;

    answered(reqwest::Client::new().post(url).json(body).timeout(wait)).await
  }

  /// A reading of one of the server's own routes, waited for no longer than `wait`.
  pub async fn read_server(&self, path: &str, wait: Duration) -> Result<Value, ServerRefused> {
    let url = self.server.join(path).map_err(|why| ServerRefused::unreached(why.to_string()))?;

    answered(reqwest::Client::new().get(url).timeout(wait)).await
  }
}

// A request to the server sent: the body it answered with, or why it was refused or never answered.
async fn answered(request: reqwest::RequestBuilder) -> Result<Value, ServerRefused> {
  let answer = request.send().await.map_err(|why| ServerRefused::unreached(why.to_string()))?;
  let status = answer.status();
  let read: Value = if status == reqwest::StatusCode::NO_CONTENT {
    Value::Null
  } else {
    answer.json().await.unwrap_or(Value::Null)
  };

  if status.is_success() {
    return Ok(read);
  }

  let text = |key: &str| read.get(key).and_then(Value::as_str).map(str::to_owned);

  Err(ServerRefused {
    status: Some(status.as_u16()),
    code: text("code"),
    error: text("error").unwrap_or_else(|| format!("the server answered {status}")),
  })
}

/// A request the server refused or never answered.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{error}")]
pub struct ServerRefused {
  /// The status it answered with; none where it never answered.
  pub status: Option<u16>,
  pub code: Option<String>,
  pub error: String,
}

impl ServerRefused {
  fn unreached(why: String) -> Self {
    Self {
      status: None,
      code: None,
      error: format!("the server did not answer: {why}"),
    }
  }
}

/// What tells a job's own process (docs/verbs.md, "Where a verb stands"): the run it works for, whether it
/// works for a job at all, and whether it is a middle step of a work order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobProcess {
  pub run_name: Option<String>,
  pub in_job: bool,
  pub work_order_step: bool,
}

impl JobProcess {
  /// The markers a set of variables carries.
  pub fn of(env: &BTreeMap<String, String>) -> Self {
    let set = |name: &str| env.get(name).map(|value| value.trim()).filter(|value| !value.is_empty());

    Self {
      run_name: set(RUN_NAME_VARIABLE).map(str::to_owned),
      in_job: set(PROGRAM_VARIABLE).is_some() && set(JOB_TURN_VARIABLE).is_some(),
      work_order_step: set(WORK_ORDER_STEP_VARIABLE) == Some("1"),
    }
  }

  /// The markers this process carries.
  pub fn of_process() -> Self {
    Self::of(&std::env::vars().collect())
  }

  /// The run a job's process works for; none outside a job.
  pub fn job_run(&self) -> Option<&str> {
    self.run_name.as_deref().filter(|_| self.in_job)
  }
}

/// The open job a run is, as its row holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningJob {
  pub id: i64,
  pub todo_id: Option<i64>,
  /// The worker it runs on; the built-in worker where the row names none.
  pub worker: Option<String>,
}

/// The open job whose unit is the run named, the oldest where several are; none where none is open.
pub async fn running_job(db: &DatabaseConnection, run: &str) -> Result<Option<RunningJob>, Refusal> {
  let row = one(
    db,
    r#"SELECT "id", "todo_id", "worker" FROM "jobs" WHERE "finished_at" IS NULL AND "unit" = $1 ORDER BY "id" LIMIT 1"#,
    vec![run.into()],
  )
  .await?;

  row
    .map(|row| -> Result<RunningJob, Refusal> {
      Ok(RunningJob {
        id: row.try_get("", "id")?,
        todo_id: row.try_get("", "todo_id")?,
        worker: row.try_get("", "worker")?,
      })
    })
    .transpose()
}

/// The newest job whose unit is the run named, open or not.
pub async fn last_job(db: &DatabaseConnection, run: &str) -> Result<Option<RunningJob>, Refusal> {
  let row = one(
    db,
    r#"SELECT "id", "todo_id", "worker" FROM "jobs" WHERE "unit" = $1 ORDER BY "id" DESC LIMIT 1"#,
    vec![run.into()],
  )
  .await?;

  row
    .map(|row| -> Result<RunningJob, Refusal> {
      Ok(RunningJob {
        id: row.try_get("", "id")?,
        todo_id: row.try_get("", "todo_id")?,
        worker: row.try_get("", "worker")?,
      })
    })
    .transpose()
}

/// What a verb answers: the lines it prints, and the code it ends with where that is not success — a verb
/// that runs a command ends with that command's code.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Answer {
  pub lines: Vec<String>,
  pub code: u8,
}

impl From<Vec<String>> for Answer {
  fn from(lines: Vec<String>) -> Self {
    Self { lines, code: 0 }
  }
}

impl Answer {
  /// A coded answer, printed as its JSON on one line.
  pub fn coded(answer: &impl Serialize) -> Self {
    vec![serde_json::to_string(answer).unwrap_or_default()].into()
  }
}

/// Why a verb did not do what it was asked, in the line it prints before it ends with a failure.
#[derive(Debug, thiserror::Error)]
pub enum VerbError {
  /// The caller asked for something the verb refuses, said in the verb's own sentence.
  #[error("{0}")]
  Refused(String),
  /// The verb ended with a code of its own, said in its own sentence.
  #[error("{said}")]
  Ended { code: u8, said: String },
  /// The verb did part of what it was asked: the lines it prints of that, and the refusal of the rest.
  #[error("{refusal}")]
  Partly { lines: Vec<String>, refusal: String },
  /// A refusal under a code, or a call on the database or another unit that failed.
  #[error(transparent)]
  Coded(#[from] Refusal),
}

impl From<sea_orm::DbErr> for VerbError {
  fn from(error: sea_orm::DbErr) -> Self {
    Self::Coded(error.into())
  }
}

impl VerbError {
  /// The code a verb that refused ends with.
  pub fn code(&self) -> u8 {
    match self {
      Self::Ended { code, .. } => *code,
      _ => 1,
    }
  }
}

/// A length of time as a person reads one at a glance: seconds under a minute, minutes and seconds under an
/// hour, hours and minutes past it.
pub fn span(seconds: f64) -> String {
  let whole = seconds.max(0.0).round() as i64;

  match whole {
    0..60 => format!("{whole}s"),
    60..3600 => format!("{}m {:02}s", whole / 60, whole % 60),
    _ => format!("{}h {:02}m", whole / 3600, whole / 60 % 60),
  }
}

/// A moment as a verb prints it: UTC to the minute, named as UTC.
pub fn clock(at_ms: i64) -> String {
  chrono::DateTime::from_timestamp_millis(at_ms)
    .map(|at| at.format("%Y-%m-%d %H:%M UTC").to_string())
    .unwrap_or_default()
}
