//! A research run — a search or a browse — and what it stands on beside its own work (docs/verbs.md,
//! "Research runs"): the profile it launched on, handed to its runner as it stood and read back against the
//! policy's research part; its clock; its branches' places in bmo-agency's co-worker pool, given back however
//! it ends and reclaimed where a run left them behind; the line it runs on and the route its branches take;
//! and the knowledge its words call for.
//!
//! Beside it, what a search's runner adds of its own: `admission`, whether a deeper run goes as asked or as
//! the shallowest and the wait an explicit depth makes for its first seat; `branch`, one branch of a run in a
//! pool place; and `run`, the run itself, opened and closed in bmo-agency's run ledger and carried as a work
//! turn.

pub mod admission;
pub mod branch;
pub mod run;

use std::future::Future;

use job_agency::{Agency, Mapping, Reservation, ReservationAsking, RouteAsk};
use job_base::Refusal;
use job_policy::research::{Range, Research};
use serde_json::{Map, Value, json};

use super::stand::Stand;
use crate::queue::units::agency_failed;

pub use crate::job_run::prompt::knowledge_block;

/// The line every research run of this module goes out on, and the work key its turns run under.
pub const LINE: &str = "search";

/// What a research run's web server is told about the run: each value set is one variable, and one left unset
/// is no variable at all.
#[derive(Clone, Debug, Default)]
pub struct Told {
  /// Epoch milliseconds after which the web tools refuse new work.
  pub closes_at: Option<i64>,
  /// The pool owner the run's branches are held under.
  pub owner: Option<String>,
  pub source: Option<String>,
  pub role: Option<String>,
  pub max_children: Option<i64>,
  pub chat_id: Option<i64>,
  pub strategy: Option<String>,
  pub host_interval_ms: Option<i64>,
  pub engine_interval_ms: Option<i64>,
  /// Whether the server serves the branch tool.
  pub branch_tool: bool,
  /// Whether the server serves the picture viewer.
  pub image_tool: bool,
  /// The rulebooks the server hands out whole by name.
  pub rulebooks: Vec<String>,
}

impl Told {
  /// The variables the server reads them from.
  pub fn variables(&self) -> Vec<(&'static str, String)> {
    let mut told = Vec::new();
    let mut set = |name: &'static str, value: Option<String>| {
      if let Some(value) = value.filter(|value| !value.is_empty()) {
        told.push((name, value));
      }
    };

    set("BMO_RESEARCH_TOOLS", self.branch_tool.then(|| "1".to_owned()));
    set("BMO_RESEARCH_IMAGES", self.image_tool.then(|| "1".to_owned()));
    set("BMO_RESEARCH_RULEBOOKS", Some(self.rulebooks.join(",")));
    set("BMO_RESEARCH_CLOSES_AT", self.closes_at.map(|at| at.to_string()));
    set("BMO_RESEARCH_OWNER", self.owner.clone());
    set("BMO_RESEARCH_SOURCE", self.source.clone());
    set("BMO_RESEARCH_ROLE", self.role.clone());
    set("BMO_RESEARCH_BRANCHES", self.max_children.map(|count| count.to_string()));
    set("BMO_RESEARCH_CHAT", self.chat_id.map(|chat| chat.to_string()));
    set("BMO_WEB_STRATEGY", self.strategy.clone());
    set("BMO_WEB_HOST_INTERVAL_MS", self.host_interval_ms.map(|ms| ms.to_string()));
    set("BMO_WEB_ENGINE_INTERVAL_MS", self.engine_interval_ms.map(|ms| ms.to_string()));
    told
  }
}

/// The moment a run is asked at, as its prompt names it: `2026-09-17 14:40 UTC, Thursday`.
pub fn asked_at_text(at_ms: i64) -> String {
  chrono::DateTime::from_timestamp_millis(at_ms)
    .map_or_else(|| at_ms.to_string(), |at| at.format("%Y-%m-%d %H:%M UTC, %A").to_string())
}

/// One profile's values, each as the snapshot carries it: a value of the wrong kind is kept as it came, for
/// the checks to name.
#[derive(Clone, Debug)]
pub struct Values {
  pub cap_ms: Value,
  pub synthesis_reserve_ms: Value,
  pub child_role: Value,
  pub max_children: Value,
  pub wait_limit_ms: Value,
  pub strategy: Value,
  pub host_interval_ms: Value,
  pub engine_interval_ms: Value,
}

impl Values {
  /// The eight fields out of a snapshot, and nothing else. A field left out is no value at all, which every
  /// check refuses as it refuses a value of the wrong kind.
  pub fn of(raw: &Value) -> Self {
    let field = |name: &str| raw.get(name).cloned().unwrap_or(Value::Bool(false));

    Self {
      cap_ms: field("capMs"),
      synthesis_reserve_ms: field("synthesisReserveMs"),
      child_role: field("childRole"),
      max_children: field("maxChildren"),
      wait_limit_ms: field("waitLimitMs"),
      strategy: field("strategy"),
      host_interval_ms: field("hostIntervalMs"),
      engine_interval_ms: field("engineIntervalMs"),
    }
  }

  /// The values as a snapshot carries them.
  pub fn fields(&self) -> Map<String, Value> {
    let mut fields = Map::new();

    fields.insert("capMs".into(), self.cap_ms.clone());
    fields.insert("synthesisReserveMs".into(), self.synthesis_reserve_ms.clone());
    fields.insert("childRole".into(), self.child_role.clone());
    fields.insert("maxChildren".into(), self.max_children.clone());
    fields.insert("waitLimitMs".into(), self.wait_limit_ms.clone());
    fields.insert("strategy".into(), self.strategy.clone());
    fields.insert("hostIntervalMs".into(), self.host_interval_ms.clone());
    fields.insert("engineIntervalMs".into(), self.engine_interval_ms.clone());

    fields
  }
}

// A number that is whole and within its bounds.
fn whole(value: &Value, min: i64, max: i64) -> Option<i64> {
  value.as_i64().filter(|number| (min..=max).contains(number))
}

fn limit(range: &Range) -> (i64, i64) {
  (range.min.unwrap_or(0), range.max.unwrap_or(i64::MAX))
}

fn role_shape(name: &str) -> bool {
  let mut chars = name.chars();

  chars.next().is_some_and(|first| first.is_ascii_lowercase())
    && name.len() <= 64
    && chars.all(|one| one.is_ascii_lowercase() || one.is_ascii_digit() || one == '_' || one == '-')
}

fn whole_fault(value: &Value, what: &str, min: i64, max: i64) -> Option<String> {
  whole(value, min, max)
    .is_none()
    .then(|| format!("{what} must be a whole number from {min} to {max}"))
}

/// Everything a profile must satisfy on its own and against the pool's roles; nothing where it holds.
pub fn profile_fault(
  research: &Research,
  key: &str,
  values: &Values,
  pooled_roles: &[String],
) -> Option<String> {
  let limits = &research.limits;
  let clocked = research.clocked(key);
  let (cap_min, cap_max) = limit(&limits.cap_ms);
  let clock = if clocked {
    whole_fault(&values.cap_ms, "capMs", cap_min, cap_max).or_else(|| {
      whole_fault(
        &values.synthesis_reserve_ms,
        "synthesisReserveMs",
        limits.synthesis_reserve_ms.min.unwrap_or(0),
        cap_max,
      )
    })
  } else if !values.cap_ms.is_null() || !values.synthesis_reserve_ms.is_null() {
    Some(format!("{key} has no clock, so capMs and synthesisReserveMs must be null"))
  } else {
    None
  };
  let fault = clock
    .or_else(|| whole_fault(&values.max_children, "maxChildren", 0, limit(&limits.max_children).1))
    .or_else(|| whole_fault(&values.wait_limit_ms, "waitLimitMs", 0, limit(&limits.wait_limit_ms).1))
    .or_else(|| whole_fault(&values.host_interval_ms, "hostIntervalMs", 0, limit(&limits.interval_ms).1))
    .or_else(|| {
      whole_fault(&values.engine_interval_ms, "engineIntervalMs", 0, limit(&limits.interval_ms).1)
    });

  if fault.is_some() {
    return fault;
  }

  let research_min = limits.research_ms.min.unwrap_or(0);

  if clocked
    && values.cap_ms.as_i64().unwrap_or(0) - values.synthesis_reserve_ms.as_i64().unwrap_or(0)
      < research_min
  {
    return Some(format!(
      "synthesisReserveMs must leave at least {} seconds of research inside capMs",
      research_min / 1000
    ));
  }

  let strategies = research.strategies(key);

  if !values
    .strategy
    .as_str()
    .is_some_and(|strategy| strategies.iter().any(|one| one == strategy))
  {
    return Some(format!("strategy must be one of {}", strategies.join(", ")));
  }

  let children = values.max_children.as_i64().unwrap_or(0);

  if research.branchless(key) && children != 0 {
    return Some(format!("{key} opens no branches"));
  }

  let Some(role) = values.child_role.as_str().filter(|role| role_shape(role)) else {
    return Some("childRole is not a role name".into());
  };

  (children > 0 && !pooled_roles.iter().any(|one| one == role))
    .then(|| format!("childRole {role} is not an enabled pooled role"))
}

/// One research key's profile as a run launches on it: the key, its values, and when they were set.
#[derive(Clone, Debug)]
pub struct Profile {
  pub key: String,
  pub values: Values,
  pub set_at: i64,
}

impl Profile {
  /// The whole time a clocked run has, none on a key with no clock.
  pub fn cap_ms(&self) -> Option<i64> {
    self.values.cap_ms.as_i64()
  }

  /// The end of that time kept for writing the result, none on a key with no clock.
  pub fn synthesis_reserve_ms(&self) -> Option<i64> {
    self.values.synthesis_reserve_ms.as_i64()
  }

  pub fn child_role(&self) -> &str {
    self.values.child_role.as_str().unwrap_or_default()
  }

  pub fn max_children(&self) -> i64 {
    self.values.max_children.as_i64().unwrap_or_default()
  }

  pub fn wait_limit_ms(&self) -> i64 {
    self.values.wait_limit_ms.as_i64().unwrap_or_default()
  }

  pub fn strategy(&self) -> &str {
    self.values.strategy.as_str().unwrap_or_default()
  }

  pub fn host_interval_ms(&self) -> i64 {
    self.values.host_interval_ms.as_i64().unwrap_or_default()
  }

  pub fn engine_interval_ms(&self) -> i64 {
    self.values.engine_interval_ms.as_i64().unwrap_or_default()
  }

  /// The profile a run kept on its caller's own registration launches on: no branch at any depth, so it
  /// reserves and waits for nothing in the pool and its web server serves no branch.
  pub fn kept_on(mut self, registration: Option<&str>) -> Self {
    if registration.is_some() {
      self.values.max_children = json!(0);
    }

    self
  }

  /// The launch-time copy a launcher hands its runner, so a run keeps the settings it started on.
  pub fn snapshot(&self) -> String {
    let mut fields = Map::new();

    fields.insert("key".into(), json!(self.key));
    fields.extend(self.values.fields());
    fields.insert("setAt".into(), json!(self.set_at));

    Value::Object(fields).to_string()
  }

  /// A launch-time copy read back for the key it must be for, and refused where it is not that key's or breaks
  /// a check the policy holds a field to.
  pub fn parse(research: &Research, text: &str, key: &str) -> Result<Self, String> {
    let parsed: Value =
      serde_json::from_str(text).map_err(|_| "The research profile snapshot is not JSON".to_owned())?;

    if !parsed.is_object() {
      return Err("The research profile snapshot is not an object".to_owned());
    }

    let held = parsed.get("key").cloned().unwrap_or(Value::Null);

    if held.as_str() != Some(key) {
      let named = held.as_str().map_or_else(|| held.to_string(), str::to_owned);

      return Err(format!("The research profile snapshot is for {named}, not {key}"));
    }

    let values = Values::of(&parsed);
    let pooled: Vec<String> = match (values.max_children.as_i64(), values.child_role.as_str()) {
      (Some(children), Some(role)) if children > 0 => vec![role.to_owned()],
      _ => vec![],
    };

    if let Some(fault) = profile_fault(research, key, &values, &pooled) {
      return Err(format!("The research profile snapshot is invalid: {fault}"));
    }

    Ok(Self {
      key: key.to_owned(),
      values,
      set_at: parsed.get("setAt").and_then(Value::as_i64).unwrap_or_default(),
    })
  }
}

/// A clocked run's clock: when it started, when its research closes, and its ceiling, each in epoch
/// milliseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clock {
  pub started_at: i64,
  pub closes_at: i64,
  pub cap_at: i64,
}

/// The clock of a run on this profile started now, none on a key with no clock.
pub fn clock(profile: &Profile, started_at: i64) -> Option<Clock> {
  let (cap, reserve) = (profile.cap_ms()?, profile.synthesis_reserve_ms()?);

  Some(Clock {
    started_at,
    closes_at: started_at + cap - reserve,
    cap_at: started_at + cap,
  })
}

/// The owner a research run's places are held under in the pool.
pub fn owner(source: &str, run_id: i64) -> String {
  format!("{source}-run:{run_id}")
}

/// The run of the ledger an owner of the pool names, where it is a research run's: its source and number.
pub fn owner_run(owner: &str) -> Option<(&str, i64)> {
  let (kind, id) = owner.split_once("-run:")?;

  if !matches!(kind, "search" | "browse") || id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit())
  {
    return None;
  }

  Some((kind, id.parse().ok()?))
}

/// The provider the search line starts on: the first provider in bmo-agency's order that is on and carries
/// the line.
pub async fn line_provider(stand: &Stand) -> Result<String, Refusal> {
  let providers = stand.ground.agency.providers().await.map_err(agency_failed)?;

  providers
    .into_iter()
    .find(|provider| provider.enabled && provider.lines.iter().any(|line| line == LINE))
    .map(|provider| provider.name)
    .ok_or_else(|| agency_failed(format!("no provider that is on carries the {LINE} line")))
}

/// The route a role's branches take: the role's tier on the provider the search line starts on.
pub async fn child_mapping(stand: &Stand, role: &str) -> Result<Mapping, Refusal> {
  let agency = &stand.ground.agency;
  let provider = line_provider(stand).await?;
  let roles = agency.pool_roles().await.map_err(agency_failed)?;
  let Some(held) = roles.into_iter().find(|one| one.name == role) else {
    return Err(agency_failed(format!("the pool knows no role {role}")));
  };
  let asked = RouteAsk {
    work_key: LINE.to_owned(),
    provider: Some(provider),
    tier: held.tier,
  };

  agency
    .resolve_route(&asked)
    .await
    .map_err(agency_failed)?
    .ok_or_else(|| agency_failed(format!("no route serves the {role} role on the {LINE} line")))
}

/// The seats a research run's branches will use, held from before its clock starts until it ends, on the route
/// its branches take.
pub async fn reserve_places(
  stand: &Stand,
  owner: &str,
  source: &str,
  role: &str,
  count: u32,
) -> Result<Reservation, Refusal> {
  let mapping = child_mapping(stand, role).await?;
  let asking = ReservationAsking {
    reservation_key: format!("{owner}:branches"),
    owner: owner.to_owned(),
    source: source.to_owned(),
    role: role.to_owned(),
    work_key: LINE.to_owned(),
    provider: mapping.provider,
    model: mapping.model,
    effort: mapping.effort,
    tier: mapping.tier,
    wanted: count,
  };

  stand.ground.agency.reserve_seats(&asking).await.map_err(agency_failed)
}

/// Everything one run still holds, given back when that run ends however it ended: its branches'
/// reservation, and every request of its own; how many were.
pub async fn finish_leases(stand: &Stand, owner: &str, outcome: &str) -> Result<i64, Refusal> {
  let agency = &stand.ground.agency;
  let mut finished = 0;

  for held in agency.held_reservations(Some(owner)).await.map_err(agency_failed)? {
    if agency.release_reservation(&held.token, outcome).await.map_err(agency_failed)? {
      finished += 1;
    }
  }

  for request in agency.open_requests(Some(owner)).await.map_err(agency_failed)? {
    if request.asking.reservation.is_some() {
      continue;
    }

    let waiting = request.state == job_agency::PoolState::Waiting;

    if agency.finish_request(&request.token, outcome, waiting).await.map_err(agency_failed)? {
      finished += 1;
    }
  }

  Ok(finished)
}

// Whether the research run an owner names has ended: its ledger entry closed and its unit no longer running,
// or no entry at all. Time alone proves nothing, since a branch inside a standing unit may still be running.
async fn ended<F, Fut>(stand: &Stand, owner: &str, running: &F) -> Result<bool, Refusal>
where
  F: Fn(String) -> Fut,
  Fut: Future<Output = bool>,
{
  let Some((_, id)) = owner_run(owner) else { return Ok(false) };
  let Some(run) = stand.ground.agency.run(id).await.map_err(agency_failed)? else {
    return Ok(true);
  };

  Ok(run.finished_at.is_some() && !running(run.unit).await)
}

/// What research runs left behind when their process died before they could give it back: only a run whose
/// ledger entry is closed and whose unit no longer runs is reclaimed; how many places came back.
pub async fn reclaim_leases<F, Fut>(stand: &Stand, running: F) -> Result<i64, Refusal>
where
  F: Fn(String) -> Fut,
  Fut: Future<Output = bool>,
{
  let agency = &stand.ground.agency;
  let mut reclaimed = 0;

  for held in agency.held_reservations(None).await.map_err(agency_failed)? {
    if ended(stand, &held.asking.owner, &running).await?
      && agency
        .release_reservation(&held.token, "research run ended without giving its seats back")
        .await
        .map_err(agency_failed)?
    {
      reclaimed += 1;
    }
  }

  for request in agency.open_requests(None).await.map_err(agency_failed)? {
    if request.asking.reservation.is_some() || !ended(stand, &request.asking.owner, &running).await? {
      continue;
    }

    let waiting = request.state == job_agency::PoolState::Waiting;

    if agency
      .finish_request(&request.token, "research run ended without giving its place back", waiting)
      .await
      .map_err(agency_failed)?
    {
      reclaimed += 1;
    }
  }

  Ok(reclaimed)
}
