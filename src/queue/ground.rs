//! The queue's ground built from the settings in one place (docs/queue.md, "Where the queue stands"): the
//! clients of the units it calls, the host a job's unit is started on, and the telling, the stops and the
//! reading of what runs. The verbs and the server take the queue from here.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use job_agency::{AgencyClient, AgencyError};
use job_base::Settings;
use job_brain::{BrainClient, BrainError};
use job_gate::{GateClient, GateError};
use job_graph::{GraphClient, GraphError};
use job_policy::Policy;
use job_receiver::{ReceiverClient, ReceiverError};
use job_testbed::{TestbedClient, TestbedError};
use job_web::{WebClient, WebError};
use queue_rows::wording::{BannedTerm, set_aside_terms};
use sea_orm::DatabaseConnection;
use url::Url;

use super::job_start::{Host, bus_environment};
use super::running::Running;
use super::stops::Stops;
use super::telling::Teller;
use super::units::{Ports, Units};
use crate::events::Events;
use crate::job_run::calls::RunPorts;

/// The clients the queue's calls go over.
pub type QueuePorts<'a> = Ports<'a, AgencyClient, ReceiverClient, WebClient, TestbedClient, GateClient>;

/// Why no ground was built from the settings.
#[derive(Debug, thiserror::Error)]
pub enum GroundError {
  #[error("the settings have no [server] section, which the queue requires")]
  NoServer,
  #[error("the settings have no [units] section, which the queue requires")]
  NoUnits,
  #[error("the settings have no [host] section, which the queue requires")]
  NoHost,
  #[error("bmo-brain's token cannot be read from {path}: {error}")]
  BrainToken { path: String, error: String },
  #[error(transparent)]
  Agency(#[from] AgencyError),
  #[error(transparent)]
  Receiver(#[from] ReceiverError),
  #[error(transparent)]
  Web(#[from] WebError),
  #[error(transparent)]
  Testbed(#[from] TestbedError),
  #[error(transparent)]
  Gate(#[from] GateError),
  #[error(transparent)]
  Graph(#[from] GraphError),
  #[error(transparent)]
  Brain(#[from] BrainError),
}

/// The clients a job's run makes its own calls over.
pub type RunCalls<'a> = RunPorts<'a, AgencyClient, GateClient, GraphClient, TestbedClient>;

/// Everything the queue stands on that the settings name.
pub struct QueueGround {
  pub agency: AgencyClient,
  pub receiver: ReceiverClient,
  pub web: WebClient,
  pub testbed: TestbedClient,
  pub gate: GateClient,
  pub graph: GraphClient,
  pub brain: BrainClient,
  /// The directory the brain's documents stand in, where one is named.
  pub brain_root: Option<PathBuf>,
  /// The cache siren a co-work branch's answer is told to, where one is named.
  pub siren: Option<Url>,
  /// The server's own address, which a job's run tells the screens the queue moved at.
  pub server: Url,
  /// The deployment's agent, as the records name it.
  pub agent_key: String,
  /// The owner's private chat, where the telling and the work-order runner tell the agent their facts.
  pub owner_chat: i64,
  /// The service's own unit, whose journal a broken run is read in.
  pub service_unit: String,
  /// The checkout the built-in worker works in where its row names none.
  pub source: PathBuf,
  /// The program the user's systemd manager is asked and told through.
  pub systemctl: PathBuf,
  /// How a job's unit is started.
  pub host: Host,
  /// The program a runner is launched through, and the directory it runs in.
  pub launcher: PathBuf,
  pub launcher_directory: PathBuf,
  /// Where a work order's prompts, result documents and check logs are written, one folder to an item.
  pub documents: PathBuf,
  /// The directory whose `bin/` holds the platform's command line.
  pub tree: PathBuf,
  /// The directory bmo-testbed writes its runs' logs in, where one is named.
  pub testbed_logs: Option<PathBuf>,
  /// The directory a worker added with no path is given its own folder under, where one is named.
  pub worker_homes: Option<PathBuf>,
  /// The terms the owner has set aside, read once out of the wording document the settings name.
  pub banned: Vec<BannedTerm>,
}

impl QueueGround {
  /// The ground the settings name, its systemd manager reached on `environment`, the process's own
  /// variables where the service runs.
  pub fn of(settings: &Settings, environment: BTreeMap<String, String>) -> Result<Self, GroundError> {
    let server = settings.server.as_ref().ok_or(GroundError::NoServer)?;
    let units = settings.units.as_ref().ok_or(GroundError::NoUnits)?;
    let host = settings.host.as_ref().ok_or(GroundError::NoHost)?;
    let token = std::fs::read_to_string(&*units.brain_token).map_err(|error| GroundError::BrainToken {
      path: units.brain_token.display().to_string(),
      error: error.to_string(),
    })?;

    Ok(Self {
      agency: AgencyClient::new(units.agency.url().clone())?,
      receiver: ReceiverClient::new(units.receiver.url().clone())?,
      web: WebClient::new(units.web.url().clone())?,
      testbed: TestbedClient::new(host.testbed.to_path_buf())?,
      gate: GateClient::new(units.gate.url().clone())?,
      graph: GraphClient::new(units.graph.url().clone())?,
      brain: BrainClient::new(units.brain.url().clone(), token.trim())?,
      brain_root: host.brain.as_deref().map(PathBuf::from),
      siren: units.siren.as_ref().map(|siren| siren.url().clone()),
      server: server.address.url().clone(),
      agent_key: settings.deployment.agent_key.to_string(),
      owner_chat: settings.deployment.owner_chat.number(),
      service_unit: server.unit.to_string(),
      source: host.source.to_path_buf(),
      systemctl: host.systemctl.to_path_buf(),
      host: Host {
        program: host.runner.to_path_buf(),
        cwd: host.runner_directory.to_path_buf(),
        setenv: vec![("PATH".to_owned(), host.runner_path.clone())],
        environment_file: host.runner_environment.as_deref().map(PathBuf::from),
        locks: host.locks.to_path_buf(),
        prompts: host.prompts.to_path_buf(),
        env: bus_environment(environment),
        systemd_run: host.systemd_run.to_path_buf(),
        lock_wait: host.lock_wait_seconds.duration(),
      },
      launcher: host.launcher.to_path_buf(),
      launcher_directory: host.launcher_directory.to_path_buf(),
      documents: host.documents.to_path_buf(),
      tree: host.tree.to_path_buf(),
      testbed_logs: host.testbed_logs.as_deref().map(PathBuf::from),
      worker_homes: host.worker_homes.as_deref().map(PathBuf::from),
      banned: set_aside_terms(settings),
    })
  }

  /// The ground of the process running now, its systemd manager reached on the process's own variables.
  pub fn of_process(settings: &Settings) -> Result<Self, GroundError> {
    Self::of(settings, std::env::vars().collect())
  }

  /// The queue's calls on other units, over the clients.
  pub fn units(&self) -> QueuePorts<'_> {
    Ports {
      agency: &self.agency,
      receiver: &self.receiver,
      web: &self.web,
      testbed: &self.testbed,
      gate: &self.gate,
      agent_key: &self.agent_key,
    }
  }

  /// A job's run's own calls on other units, over the clients, its server reached at its own address.
  pub fn run_calls(&self) -> RunCalls<'_> {
    RunPorts {
      agency: &self.agency,
      gate: &self.gate,
      graph: &self.graph,
      testbed: &self.testbed,
      server: Some(self.server.clone()),
    }
  }

  /// The reading of what runs.
  pub fn running<'a>(
    &self,
    db: &'a DatabaseConnection,
    units: &'a dyn Units,
  ) -> Running<'a, DatabaseConnection> {
    Running {
      db,
      units,
      systemctl: Some(self.systemctl.clone()),
    }
  }

  /// The stops, giving the runs a cancel reached `settle` to see their own ending.
  pub fn stops<'a>(
    &self,
    db: &'a DatabaseConnection,
    units: &'a dyn Units,
    settle: Duration,
  ) -> Stops<'a, DatabaseConnection> {
    Stops {
      db,
      units,
      systemctl: Some(self.systemctl.clone()),
      settle,
    }
  }

  /// The telling, its facts going into the owner's chat, its news on the event stream where one is given.
  pub fn teller<'a>(
    &self,
    db: &'a DatabaseConnection,
    units: &'a dyn Units,
    policy: &'a Policy,
    events: Option<&'a Events>,
  ) -> Teller<'a> {
    Teller {
      db,
      units,
      policy,
      owner_chat: self.owner_chat,
      source: self.source.clone(),
      service_unit: self.service_unit.clone(),
      systemctl: Some(self.systemctl.clone()),
      events,
    }
  }
}
