//! Whether an item has a route to run on now (docs/queue.md, "Routes"): the selections its job may go out
//! on, in the order a job walks them, and whether one of them is not blocked. The queue's telling asks it
//! of each item at the head of a worker's line, so a worker is told open only where its turn can actually
//! be launched. The providers, their routes and a worker's pin are bmo-agency's.

use std::sync::LazyLock;

use job_agency::{Provider, RouteAsk};
use job_codes::{JobError, ProviderRefusal};
use job_policy::Policy;
use regex::Regex;
use sea_orm::ConnectionTrait;
use serde_json::Value;

use super::settings;
use super::units::Units;
use crate::Refusal;

/// The work key every job of a worker goes out on.
pub const JOB_WORK_KEY: &str = "job-worker";

/// The provider that runs as the deployment's agent, whose road is shut while the agent rests.
pub const CLAUDE_CODE: &str = "claude-code";

/// The prefix of the setting a failure that closed a road is kept under.
pub const ROUTE_BLOCK_PREFIX: &str = "job_route_block:";

// `<provider>::<model>`, as a selection is written.
static CHOICE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"^([a-z0-9-]+)::([a-zA-Z0-9][a-zA-Z0-9._:/-]{0,200})$").expect("the selection pattern builds")
});

// A Claude model named on its own, as the oldest launches name one.
static CLAUDE_MODEL: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^claude-[a-zA-Z0-9._-]+$").expect("the Claude model pattern builds"));

/// One selection read: the provider and the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Choice<'a> {
  pub provider: &'a str,
  pub model: &'a str,
}

/// What a walk of the roads stands on: the providers bmo-agency lists, read once, the database the
/// closed roads are kept in, and the policy's profile of the job's work key.
pub struct Roads<'a, C> {
  pub db: &'a C,
  pub units: &'a dyn Units,
  pub providers: Vec<Provider>,
  /// The run line a job goes out on, which a provider that does not carry it never answers.
  pub line: String,
  /// The tier a job runs at where none is asked.
  pub tier: String,
}

fn refused(refusal: ProviderRefusal) -> Refusal {
  JobError::from(refusal).into()
}

impl<'a, C: ConnectionTrait> Roads<'a, C> {
  /// The roads as they stand now.
  pub async fn read(db: &'a C, units: &'a dyn Units, policy: &Policy) -> Result<Self, Refusal> {
    let profile = policy.work_keys.profile(JOB_WORK_KEY);

    Ok(Self {
      db,
      units,
      providers: units.providers().await?,
      line: profile.map(|profile| profile.line.clone()).unwrap_or_default(),
      tier: profile.map(|profile| profile.tier.clone()).unwrap_or_default(),
    })
  }

  fn provider(&self, name: &str) -> Option<&Provider> {
    self.providers.iter().find(|provider| provider.name == name)
  }

  fn enabled(&self, name: &str) -> bool {
    self.provider(name).is_some_and(|provider| provider.enabled)
  }

  fn carries_line(&self, provider: &Provider) -> bool {
    provider.lines.contains(&self.line)
  }

  /// A selection as a provider and its model, where it names a provider bmo-agency lists.
  pub fn choice<'v>(&self, value: &'v str) -> Option<Choice<'v>> {
    let read = CHOICE.captures(value)?;
    let provider = read.get(1)?.as_str();

    self.provider(provider).is_some().then(|| Choice {
      provider,
      model: read.get(2).map_or("", |model| model.as_str()),
    })
  }

  /// Whether a value is a selection at all: `auto`, a provider's model, or a Claude model on its own.
  pub fn is_selection(&self, value: &str) -> bool {
    value == "auto" || self.choice(value).is_some() || CLAUDE_MODEL.is_match(value)
  }

  // What a worker is pinned to: a provider, and one model of it where the provider's models are
  // registrations of their own. A pin to a provider the owner turned off reads as none.
  async fn worker_pin(&self, worker: &str) -> Result<Option<(String, Option<String>)>, Refusal> {
    let Some(pin) = self.units.worker_runtime(worker).await?.and_then(|runtime| runtime.pin) else {
      return Ok(None);
    };
    let Some(provider) = self.provider(&pin.provider).filter(|provider| provider.enabled) else {
      return Ok(None);
    };
    let model = if provider.registered { pin.model } else { None };

    Ok(Some((pin.provider, model)))
  }

  /// The worker's selection on each provider the walk asks, in the provider order, or on the one provider
  /// the worker is pinned to.
  pub async fn worker_selections(
    &self,
    tier: Option<&str>,
    worker: Option<&str>,
  ) -> Result<Vec<String>, Refusal> {
    let pin = match worker {
      Some(worker) => self.worker_pin(worker).await?,
      None => None,
    };

    if let Some((provider, Some(model))) = &pin {
      return Ok(vec![format!("{provider}::{model}")]);
    }

    let order: Vec<String> = match pin {
      Some((provider, None)) => vec![provider],
      _ => self
        .providers
        .iter()
        .filter(|provider| provider.enabled && self.carries_line(provider))
        .map(|provider| provider.name.clone())
        .collect(),
    };
    let mut selections = vec![];

    for provider in order {
      let asked = RouteAsk {
        work_key: JOB_WORK_KEY.to_owned(),
        provider: Some(provider.clone()),
        tier: tier.unwrap_or(&self.tier).to_owned(),
      };

      if let Some(mapping) = self.units.resolve_route(&asked).await? {
        selections.push(format!("{provider}::{}", mapping.model));
      }
    }

    Ok(selections)
  }

  /// The selection a job asked for as it goes out: a provider's model as it is named, a bare Claude model
  /// on Claude Code, and the automatic choice on the first the worker's walk asks.
  pub async fn resolve_job_model(
    &self,
    selection: Option<&str>,
    worker: Option<&str>,
  ) -> Result<String, Refusal> {
    if let Some(selection) = selection.filter(|selection| *selection != "auto" && !selection.is_empty()) {
      if !self.is_selection(selection) {
        return Err(refused(ProviderRefusal::JobModelWrong {
          selection: selection.to_owned(),
        }));
      }

      return Ok(if self.choice(selection).is_some() {
        selection.to_owned()
      } else {
        format!("{CLAUDE_CODE}::{selection}")
      });
    }

    self
      .worker_selections(None, worker)
      .await?
      .into_iter()
      .next()
      .ok_or_else(|| refused(ProviderRefusal::NoJobModel))
  }

  /// Every selection a job of a worker may go out on now, in the order the job walks them: the one it
  /// asked for where it named one, then the worker's walk on its tier, each kept only where its provider
  /// is on and carries the job's line.
  pub async fn job_route_candidates(
    &self,
    requested: Option<&str>,
    worker: Option<&str>,
    tier: Option<&str>,
  ) -> Result<Vec<String>, Refusal> {
    let mut choices: Vec<String> = vec![];

    if let Some(requested) = requested.filter(|requested| *requested != "auto" && !requested.is_empty())
      && let Some(asked) = self.choice(requested)
      && self.enabled(asked.provider)
    {
      choices.push(self.resolve_job_model(Some(requested), worker).await?);
    }

    for selection in self.worker_selections(tier, worker).await? {
      if !choices.contains(&selection) {
        choices.push(selection);
      }
    }

    Ok(
      choices
        .into_iter()
        .filter(|selection| {
          self
            .choice(selection)
            .and_then(|asked| self.provider(asked.provider))
            .is_some_and(|provider| provider.enabled && self.carries_line(provider))
        })
        .collect(),
    )
  }

  /// The setting a failure closes a road under: the provider's, or on a provider whose models are
  /// registrations of their own the one model's that did not answer.
  pub fn health_key(&self, selection: &Choice<'_>) -> String {
    if self.provider(selection.provider).is_some_and(|provider| provider.registered) {
      format!("{ROUTE_BLOCK_PREFIX}{}::{}", selection.provider, selection.model)
    } else {
      format!("{ROUTE_BLOCK_PREFIX}{}", selection.provider)
    }
  }

  /// Whether a selection's road is shut now: its provider off, Claude Code while the agent rests, or a
  /// failure that closed it still standing.
  pub async fn route_blocked(&self, selection: &str, now: i64) -> Result<bool, Refusal> {
    let Some(asked) = self.choice(selection) else {
      return Ok(false);
    };

    if !self.enabled(asked.provider) {
      return Ok(true);
    }

    if asked.provider == CLAUDE_CODE && self.units.agent_resting().await? {
      return Ok(true);
    }

    let block = settings::get(self.db, &self.health_key(&asked)).await?;
    let until = block
      .and_then(|text| serde_json::from_str::<Value>(&text).ok())
      .and_then(|read| read.get("until").and_then(Value::as_i64));

    Ok(until.is_some_and(|until| until > now))
  }

  /// Whether an item has a route to run on now: some selection its job may go out on that is not
  /// blocked. A walk that could not be read is no route.
  pub async fn item_route_open(&self, model: Option<&str>, worker: Option<&str>, now: i64) -> bool {
    let Ok(candidates) = self.job_route_candidates(Some(model.unwrap_or("auto")), worker, None).await
    else {
      return false;
    };

    for selection in candidates {
      match self.route_blocked(&selection, now).await {
        Ok(false) => return true,
        Ok(true) => {}
        Err(_) => return false,
      }
    }

    false
  }
}
