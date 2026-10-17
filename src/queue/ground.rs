//! The queue's ground built from the settings in one place (docs/queue.md, "Where the queue stands"): the
//! clients of the units it calls, the host a job's unit is started on, and the telling, the stops and the
//! reading of what runs. The verbs and the server take the queue from here.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use job_agency::{AgencyClient, AgencyError};
use job_base::Settings;
use job_policy::Policy;
use job_receiver::{ReceiverClient, ReceiverError};
use job_testbed::{TestbedClient, TestbedError};
use job_web::{WebClient, WebError};
use sea_orm::DatabaseConnection;

use super::job_start::{Host, bus_environment};
use super::running::Running;
use super::stops::Stops;
use super::telling::Teller;
use super::units::{Ports, Units};
use crate::events::Events;

/// The clients the queue's calls go over.
pub type QueuePorts<'a> = Ports<'a, AgencyClient, ReceiverClient, WebClient, TestbedClient>;

/// Why no ground was built from the settings.
#[derive(Debug, thiserror::Error)]
pub enum GroundError {
  #[error("the settings have no [server] section, which the queue requires")]
  NoServer,
  #[error("the settings have no [units] section, which the queue requires")]
  NoUnits,
  #[error("the settings have no [host] section, which the queue requires")]
  NoHost,
  #[error(transparent)]
  Agency(#[from] AgencyError),
  #[error(transparent)]
  Receiver(#[from] ReceiverError),
  #[error(transparent)]
  Web(#[from] WebError),
  #[error(transparent)]
  Testbed(#[from] TestbedError),
}

/// Everything the queue stands on that the settings name.
pub struct QueueGround {
  pub agency: AgencyClient,
  pub receiver: ReceiverClient,
  pub web: WebClient,
  pub testbed: TestbedClient,
  /// The deployment's agent, as the records name it.
  pub agent_key: String,
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
}

impl QueueGround {
  /// The ground the settings name, its systemd manager reached on `environment`, the process's own
  /// variables where the service runs.
  pub fn of(settings: &Settings, environment: BTreeMap<String, String>) -> Result<Self, GroundError> {
    let server = settings.server.as_ref().ok_or(GroundError::NoServer)?;
    let units = settings.units.as_ref().ok_or(GroundError::NoUnits)?;
    let host = settings.host.as_ref().ok_or(GroundError::NoHost)?;

    Ok(Self {
      agency: AgencyClient::new(units.agency.url().clone())?,
      receiver: ReceiverClient::new(units.receiver.url().clone())?,
      web: WebClient::new(units.web.url().clone())?,
      testbed: TestbedClient::new(host.testbed.to_path_buf())?,
      agent_key: settings.deployment.agent_key.to_string(),
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
      agent_key: &self.agent_key,
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
    owner_chat: i64,
    events: Option<&'a Events>,
  ) -> Teller<'a> {
    Teller {
      db,
      units,
      policy,
      owner_chat,
      source: self.source.clone(),
      service_unit: self.service_unit.clone(),
      systemctl: Some(self.systemctl.clone()),
      events,
    }
  }
}
