//! Workers' runtime: what bmo-agency holds of a worker as it runs, read off its agent row.

use serde::{Deserialize, Serialize};

/// The provider, and the model where one is named, a worker runs on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pin {
  pub provider: String,
  /// None leaves the model to the route.
  pub model: Option<String>,
}

impl Pin {
  /// The pin as bmo-agency's choice of a provider writes it: the provider, or the provider and its model
  /// joined by `::`.
  pub fn choice(&self) -> String {
    match &self.model {
      Some(model) => format!("{}::{model}", self.provider),
      None => self.provider.clone(),
    }
  }
}

/// A worker's runtime. The pin is the one in force: a support worker with none of its own is answered
/// its base worker's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkerRuntime {
  pub worker: String,
  pub pin: Option<Pin>,
  pub keepalive_paused_at: Option<i64>,
  pub context_tokens: Option<u64>,
}

/// What bmo-agency's agent list answers of an agent, as far as a worker's runtime reads it.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentRow {
  name: String,
  #[serde(default)]
  support_of: Option<String>,
  #[serde(default)]
  pinned_provider: Option<String>,
  #[serde(default)]
  pinned_model: Option<String>,
  #[serde(default)]
  keepalive_paused_at: Option<i64>,
  #[serde(default)]
  context_tokens: Option<u64>,
}

impl AgentRow {
  fn pin(&self) -> Option<Pin> {
    self.pinned_provider.clone().map(|provider| Pin {
      provider,
      model: self.pinned_model.clone(),
    })
  }
}

/// The runtime of the worker named among the agents listed, its pin its base's where it has none of its own;
/// none where no agent of that name is listed.
pub(crate) fn runtime_of(agents: &[AgentRow], worker: &str) -> Option<WorkerRuntime> {
  let row = agents.iter().find(|row| row.name == worker)?;
  let base = || {
    let base = row.support_of.as_deref()?;

    agents.iter().find(|one| one.name == base).and_then(AgentRow::pin)
  };

  Some(WorkerRuntime {
    worker: row.name.clone(),
    pin: row.pin().or_else(base),
    keepalive_paused_at: row.keepalive_paused_at,
    context_tokens: row.context_tokens,
  })
}
