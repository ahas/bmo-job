//! A work order's seats and places in bmo-agency's co-worker pool (docs/queue.md, "Launching a step", "Seats
//! for the next stage"): where a role resolves to, the seats the next stage will use held while a stage runs
//! and kept only while that stage can still use them, every place a task took given back once what took it
//! is gone, and a lease held to the launch it was admitted for. The pool itself — its lines, its limits and
//! its leases — is bmo-agency's.

use std::sync::LazyLock;

use job_agency::{Mapping, PoolRequest, PoolState, Reservation, ReservationAsking, RouteAsk};
use regex::Regex;
use serde_json::{Value, json};

use super::{Fault, Ground, faulted};
use crate::job_run::plan_check::{BUILDER, Roster};
use crate::queue::holds::queue_held;
use crate::queue::turn::orders_in;
use crate::queue::work_orders::{self as orders, current_places, places, task_at, task_at_mut};

/// What a branch of a work order asks the pool as; a builder turn asks as the worker itself.
pub const COWORK: &str = "cowork";
pub const WORKER: &str = "worker";

/// The most places a reservation holds.
pub const MOST_SEATS: u32 = 128;

// A Claude model named on its own, which runs on Claude Code.
static CLAUDE_MODEL: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^claude-[a-zA-Z0-9._-]+$").expect("the Claude model pattern builds"));

/// The provider a Claude model named on its own runs on.
pub const CLAUDE_CODE: &str = "claude-code";

fn text<'v>(value: &'v Value, key: &str) -> Option<&'v str> {
  value.get(key).and_then(Value::as_str)
}

/// Whether a task is a branch of the work — any role but the builder, whose turns are the worker's own.
pub fn branch_task(task: &Value) -> bool {
  text(task, "role") != Some(BUILDER)
}

/// Whether a value names a stored request token of a task.
pub fn token_of(task: &Value) -> Option<&str> {
  text(task, "poolRequest").filter(|token| !token.is_empty())
}

/// A token kept on a task with the generation it was asked at.
pub fn keep_token(task: &mut Value, token: &str, generation: i64) {
  task["poolRequest"] = json!(token);
  task["poolGeneration"] = json!(generation);
}

/// The reservation a co-work holds for one stage's seats on one provider and tier; the stage is in the key,
/// so a reservation says which stage it was made for however far the order has moved.
pub fn stage_reservation_key(todo_id: i64, stage: i64, provider: &str, tier: &str) -> String {
  format!("work-order:{todo_id}:stage-{stage}:{provider}:{tier}")
}

/// The stage a reservation's key was made for.
pub fn reservation_stage(key: &str) -> Option<i64> {
  static STAGE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^work-order:\d+:stage-(\d+):").expect("the stage pattern builds"));

  STAGE.captures(key).and_then(|found| found[1].parse().ok())
}

/// The owner a work order's places and seats are held under.
pub fn owner_of(order: &Value) -> String {
  format!("work-order:{}", orders::number(order, "todoId").unwrap_or_default())
}

/// The work key a role's place resolves under.
pub fn role_key(role: &str) -> String {
  format!("role:{role}")
}

/// What a place in the pool is resolved from: the role, the selection the task names, the tier it asks for,
/// and the provider its caller names where only some carry its line.
#[derive(Clone, Copy, Debug)]
pub struct Wanted<'a> {
  pub role: &'a str,
  pub selection: &'a str,
  pub requested_tier: Option<&'a str>,
  pub provider: Option<&'a str>,
}

/// Where a place runs: for a branch, a provider that lends co-workers; for a builder turn, the worker itself
/// on any provider that is on. The selection chooses a provider where it names one, the first provider that
/// serves the role's route otherwise, and the tier — the one asked, the selection where it is one, or the
/// role's — the model and effort on it.
pub async fn mapping(
  ground: &Ground<'_>,
  roster: &Roster,
  wanted: &Wanted<'_>,
  coworker: bool,
) -> Result<Mapping, Fault> {
  let providers = ground.units.providers().await.map_err(faulted)?;
  let open: Vec<&str> = providers
    .iter()
    .filter(|provider| provider.enabled && (!coworker || provider.coworker))
    .map(|provider| provider.name.as_str())
    .collect();
  let selection = wanted.selection;
  let named = |provider: &str| providers.iter().any(|one| one.name == provider);
  let choice = selection
    .split_once("::")
    .filter(|(provider, model)| named(provider) && !model.is_empty())
    .map(|(provider, _)| provider.to_owned());

  if !roster.holds(selection)
    && choice.is_none()
    && !CLAUDE_MODEL.is_match(selection)
    && selection != "auto"
  {
    return Err(format!("the job model {selection} is no tier, no provider and no model"));
  }

  if let Some(provider) = wanted.provider
    && !open.contains(&provider)
  {
    let on = providers.iter().any(|one| one.name == provider && one.enabled);

    return Err(if on {
      format!("{provider}: the provider serves one model alone, so it takes no co-worker")
    } else {
      format!("{provider}: the run's provider is switched off")
    });
  }

  let tier = match wanted.requested_tier {
    Some(tier) => tier.to_owned(),
    None if roster.holds(selection) => selection.to_owned(),
    None => roster.role_tier(wanted.role)?,
  };
  let explicit = wanted
    .provider
    .map(str::to_owned)
    .or(choice)
    .or_else(|| CLAUDE_MODEL.is_match(selection).then(|| CLAUDE_CODE.to_owned()))
    .filter(|explicit| open.contains(&explicit.as_str()));
  let candidates: Vec<String> = match explicit {
    Some(provider) => vec![provider],
    None => open.iter().map(|provider| (*provider).to_owned()).collect(),
  };

  for provider in candidates {
    let asked = RouteAsk {
      work_key: role_key(wanted.role),
      provider: Some(provider),
      tier: tier.clone(),
    };

    if let Some(found) = ground.units.resolve_route(&asked).await.map_err(faulted)? {
      return Ok(found);
    }
  }

  Err("No enabled provider is available for a co-worker role".to_owned())
}

// Where one branch task is counted: the request it already holds, or where its role and tier resolve; none
// for a task nothing can resolve, which fails on its own terms when it launches.
async fn task_bucket(ground: &Ground<'_>, roster: &Roster, task: &Value) -> Option<(String, String)> {
  if let Some(token) = token_of(task)
    && let Ok(Some(held)) = ground.units.pool_request(token).await
  {
    return Some((held.asking.provider, held.asking.tier));
  }

  let role = text(task, "role").unwrap_or_default();
  let tier = match text(task, "tier") {
    Some(tier) => tier.to_owned(),
    None => roster.role_tier(role).ok()?,
  };
  let wanted = Wanted {
    role,
    selection: text(task, "model").unwrap_or_default(),
    requested_tier: Some(&tier),
    provider: None,
  };

  mapping(ground, roster, &wanted, true)
    .await
    .ok()
    .map(|found| (found.provider, found.tier))
}

// One bucket the next stage will be let in under, with the role, selection and mapping its first task names.
struct NextSeats {
  provider: String,
  tier: String,
  role: String,
  selection: String,
  count: u32,
}

// The seats the stage after this one will use, one entry per provider and tier its pending branch tasks will
// be let in under, in the order the tasks stand.
async fn next_stage_seats(ground: &Ground<'_>, roster: &Roster, order: &Value) -> Vec<NextSeats> {
  let mut seats: Vec<NextSeats> = vec![];

  if text(order, "phase") != Some("research") {
    return seats;
  }

  let next = orders::number(order, "stage").unwrap_or_default() + 1;
  let Some(stage) = order
    .get("stages")
    .and_then(Value::as_array)
    .and_then(|stages| stages.get(usize::try_from(next).ok()?))
  else {
    return seats;
  };

  for task in stage.get("tasks").and_then(Value::as_array).into_iter().flatten() {
    if !branch_task(task) || text(task, "state") != Some("pending") {
      continue;
    }

    let Some((provider, tier)) = task_bucket(ground, roster, task).await else { continue };

    match seats.iter_mut().find(|held| held.provider == provider && held.tier == tier) {
      Some(held) => held.count += 1,
      None => seats.push(NextSeats {
        provider,
        tier,
        role: text(task, "role").unwrap_or_default().to_owned(),
        selection: text(task, "model").unwrap_or_default().to_owned(),
        count: 1,
      }),
    }
  }

  seats
}

/// While a stage runs, the seats the next stage will use held in the pool, which holds them against its
/// limits and keeps their places warm; nothing is held while the queue is held. A reservation the pool
/// refuses is written to the service's log and never stops the stage.
pub async fn reserve_next_stage(ground: &Ground<'_>, roster: &Roster, order: &Value) -> Result<(), Fault> {
  if queue_held(ground.db).await.map_err(faulted)? {
    return Ok(());
  }

  let todo_id = orders::number(order, "todoId").unwrap_or_default();
  let stage = orders::number(order, "stage").unwrap_or_default() + 1;

  for seat in next_stage_seats(ground, roster, order).await {
    let wanted = Wanted {
      role: &seat.role,
      selection: &seat.selection,
      requested_tier: Some(&seat.tier),
      provider: Some(&seat.provider),
    };
    let found = match mapping(ground, roster, &wanted, true).await {
      Ok(found) => found,
      Err(why) => {
        eprintln!("bmo-job: work order {todo_id} reservation: {why}");
        continue;
      }
    };
    let asking = ReservationAsking {
      reservation_key: stage_reservation_key(todo_id, stage, &seat.provider, &seat.tier),
      owner: owner_of(order),
      source: COWORK.to_owned(),
      role: seat.role.clone(),
      provider: found.provider,
      model: found.model,
      effort: found.effort,
      tier: found.tier,
      // A stage is as wide as its plan; past the most any limit seats, the rest of it waits in line
      // unreserved.
      wanted: seat.count.min(MOST_SEATS),
    };

    if let Err(why) = ground.units.reserve_seats(&asking).await {
      eprintln!("bmo-job: work order {todo_id} reservation: {}", why.error);
    }
  }

  Ok(())
}

// Whether a pending branch task of the stage would be let in under the reservation's provider and tier.
async fn waits_for(
  ground: &Ground<'_>,
  roster: &Roster,
  tasks: &[&Value],
  reservation: &Reservation,
) -> bool {
  for task in tasks {
    if text(task, "state") != Some("pending") || !branch_task(task) {
      continue;
    }

    if let Some((provider, tier)) = task_bucket(ground, roster, task).await
      && provider == reservation.asking.provider
      && tier == reservation.asking.tier
    {
      return true;
    }
  }

  false
}

/// A reservation kept while the stage it was made for can still use it: the next stage's while this one
/// runs, and this stage's while a branch task of its provider and tier still waits to be let in. Everything
/// else the order holds is given back — a stopped, paused or finished order holds nothing, and neither does
/// any order while the queue is held.
pub async fn settle_reservations(ground: &Ground<'_>, roster: &Roster, order: &Value) -> Result<(), Fault> {
  let owner = owner_of(order);
  let held = ground.units.held_reservations(Some(&owner)).await.map_err(faulted)?;

  if held.is_empty() {
    return Ok(());
  }

  let status = text(order, "status").unwrap_or_default();
  let stage_now = orders::number(order, "stage").unwrap_or_default();
  let tasks: Vec<&Value> = current_places(order).into_iter().map(|place| task_at(order, place)).collect();
  let carrying = status == "active"
    && text(order, "phase") == Some("research")
    && !queue_held(ground.db).await.map_err(faulted)?;
  let running = tasks.iter().any(|task| text(task, "state") == Some("running"));

  for one in held {
    let stage = reservation_stage(&one.asking.reservation_key);
    let wanted = carrying
      && ((stage == Some(stage_now + 1) && running)
        || (stage == Some(stage_now) && waits_for(ground, roster, &tasks, &one).await));

    if !wanted {
      let outcome = if status == "active" {
        "the stage it was made for has no seat left to fill".to_owned()
      } else {
        format!("work order {status}")
      };

      ground.units.release_reservation(&one.token, &outcome).await.map_err(faulted)?;
    }
  }

  Ok(())
}

/// The reservations of orders no longer carried forward at all — finished, removed or gone — given back.
pub async fn release_orphan_reservations(ground: &Ground<'_>) -> Result<(), Fault> {
  let carried: Vec<String> = orders_in(ground.db, &["active", "paused"])
    .await
    .map_err(faulted)?
    .iter()
    .filter_map(|order| orders::number(order, "todoId"))
    .map(|todo_id| format!("work-order:{todo_id}"))
    .collect();

  for one in ground.units.held_reservations(None).await.map_err(faulted)? {
    if one.asking.source == COWORK
      && one.asking.owner.starts_with("work-order:")
      && !carried.contains(&one.asking.owner)
    {
      ground
        .units
        .release_reservation(&one.token, "work order is no longer carried forward")
        .await
        .map_err(faulted)?;
    }
  }

  Ok(())
}

/// A task's place given back with what it came to, its token taken off the task. A builder turn holds its
/// place until its unit has left the checkout rather than only until its row closed: the unit goes on for
/// seconds after, and the next turn let into that place would land in the checkout beside it.
pub async fn finish_pool(
  ground: &Ground<'_>,
  task: &mut Value,
  outcome: &str,
  cancel: bool,
) -> Result<(), Fault> {
  let token = token_of(task).map(str::to_owned);

  if let Some(task) = task.as_object_mut() {
    task.remove("poolRequest");
  }

  let Some(token) = token else { return Ok(()) };

  if let Some(held) = ground.units.pool_request(&token).await.map_err(faulted)?
    && held.state == PoolState::Active
    && held.asking.source == WORKER
    && let Some(child) = &held.child
    && ground
      .running()
      .active(std::slice::from_ref(child))
      .await
      .map_err(faulted)?
      .contains(child)
  {
    return Ok(());
  }

  ground.units.finish_request(&token, outcome, cancel).await.map(drop).map_err(faulted)
}

/// Every place a work order's turn took, measured against the task holding it: one whose unit has gone and
/// whose task no longer runs given back, and one still waiting that no pending task holds given up. A
/// branch's place goes the same way once no running task holds it and its unit has gone.
pub async fn release_turn_places(ground: &Ground<'_>) -> Result<(), Fault> {
  let turns: Vec<PoolRequest> = ground
    .units
    .open_requests(None)
    .await
    .map_err(faulted)?
    .into_iter()
    .filter(|one| {
      one.asking.owner.starts_with("work-order:")
        && (one.asking.source == WORKER || one.asking.source == COWORK && one.state == PoolState::Active)
    })
    .collect();

  if turns.is_empty() {
    return Ok(());
  }

  // Only the orders the turns name are read, each once.
  let mut todo_ids: Vec<i64> = vec![];

  for one in &turns {
    if let Some(todo_id) = one.asking.owner.split(':').nth(1).and_then(|id| id.parse().ok())
      && !todo_ids.contains(&todo_id)
    {
      todo_ids.push(todo_id);
    }
  }

  let mut holders: Vec<(String, String)> = vec![];

  for todo_id in todo_ids {
    if let Some(order) = queue_rows::work_orders::read(ground.db, todo_id).await.map_err(faulted)? {
      for (_, task) in places(&order) {
        if let Some(token) = token_of(task) {
          holders.push((token.to_owned(), text(task, "state").unwrap_or_default().to_owned()));
        }
      }
    }
  }

  let holder =
    |token: &str| holders.iter().find(|(held, _)| held == token).map(|(_, state)| state.as_str());
  let ended: Vec<String> = turns
    .iter()
    .filter(|one| one.state == PoolState::Active && holder(&one.token) != Some("running"))
    .filter_map(|one| one.child.clone())
    .collect();
  let live = if ended.is_empty() {
    Default::default()
  } else {
    ground.running().active(&ended).await.map_err(faulted)?
  };

  for one in &turns {
    let state = holder(&one.token);

    if one.state == PoolState::Waiting {
      if state != Some("pending") {
        ground
          .units
          .finish_request(&one.token, "no turn waits for this place any more", true)
          .await
          .map_err(faulted)?;
      }

      continue;
    }

    if state == Some("running") || one.child.as_ref().is_some_and(|child| live.contains(child)) {
      continue;
    }

    let outcome = match &one.child {
      None => "the turn it was taken for never launched".to_owned(),
      Some(child) => format!("unit {child}: gone"),
    };

    ground.units.finish_request(&one.token, &outcome, false).await.map_err(faulted)?;
  }

  Ok(())
}

/// The places the current stage's pending tasks wait on given up with why; whether any was.
pub async fn cancel_pending_pools(
  ground: &Ground<'_>,
  order: &mut Value,
  outcome: &str,
) -> Result<bool, Fault> {
  let mut changed = false;

  for place in current_places(order) {
    let task = task_at_mut(order, place);

    if text(task, "state") != Some("pending") || token_of(task).is_none() {
      continue;
    }

    finish_pool(ground, task, outcome, true).await?;
    changed = true;
  }

  Ok(changed)
}

/// A launch held to the lease it was admitted on: active, of the same owner, role, provider and model, and
/// bound to no other unit than the one it starts.
pub fn require(
  held: &PoolRequest,
  owner: &str,
  role: &str,
  unit: &str,
  provider: &str,
  model: &str,
) -> Result<(), Fault> {
  let asked = &held.asking;

  if held.state != PoolState::Active
    || asked.owner != owner
    || asked.role != role
    || asked.provider != provider
    || asked.model != model
    || held.child.as_deref().is_some_and(|child| child != unit)
  {
    return Err("the co-worker place is not the one this launch was admitted on".to_owned());
  }

  Ok(())
}
