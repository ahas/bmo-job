//! One research run, from its opening in bmo-agency's run ledger to its closing there (docs/verbs.md,
//! "Research runs"): its model carried as a work turn on the search line's route, on a session thrown away, its
//! web server told the run's settings in the turn's variables, a question its program stops on left standing
//! for a person, a stop asked from outside interrupting the turn and keeping what bmo-agency read it had
//! written, and the run closed once with how it ended and the context it ended carrying, every place it holds
//! in the pool given back.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use job_agency::{
  Agency, ConfirmAnswer, Mapping, Reply, Resume, RouteAsk, RunClosing, RunOpening, TurnAsking, TurnMapping,
  TurnSeat, TurnState,
};
use job_base::Refusal;
use job_gate::{Gate, KnowledgeAsked};
use serde::Serialize;

use super::admission::{Branches, Demand, PLACE_POLL, await_place, unit_running};
use super::{
  LINE, Profile, Told, asked_at_text, clock, finish_leases, knowledge_block, line_provider,
  owner as research_owner, reclaim_leases, reserve_places,
};
use crate::job_run::verdict::live_task_names;
use crate::queue::units::agency_failed;
use crate::verbs::stand::{Answer, Stand, VerbError};
use crate::{now_ms, now_seconds};

/// How often a research run's or a branch's running turn is read again.
pub const TURN_POLL: Duration = Duration::from_secs(1);

/// What one run's web server is told, laid over what every run of the line is: the close its profile's clock
/// gives it where it holds one, the owner its branches are held under, its source and role, its branch
/// allowance — none for a run with no owner — its room, and its fetching strategy and pacing.
pub fn run_told(
  standing: Told,
  profile: &Profile,
  owner: Option<String>,
  source: &str,
  chat_id: i64,
  now: i64,
) -> Told {
  Told {
    closes_at: clock(profile, now).map(|clock| clock.closes_at),
    max_children: Some(if owner.is_some() { profile.max_children() } else { 0 }),
    owner,
    source: Some(source.to_owned()),
    role: Some(profile.child_role().to_owned()),
    chat_id: Some(chat_id),
    strategy: Some(profile.strategy().to_owned()),
    host_interval_ms: Some(profile.host_interval_ms()),
    engine_interval_ms: Some(profile.engine_interval_ms()),
    ..standing
  }
}

/// The tier a research key's run goes out at: the work key's own profile's tier, where the policy declares
/// one.
pub fn key_tier(stand: &Stand, key: &str) -> Option<String> {
  stand.policy.work_keys.profile(key).map(|profile| profile.tier.clone())
}

/// The route a research key's run takes: the tier its launcher took, or else the key's own, on the provider
/// the search line starts on.
pub async fn run_mapping(stand: &Stand, key: &str, tier: Option<&str>) -> Result<Mapping, Refusal> {
  let Some(tier) = tier.map(str::to_owned).or_else(|| key_tier(stand, key)) else {
    return Err(agency_failed(format!("the research key {key} names no tier")));
  };
  let asked = RouteAsk {
    work_key: key.to_owned(),
    provider: Some(line_provider(stand).await?),
    tier,
  };

  stand
    .ground
    .agency
    .resolve_route(&asked)
    .await
    .map_err(agency_failed)?
    .ok_or_else(|| agency_failed(format!("no route serves the research key {key}")))
}

/// How a carried turn ended: its answer, or what it had written by the time it was stopped, or why it gave
/// none.
#[derive(Clone, Debug, PartialEq)]
pub enum Carried {
  Answered(Box<Reply>),
  Stopped(String),
  Failed(String),
}

/// A turn as one research run or branch asks it carried.
pub struct Carry<'a> {
  pub agent: &'a str,
  pub role: Option<&'a str>,
  pub mapping: &'a Mapping,
  pub cwd: &'a str,
  pub name: &'a str,
  pub title: String,
  pub chat: i64,
  pub unit: Option<&'a str>,
  pub told: &'a Told,
  /// How often the running turn is read again.
  pub poll: Duration,
}

/// The turn carried on bmo-agency to its answer, `prompt` on its input, read again until it answers or fails;
/// once `stopping` is set or `closes_at` passes, it is interrupted, and what bmo-agency reads it had written by
/// its end is what a stopped turn keeps. A question its program stops on is left standing for a person: nobody
/// answers one for a research run.
pub async fn carry_turn(
  stand: &Stand,
  carry: &Carry<'_>,
  prompt: &str,
  stopping: &AtomicBool,
  closes_at: Option<i64>,
) -> Result<Carried, Refusal> {
  let turns = &stand.ground.turns;
  let variables: BTreeMap<String, String> = carry
    .told
    .variables()
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect();
  let asking = TurnAsking {
    agent: carry.agent.to_owned(),
    seat: TurnSeat::Branch {
      todo: None,
      role: carry.role.map(str::to_owned),
    },
    provider: carry.mapping.provider.clone(),
    model: carry.mapping.model.clone(),
    mapping: TurnMapping {
      model: carry.mapping.model.clone(),
      effort: carry.mapping.effort.clone(),
      tier: Some(carry.mapping.tier.clone()),
      answer_tokens: carry.mapping.answer_tokens,
    },
    resume: Resume::Fresh,
    cwd: carry.cwd.to_owned(),
    name: carry.name.to_owned(),
    title: carry.title.clone(),
    thread: None,
    job: None,
    item: None,
    chat: carry.chat,
    unit: carry.unit.map(str::to_owned),
    user: None,
    work_key: LINE.to_owned(),
    hears_words: false,
    variables,
  };

  if stopping.load(Ordering::SeqCst) {
    return Ok(Carried::Stopped(String::new()));
  }

  let prepared = turns.prepare(&asking).await.map_err(agency_failed)?;

  turns.send(&prepared.turn, prompt).await.map_err(agency_failed)?;

  let mut interrupted: Option<String> = None;
  let mut left: Option<String> = None;

  loop {
    tokio::time::sleep(carry.poll).await;

    let stop = stopping.load(Ordering::SeqCst) || closes_at.is_some_and(|at| now_ms() >= at);

    if stop && interrupted.is_none() {
      let _ = turns.interrupt(&prepared.turn).await;
      interrupted = Some(String::new());
    }

    // What the interrupted turn has written, read while it is still carried: the reading of its ending lets it go.
    if let Some(written) = interrupted.as_mut()
      && let Ok(Some(output)) = turns.output(&prepared.turn).await
    {
      *written = output.text;
    }

    let Some(turn) = turns.read(&prepared.turn) else {
      return Ok(Carried::Failed("the turn is gone".to_owned()));
    };

    match turn.state {
      TurnState::Answered { reply } => return Ok(Carried::Answered(reply)),
      TurnState::Failed { failure, evidence } => {
        return Ok(match interrupted {
          Some(written) => Carried::Stopped(written),
          None => Carried::Failed(
            evidence
              .or_else(|| failure.map(|failure| format!("{failure:?}")))
              .unwrap_or_else(|| "the turn failed".to_owned()),
          ),
        });
      }
      TurnState::Reprepared => turns.send(&prepared.turn, prompt).await.map_err(agency_failed)?,
      TurnState::Running {
        question: Some(question),
      } if left.as_deref() != Some(question.id.as_str()) => {
        // A question already gone from the turn, answered in the pane or withdrawn, is nothing to leave.
        if let Err(why) = turns.answer(&prepared.turn, &question.id, &ConfirmAnswer::Leave).await {
          eprintln!("bmo-job: research turn {}: its question was not left standing: {why}", carry.name);
        }

        left = Some(question.id);
      }
      TurnState::Running { .. } => {}
    }
  }
}

/// What a run opens its ledger entry with beside its agent, room and unit.
#[derive(Clone, Debug, Default)]
pub struct Opening {
  pub depth: u32,
  pub cap_ms: Option<i64>,
  pub started_by: Option<String>,
  /// The number of the search the run carries, as bmo-artifact numbers its searches.
  pub search_id: Option<i64>,
}

/// One run as its runner starts it: its room and ledger entry, and how its pieces are named.
pub struct Research<'s> {
  stand: &'s Stand,
  source: String,
  chat_id: i64,
  unit: String,
  run_id: Option<i64>,
  owner: Option<String>,
  stopping: Arc<AtomicBool>,
  context_tokens: Option<u64>,
  closed: bool,
}

impl<'s> Research<'s> {
  /// A run of `source` for the room under its unit, its entry opened in the ledger now.
  pub async fn open(
    stand: &'s Stand,
    source: &str,
    chat_id: i64,
    unit: &str,
    opening: &Opening,
  ) -> Result<Self, Refusal> {
    let asked = RunOpening {
      agent: source.to_owned(),
      chat_id: Some(chat_id),
      unit: unit.to_owned(),
      depth: opening.depth,
      cap_ms: opening.cap_ms,
      started_at: now_seconds(),
      started_by: opening.started_by.clone(),
      search_id: opening.search_id,
    };
    let run_id = stand.ground.agency.open_run(&asked).await.map_err(agency_failed)?;

    Ok(Self::new(stand, source, chat_id, Some(run_id), unit))
  }

  /// A run of `source` for the room under the ledger entry and unit its launcher opened, where it opened one.
  pub fn new(stand: &'s Stand, source: &str, chat_id: i64, run_id: Option<i64>, unit: &str) -> Self {
    Self {
      stand,
      source: source.to_owned(),
      chat_id,
      unit: unit.to_owned(),
      run_id,
      owner: run_id.map(|id| research_owner(source, id)),
      stopping: Arc::new(AtomicBool::new(false)),
      context_tokens: None,
      closed: false,
    }
  }

  /// The run's entry in the ledger, where it has one.
  pub fn run_id(&self) -> Option<i64> {
    self.run_id
  }

  /// The name the run's places are held under in the pool; none for a run with no entry, which opens no
  /// branch.
  pub fn owner(&self) -> Option<&str> {
    self.owner.as_deref()
  }

  /// The run asked to stop: a model it carries is stopped, and one not launched yet never is.
  pub fn stop(&self) {
    self.stopping.store(true, Ordering::SeqCst);
  }

  /// Whether the run was asked to stop.
  pub fn stopping(&self) -> bool {
    self.stopping.load(Ordering::SeqCst)
  }

  /// The flag a stop sets, for a wait that is called off with it.
  pub fn stop_flag(&self) -> Arc<AtomicBool> {
    Arc::clone(&self.stopping)
  }

  /// The run stopped by `flag` instead of a flag of its own, so a stop heard before it was made reaches it.
  pub fn stopped_by(mut self, flag: Arc<AtomicBool>) -> Self {
    self.stopping = flag;
    self
  }

  /// The run's model carried on its route in `cwd`, `prompt` on its input, its web server told `told`: its
  /// answer with the context it ended carrying kept; what it had written where it was stopped; or why it gave
  /// none.
  pub async fn ask(
    &mut self,
    told: &Told,
    mapping: &Mapping,
    cwd: &str,
    prompt: &str,
    closes_at: Option<i64>,
  ) -> Result<Carried, Refusal> {
    let carry = Carry {
      agent: &self.source,
      role: None,
      mapping,
      cwd,
      name: &self.unit,
      title: format!("{} run", self.source),
      chat: self.chat_id,
      unit: (!self.unit.is_empty()).then_some(self.unit.as_str()),
      told,
      poll: TURN_POLL,
    };
    let carried = carry_turn(self.stand, &carry, prompt, &self.stopping, closes_at).await?;

    if let Carried::Answered(reply) = &carried {
      self.context_tokens = reply.context_tokens.and_then(|tokens| u64::try_from(tokens).ok());
    }

    Ok(carried)
  }

  /// Every place the run holds in the pool given back, its ledger entry left as it stands.
  pub async fn give_back(&self, outcome: &str) {
    if let Some(owner) = &self.owner
      && let Err(why) = finish_leases(self.stand, owner, &format!("{} run {outcome}", self.source)).await
    {
      eprintln!("{} run: its pool places could not be given back: {why}", self.source);
    }
  }

  /// The run ended with how it ended: every place it holds in the pool given back, and its ledger entry
  /// closed with the context it ended carrying. Whether this was the ending that closed the entry: the first
  /// reached is the real one.
  pub async fn close(&mut self, outcome: &str) -> bool {
    self.give_back(outcome).await;

    let Some(run_id) = self.run_id else { return false };

    if self.closed {
      return false;
    }

    self.closed = true;

    let closing = RunClosing {
      finished_at: now_seconds(),
      outcome: outcome.to_owned(),
      context_tokens: self.context_tokens,
    };

    if let Err(why) = self.stand.ground.agency.close_run(run_id, &closing).await {
      eprintln!("{} run: its ledger entry could not be closed: {why}", self.source);
    }

    true
  }
}

/// The usage `research run` answers a wrong line with.
pub const USAGE: &str = "usage: research run --key <key> --chat <chat> --unit <unit> --profile <profile> [--tier <tier>] [--demand explicit|optional] [--run <run>] [--depth <n>] [--started-by <who>] [--search-id <id>] [--leave-open]  (the prompt on stdin)";

/// One run as a research tool hands it over (docs/research-runs.md, "The verbs a research tool calls").
pub struct Started {
  pub key: String,
  pub chat_id: i64,
  pub unit: String,
  pub profile: Profile,
  /// The tier its launcher took; none takes the key's own.
  pub tier: Option<String>,
  pub demand: Demand,
  /// The entry its launcher opened in the ledger; none opens one.
  pub run_id: Option<i64>,
  pub opening: Opening,
  /// Whether an answer leaves the entry open for the tool to close.
  pub leave_open: bool,
  /// The directory its turn runs in.
  pub cwd: String,
}

/// How one carried run came out, as the verb prints it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ending {
  pub run: Option<i64>,
  pub outcome: String,
  pub context_tokens: Option<u64>,
  pub text: String,
}

/// The source a research key's runs are: `search` for a search depth, `browse` for the browser.
pub fn source_of(key: &str) -> Option<&'static str> {
  match key.split_once(':').map_or(key, |(source, _)| source) {
    "search" => Some("search"),
    "browse" => Some("browse"),
    _ => None,
  }
}

/// One run carried whole: what ended runs left behind reclaimed, its entry opened where its launcher opened
/// none, its branches' seats reserved — an explicit run waiting for its first — its model carried on `prompt`
/// closed with the moment it was asked and the knowledge its words call for, and its entry closed once. A run
/// stopped through `stopping` ends with what it had written.
pub async fn research_run(
  stand: &Stand,
  started: &Started,
  prompt: &str,
  stopping: Arc<AtomicBool>,
) -> Result<Ending, VerbError> {
  let Some(source) = source_of(&started.key).filter(|_| stand.policy.research.holds(&started.key)) else {
    return Err(VerbError::Refused(format!("{} is no research key", started.key)));
  };
  let systemctl = stand.ground.systemctl.clone();

  // Places a research run left behind when its process died are given back before this run asks.
  if let Err(why) = reclaim_leases(stand, |unit| unit_running(&systemctl, unit)).await {
    eprintln!("the pool's abandoned places could not be reclaimed: {why}");
  }

  let research = match started.run_id {
    Some(id) => Research::new(stand, source, started.chat_id, Some(id), &started.unit),
    None => Research::open(stand, source, started.chat_id, &started.unit, &started.opening).await?,
  };
  let mut research = research.stopped_by(stopping);
  let profile = &started.profile;

  // A run that may open branches reserves their seats before the model is launched; an explicit one waits for
  // its first seat before the model starts at all.
  if let Some(owner) = research.owner().map(str::to_owned)
    && profile.max_children() > 0
  {
    let branches = Branches {
      owner: &owner,
      source,
      role: profile.child_role(),
      count: u32::try_from(profile.max_children()).unwrap_or_default(),
    };

    if started.demand == Demand::Explicit {
      let flag = research.stop_flag();
      let place =
        await_place(stand, &branches, profile.wait_limit_ms(), PLACE_POLL, &|| flag.load(Ordering::SeqCst))
          .await;

      if research.stopping() {
        research.close(STOPPED).await;
        return Ok(ending(&research, STOPPED, String::new()));
      }

      if place.reservation.is_none() {
        let why = format!(
          "the explicit {} run got no place in the common co-worker pool: {}",
          started.key,
          place.reason.unwrap_or_default()
        );

        return Err(failed(&mut research, FAILED, why).await);
      }

      eprintln!(
        "{source} run: a pool seat was held after {}s, before the model was launched",
        (place.waited_ms as f64 / 1000.0).round()
      );
    } else if let Err(why) = reserve_places(stand, &owner, source, branches.role, branches.count).await {
      eprintln!("{source} run: its branches' seats were not reserved: {why}");
    }
  }

  let mapping = match run_mapping(stand, &started.key, started.tier.as_deref()).await {
    Ok(mapping) => mapping,
    Err(why) => return Err(failed(&mut research, FAILED, why.error).await),
  };
  let standing = Told {
    branch_tool: research.owner().is_some() && profile.max_children() > 0,
    image_tool: true,
    rulebooks: if source == "search" {
      stand.policy.research.search.on_request.clone()
    } else {
      vec![]
    },
    ..Told::default()
  };
  let now = now_ms();
  let told = run_told(standing, profile, research.owner().map(str::to_owned), source, started.chat_id, now);
  let prompt = asked_prompt(stand, source, started.chat_id, prompt, now).await;

  match research.ask(&told, &mapping, &started.cwd, &prompt, None).await {
    Ok(Carried::Answered(reply)) if !reply.live_tasks.is_empty() => {
      let why = format!(
        "the run ended with its own work still running: {}",
        live_task_names(&reply.live_tasks, 4)
      );

      Err(failed(&mut research, LIVE_TASKS, why).await)
    }
    Ok(Carried::Answered(reply)) if reply.text.trim().is_empty() => {
      Err(failed(&mut research, FAILED, "the run ended without a result".to_owned()).await)
    }
    Ok(Carried::Answered(reply)) => {
      let outcome = if started.leave_open {
        research.give_back(DONE).await;
        OPEN
      } else {
        research.close(DONE).await;
        DONE
      };

      Ok(ending(&research, outcome, reply.text.trim().to_owned()))
    }
    Ok(Carried::Stopped(written)) => {
      research.close(STOPPED).await;
      Ok(ending(&research, STOPPED, written.trim().to_owned()))
    }
    Ok(Carried::Failed(_)) if research.stopping() => {
      research.close(STOPPED).await;
      Ok(ending(&research, STOPPED, String::new()))
    }
    Ok(Carried::Failed(why)) => Err(failed(&mut research, FAILED, why).await),
    Err(why) => Err(failed(&mut research, FAILED, why.error).await),
  }
}

/// A run's entry its tool left open, closed with how the tool read it ended: every place the run holds given
/// back, then the entry closed; an entry already closed keeps its closing and is refused.
pub async fn close_run(
  stand: &Stand,
  run_id: i64,
  outcome: &str,
  context_tokens: Option<u64>,
) -> Result<Answer, VerbError> {
  let agency = &stand.ground.agency;
  let Some(run) = agency.run(run_id).await.map_err(agency_failed)? else {
    return Err(VerbError::Refused(format!("the ledger holds no run {run_id}")));
  };

  if let Some(ended) = run.outcome.filter(|_| run.finished_at.is_some()) {
    return Err(VerbError::Refused(format!("run {run_id} is already closed: {ended}")));
  }

  let source = source_of(&run.agent).unwrap_or(LINE);

  if let Err(why) =
    finish_leases(stand, &research_owner(source, run_id), &format!("{source} run {outcome}")).await
  {
    eprintln!("{source} run: its pool places could not be given back: {why}");
  }

  let closing = RunClosing {
    finished_at: now_seconds(),
    outcome: outcome.to_owned(),
    context_tokens,
  };

  agency.close_run(run_id, &closing).await.map_err(agency_failed)?;
  Ok(vec![format!("run {run_id} closed: {outcome}")].into())
}

// A run that failed, closed with how: the sentence it ends with.
async fn failed(research: &mut Research<'_>, outcome: &str, why: String) -> VerbError {
  research.close(outcome).await;
  VerbError::Ended {
    code: 1,
    said: format!("{} run failed: {why}", research.source),
  }
}

// How a carried run's entry is closed, and what the verb says of an entry it left open.
const DONE: &str = "done";
const STOPPED: &str = "stopped";
const FAILED: &str = "failed";
const LIVE_TASKS: &str = "live-tasks";
const OPEN: &str = "open";

fn ending(research: &Research<'_>, outcome: &str, text: String) -> Ending {
  Ending {
    run: research.run_id(),
    outcome: outcome.to_owned(),
    context_tokens: research.context_tokens,
    text,
  }
}

// The prompt a tool handed, closed with the moment it was asked and the knowledge its words call for; a
// failure of the gate hands over no knowledge.
async fn asked_prompt(stand: &Stand, source: &str, chat_id: i64, prompt: &str, now: i64) -> String {
  let asked = KnowledgeAsked {
    agent: stand.ground.agent_key.clone(),
    chat: chat_id,
    lines: vec![prompt.trim().to_owned()],
  };
  let pieces = stand.ground.gate.knowledge(&asked).await.unwrap_or_else(|why| {
    eprintln!("{source} run: no knowledge was picked: {why}");
    vec![]
  });
  let asked_at = format!("This was asked at {}.", asked_at_text(now));

  [
    Some(prompt.trim_end().to_owned()),
    Some(asked_at),
    knowledge_block(source, &stand.names.agent, &pieces),
  ]
  .into_iter()
  .flatten()
  .collect::<Vec<_>>()
  .join("\n\n")
}
