//! The walk over the providers and the tiers a job's run goes out on (docs/runs.md, "The turns"). Each run the
//! job makes — the job's first, a later turn's, a shaping turn's, a handoff note's — is walked here: a co-work
//! branch's lease read off its task and its pool request, the tier its work key is assigned, the selections
//! in order with a blocked road passed by — a registered model's road closed for an outage asked for its
//! engine's health first — the switch told and the handover put ahead of the prompt, a failure read and its
//! road closed for its while, the rest a full Claude window begins, and the walk's own end where no road is
//! left. Each attempt runs on the road its selection names (drive.rs). Also: the names a work order's task
//! runs under.

use std::sync::LazyLock;

use job_agency::{Mapping, PoolState, Reply, RouteAsk};
use job_codes::Fact;
pub use job_codes::job_roads::RoadFailure;
use job_codes::job_roads::{RoadFact, SwitchCause};
use regex::Regex;
use serde_json::{Value, json};

use super::Ground;
use super::claude::failure::Failure;
use crate::job_run::plan_check::Roster;
use crate::now_ms;
use crate::order_run::pool::{COWORK, owner_of, role_key};
use crate::queue::routes::{JOB_WORK_KEY, Roads};
use crate::queue::settings;
use crate::queue::work_orders;

/// What a task's error opens with once every road its job could go out on has been tried.
pub const ROUTE_EXHAUSTED: &str = "job-route-exhausted:";

/// The setting the selections a job's walk tried are kept under, before the job's number.
pub const ROUTE_ATTEMPTS: &str = "job_route_attempts:";

/// How long a full window rests where the run named no moment it resets.
pub const REST_FALLBACK_MS: i64 = 3_600_000;

// How long a road a failure closed stays closed where the failure names no moment of its own.
fn closed_for_ms(failure: RoadFailure) -> i64 {
  match failure {
    RoadFailure::Quota => 900_000,
    RoadFailure::Outage => 60_000,
    RoadFailure::Access => 300_000,
  }
}

static QUOTA: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)ClaudeLimitError|usage limit|rate.?limit|quota|weekly limit|too many requests|\b429\b|insufficient_quota")
    .expect("the quota wording is a pattern")
});
static OUTAGE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)ClaudeOutageError|api unavailable|overloaded|service unavailable|\b50[0234]\b|ECONNRESET|ECONNREFUSED|ETIMEDOUT|fetch failed|network error|timed? ?out")
    .expect("the outage wording is a pattern")
});
static ACCESS: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)\b40[134]\b|unauthorized|authentication|invalid.api.key|model.*(?:not found|not available|does not exist|not supported)|ENOENT|not installed")
    .expect("the access wording is a pattern")
});

/// What a failure kept as its text alone says of its road, as a stopped task keeps its error; nothing for a
/// failure of anything else, which the walk does not carry past.
pub fn text_failure(said: &str) -> Option<RoadFailure> {
  if QUOTA.is_match(said) {
    Some(RoadFailure::Quota)
  } else if OUTAGE.is_match(said) || said.contains("SelectedRouteUnavailableError") {
    Some(RoadFailure::Outage)
  } else if ACCESS.is_match(said) {
    Some(RoadFailure::Access)
  } else {
    None
  }
}

/// What an attempt that did not answer raised, as the walk reads it: the words it is known by, whether it was
/// a Claude window gone full with the moment it resets and the login it went out on, whether it was the
/// pane's transport failing rather than the road, and whether the road answered by refusing the request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Raised {
  pub said: String,
  pub full_window: bool,
  /// The moment the window resets, in epoch seconds, where the run named one.
  pub resets_at: Option<i64>,
  pub account: Option<String>,
  pub transport: bool,
  pub refused: bool,
}

impl Raised {
  /// What a failed turn raised, as the walk reads it.
  pub fn of(failure: &Failure) -> Self {
    let (full_window, resets_at, account) = match failure {
      Failure::Limit {
        resets_at, account, ..
      } => (true, *resets_at, account.clone()),
      _ => (false, None, None),
    };

    Self {
      said: failure.cause(),
      full_window,
      resets_at,
      account,
      transport: matches!(failure, Failure::Transport { .. }),
      refused: matches!(failure, Failure::Refused { .. }),
    }
  }
}

/// What a failed attempt says of its road: a window gone full, the service out, or no access; nothing for a
/// failure of anything else, which the walk does not carry past. A request the road refused is the road
/// answering, so it closes nothing whatever its words name, and so is the pane failing under the turn.
pub fn road_failure(raised: &Raised) -> Option<RoadFailure> {
  if raised.transport || raised.refused {
    return None;
  }

  text_failure(&raised.said)
}

/// One attempt of a walk: the selection, what the prompt is told of a switch, and the mapping the road goes
/// out on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttemptAsked {
  pub selection: String,
  pub continuation: String,
  pub resolved: Mapping,
}

/// An attempt's answer: the reply, or what it raised.
#[derive(Clone, Debug, PartialEq)]
pub enum Attempted {
  Answered(Box<Reply>),
  Raised(Raised),
}

/// Where each attempt is run, answering with the reply or what it raised; a failure of the attempt's own
/// carrying is the walk's.
pub trait Attempts {
  fn attempt(&mut self, asked: &AttemptAsked) -> impl Future<Output = Result<Attempted, Failure>>;
}

/// The name a task of a work order runs under once a round has carried it on: the round's prefix before an
/// id that has none of its own.
pub fn task_key(order: &Value, task: &Value) -> String {
  static CARRIED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^r\d+-").expect("the carried pattern builds"));
  let round = order.get("round").and_then(Value::as_i64).unwrap_or(1);
  let id = task.get("id").and_then(Value::as_str).unwrap_or_default();

  if round > 1 && !CARRIED.is_match(id) {
    format!("r{round}-{id}")
  } else {
    id.to_owned()
  }
}

/// The step a work order's unit names: its task's key without the attempt.
pub fn unit_step(unit: &str) -> Option<String> {
  static STEP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^bmo-work-\d+-(.+)-\d+$").expect("the step pattern builds"));

  STEP.captures(unit).and_then(|read| read.get(1)).map(|step| step.as_str().to_owned())
}

/// The root a run's executions are counted under: a work order's unit without its attempt, and a task unit's
/// own name; none for anything else.
pub fn execution_root(owner: &str) -> Option<String> {
  if let Some(rest) = owner.strip_prefix("bmo-work-")
    && let Some((head, tail)) = rest.rsplit_once('-')
    && !head.is_empty()
    && !tail.is_empty()
    && tail.bytes().all(|byte| byte.is_ascii_digit())
  {
    return Some(format!("bmo-work-{head}"));
  }

  owner.starts_with("bmo-task-").then(|| owner.to_owned())
}

/// The handover line a run that switched roads opens with, ahead of what it is asked.
pub fn switch_line(previous: &str, selection: &str) -> String {
  format!(
    "\n[Run handed on after a stop]\nThis work goes on on {selection} because the road {previous} could not carry it. The earlier process has ended. First check what of the checkout's edits, commits, run records and outside actions is already done, do not repeat what is finished, and do only what is left.\n"
  )
}

/// What the walk stands on: the job, its worker, unit and item, and whether it is a step of a work order or a
/// co-work branch with a session it throws away.
pub struct Walk<'g> {
  pub ground: &'g Ground<'g>,
  pub job_id: i64,
  pub worker: String,
  pub unit: String,
  pub todo_id: Option<i64>,
  pub managed: bool,
  pub ephemeral: bool,
}

fn error(message: impl Into<String>) -> Failure {
  Failure::error(message)
}

fn refused(why: crate::Refusal) -> Failure {
  error(why.error)
}

impl Walk<'_> {
  /// The work order's task this run is, where the run is a step of one.
  pub async fn managed_task(&self) -> Result<Option<Value>, Failure> {
    let (true, Some(todo)) = (self.managed, self.todo_id) else {
      return Ok(None);
    };
    let Some(order) = queue_rows::work_orders::read(self.ground.db, todo).await.map_err(refused)? else {
      return Ok(None);
    };
    let Some(step) = unit_step(&self.unit) else {
      return Ok(None);
    };

    Ok(
      work_orders::places(&order)
        .into_iter()
        .map(|(_, task)| task)
        .find(|task| task_key(&order, task) == step)
        .cloned(),
    )
  }

  // The co-worker pool lease a co-work branch runs on, as the mapping its walk is held to; none for a run that
  // is no branch. A branch with no durable task or no matching active lease is refused, since a branch run past
  // the pool would take capacity nobody gave it.
  async fn lease_mapping(&self, task: Option<&Value>) -> Result<Option<Mapping>, Failure> {
    if !self.ephemeral {
      return Ok(None);
    }

    let (Some(task), Some(todo)) = (task, self.todo_id) else {
      return Err(error("the co-work branch has no durable task for its co-worker pool lease"));
    };
    let role = task.get("role").and_then(Value::as_str).unwrap_or_default();
    let token = task.get("poolRequest").and_then(Value::as_str).filter(|token| !token.is_empty());
    let lease = match token {
      Some(token) => self.ground.units.pool_request(token).await.map_err(refused)?,
      None => None,
    };
    let owner = owner_of(&json!({ "todoId": todo }));
    let Some(lease) = lease.filter(|lease| {
      lease.state == PoolState::Active
        && lease.asking.owner == owner
        && lease.asking.source == COWORK
        && lease.asking.role == role
        && lease.asking.work_key == role_key(role)
        && lease.child.as_deref() == Some(self.unit.as_str())
    }) else {
      return Err(error("the co-work branch has no matching active co-worker pool lease"));
    };

    Ok(Some(Mapping {
      provider: lease.asking.provider,
      tier: lease.asking.tier,
      model: lease.asking.model,
      effort: lease.asking.effort.filter(|effort| !effort.is_empty()),
      answer_tokens: None,
    }))
  }

  // The tier a task of a work order runs at: its own where it names one, its role's assignment otherwise.
  async fn task_tier(&self, task: Option<&Value>) -> Result<Option<String>, Failure> {
    let Some(task) = task else {
      return Ok(None);
    };

    if let Some(tier) = task.get("tier").and_then(Value::as_str).filter(|tier| !tier.is_empty()) {
      return Ok(Some(tier.to_owned()));
    }

    let role = task.get("role").and_then(Value::as_str).unwrap_or_default();
    let roster = Roster::read(self.ground.units).await.map_err(refused)?;

    roster.role_tier(role).map(Some).map_err(error)
  }

  async fn tell(&self, fact: RoadFact) {
    self.ground.tell(Fact::Road(fact)).await;
  }

  // What one provider's model goes out on for the work key: the tier's mapping, or the one model named where the
  // provider runs models registered one by one and the tier maps to another.
  async fn named_choice(
    &self,
    roads: &Roads<'_, sea_orm::DatabaseConnection>,
    key: &str,
    provider: &str,
    model: &str,
    tier: Option<&str>,
  ) -> Result<Mapping, Failure> {
    let registered = roads.providers.iter().find(|one| one.name == provider);
    let asked = RouteAsk {
      work_key: key.to_owned(),
      provider: Some(provider.to_owned()),
      tier: tier.unwrap_or(&roads.tier).to_owned(),
    };
    let mapping = self.ground.units.resolve_route(&asked).await.map_err(refused)?;

    if !registered.is_some_and(|one| one.registered) {
      return mapping
        .ok_or_else(|| error(format!("no route of {key} serves {provider} at {}", asked.tier)));
    }

    if !registered.is_some_and(|one| one.enabled) {
      return Err(error(format!("{provider}: the provider is switched off")));
    }

    Ok(match mapping {
      Some(mapping) if mapping.model == model => mapping,
      mapping => Mapping {
        provider: provider.to_owned(),
        tier: mapping.map(|mapping| mapping.tier).unwrap_or_else(|| asked.tier.clone()),
        model: model.to_owned(),
        effort: None,
        answer_tokens: None,
      },
    })
  }

  // A road a failure closed, until the moment the failure named or for the failure's own while.
  async fn close_road(
    &self,
    roads: &Roads<'_, sea_orm::DatabaseConnection>,
    selection: &str,
    failure: RoadFailure,
    resets_at: Option<i64>,
  ) -> Result<(), Failure> {
    let Some(chosen) = roads.choice(selection) else {
      return Ok(());
    };
    let now = now_ms();
    let until = match resets_at {
      Some(at) if at * 1000 > now => at * 1000,
      _ => now + closed_for_ms(failure),
    };
    let reason: &str = failure.into();
    let block = json!({ "until": until, "reason": reason });

    settings::set(self.ground.db, &roads.health_key(&chosen), &block.to_string())
      .await
      .map_err(refused)
  }

  // A registered model's road closed for an outage, opened again where the engine behind it answers its health
  // and stands aside for nothing; whether it was. Any other closing waits out its while.
  async fn reopened(
    &self,
    roads: &Roads<'_, sea_orm::DatabaseConnection>,
    selection: &str,
  ) -> Result<bool, Failure> {
    let db = self.ground.db;
    let Some(chosen) = roads.choice(selection) else { return Ok(false) };

    if !roads.providers.iter().any(|one| one.name == chosen.provider && one.registered) {
      return Ok(false);
    }

    let key = roads.health_key(&chosen);
    let closed = settings::get(db, &key).await.map_err(refused)?;
    let outage: &str = RoadFailure::Outage.into();
    let for_outage = closed
      .and_then(|text| serde_json::from_str::<Value>(&text).ok())
      .is_some_and(|block| block.get("reason").and_then(Value::as_str) == Some(outage));

    if !for_outage
      || !self.ground.calls.model_health(chosen.provider, chosen.model).await.map_err(refused)?
    {
      return Ok(false);
    }

    settings::forget(db, &key).await.map_err(refused)?;
    println!(
      "{}: {selection} was closed for an outage and its engine answers its health, so it is open again",
      self.worker
    );

    Ok(true)
  }

  /// One run walked from the selection preferred: each provider not yet tried, in order, until one answers. A
  /// failure the walk does not read is the attempt's own, which its runner keeps and ends the job by.
  pub async fn walk<A: Attempts>(&self, preferred: &str, attempts: &mut A) -> Result<Reply, Failure> {
    let ground = self.ground;
    let db = ground.db;
    let roads = Roads::read(db, ground.units, ground.policy).await.map_err(refused)?;
    let task = self.managed_task().await?;
    let fixed = self.lease_mapping(task.as_ref()).await?;
    let task_tier = self.task_tier(task.as_ref()).await?;
    let work_key = match (&task, self.ephemeral) {
      (Some(task), true) => role_key(task.get("role").and_then(Value::as_str).unwrap_or_default()),
      _ => JOB_WORK_KEY.to_owned(),
    };
    let tier = task_tier.or_else(|| (work_key == JOB_WORK_KEY).then(|| roads.tier.clone()));
    let requested = roads
      .resolve_job_model(Some(preferred), Some(&self.worker))
      .await
      .unwrap_or_else(|_| preferred.to_owned());
    let mut attempted: Vec<String> = vec![];
    let mut providers: Vec<String> = vec![];
    let mut previous = preferred.to_owned();

    loop {
      let requested_provider = if self.ephemeral {
        match roads.choice(&requested) {
          Some(chosen) => Some(chosen.provider.to_owned()),
          None => break,
        }
      } else {
        None
      };
      let candidates = roads
        .job_route_candidates(Some(preferred), Some(&self.worker), tier.as_deref())
        .await
        .map_err(refused)?;
      let mut selection = None;

      for candidate in candidates {
        let Some(chosen) = roads.choice(&candidate) else { continue };

        if requested_provider.as_deref().is_some_and(|provider| provider != chosen.provider)
          || providers.iter().any(|tried| tried == chosen.provider)
        {
          continue;
        }

        let open = !roads.route_blocked(&candidate, now_ms()).await.map_err(refused)?
          || (self.reopened(&roads, &candidate).await?
            && !roads.route_blocked(&candidate, now_ms()).await.map_err(refused)?);

        if open {
          selection = Some(candidate);
          break;
        }
      }

      let Some(selection) = selection else { break };
      let switched = !attempted.is_empty() || selection != requested;

      if switched {
        self
          .tell(RoadFact::RoadSwitching {
            job_id: self.job_id,
            item: self.todo_id,
            worker: self.worker.clone(),
            requested: preferred.to_owned(),
            from: previous.clone(),
            to: selection.clone(),
            cause: if attempted.is_empty() {
              SwitchCause::FirstUnavailable
            } else {
              SwitchCause::PreviousFailed
            },
          })
          .await;
      }

      let chosen = roads.choice(&selection).ok_or_else(|| error("an unreadable selection"))?;
      let (provider, model) = (chosen.provider.to_owned(), chosen.model.to_owned());

      providers.push(provider.clone());
      attempted.push(selection.clone());
      settings::set(db, &format!("{ROUTE_ATTEMPTS}{}", self.job_id), &json!(attempted).to_string())
        .await
        .map_err(refused)?;

      let continuation = if switched { switch_line(&previous, &selection) } else { String::new() };
      let resolved = match &fixed {
        Some(fixed) => fixed.clone(),
        None => self.named_choice(&roads, &work_key, &provider, &model, tier.as_deref()).await?,
      };

      if resolved.provider != provider || (fixed.is_some() && (!self.ephemeral || resolved.model != model))
      {
        return Err(error("a fixed pool mapping must match one single-provider attempt"));
      }

      let answered = attempts
        .attempt(&AttemptAsked {
          selection: selection.clone(),
          continuation,
          resolved,
        })
        .await?;
      let raised = match answered {
        Attempted::Answered(reply) => {
          settings::forget(db, &roads.health_key(&chosen)).await.map_err(refused)?;

          if switched {
            self
              .tell(RoadFact::RoadAnswered {
                job_id: self.job_id,
                item: self.todo_id,
                worker: self.worker.clone(),
                requested: preferred.to_owned(),
                from: previous.clone(),
                to: format!("{}::{}", reply.mode, reply.model.as_deref().unwrap_or_default()),
              })
              .await;
          }

          return Ok(*reply);
        }
        Attempted::Raised(raised) => raised,
      };
      let Some(failure) = road_failure(&raised) else {
        return Err(Failure::Attempt);
      };

      self.close_road(&roads, &selection, failure, raised.resets_at).await?;
      self
        .tell(RoadFact::RoadUnavailable {
          job_id: self.job_id,
          item: self.todo_id,
          worker: self.worker.clone(),
          requested: preferred.to_owned(),
          selection: selection.clone(),
          failure,
        })
        .await;

      // A Claude window gone full rests the login the run went out on, where the walk named one; a run that went
      // out on none rests nothing.
      if raised.full_window
        && let Some(account) = &raised.account
      {
        let until = raised.resets_at.map_or_else(|| now_ms() + REST_FALLBACK_MS, |at| at * 1000);

        ground.calls.rest_login(account, until).await.map_err(refused)?;
      }

      previous = selection;
    }

    self
      .tell(RoadFact::RoadsExhausted {
        job_id: self.job_id,
        item: self.todo_id,
        worker: self.worker.clone(),
        requested: preferred.to_owned(),
        attempted: attempted.clone(),
      })
      .await;

    Err(Failure::Exhausted { attempted })
  }
}
