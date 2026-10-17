//! One research run, from its opening in bmo-agency's run ledger to its closing there (docs/verbs.md,
//! "Research runs"): its model carried as a work turn on the search line's route, its web server told the
//! run's settings in the turn's variables, a stop asked from outside interrupting the turn's session and
//! keeping what it had written, and the run closed once with how it ended and the context it ended carrying,
//! every place it holds in the pool given back.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use job_agency::{
  Agency, ConfirmAnswer, Mapping, Reply, Resume, RouteAsk, RunClosing, RunOpening, SessionId, TurnAsking,
  TurnMapping, TurnSeat, TurnState,
};
use job_base::Refusal;
use serde_json::Value;

use super::{LINE, Profile, Told, clock, finish_leases, line_provider, owner as research_owner};
use crate::now_ms;
use crate::queue::units::agency_failed;
use crate::verbs::stand::Stand;

// How often a running turn is read again.
const POLL: Duration = Duration::from_secs(1);

// What a research run's turn answers a question its program stops on with: nobody is there to answer it.
const NO_QUESTION: &str = "A research run answers no question; carry on with what you have.";

/// Every text block a stream of the program's records says its assistant turns wrote, in order: what a run
/// stopped from outside had written.
pub fn assistant_texts(stream: &str) -> Vec<String> {
  stream
    .lines()
    .map(str::trim)
    .filter(|line| line.starts_with('{'))
    .filter_map(|line| serde_json::from_str::<Value>(line).ok())
    .filter(|event| event.get("type").and_then(Value::as_str) == Some("assistant"))
    .flat_map(|event| {
      event.pointer("/message/content").and_then(Value::as_array).cloned().unwrap_or_default()
    })
    .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
    .filter_map(|block| block.get("text").and_then(Value::as_str).map(str::to_owned))
    .collect()
}

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

/// The route a research key's run takes: its tier on the provider the search line starts on.
pub async fn run_mapping(stand: &Stand, key: &str) -> Result<Mapping, Refusal> {
  let Some(tier) = key_tier(stand, key) else {
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
}

/// The turn carried on bmo-agency to its answer, `prompt` on its input, read again until it answers or fails;
/// once `stopping` is set or `closes_at` passes, its session is interrupted and what it wrote is kept.
pub async fn carry_turn(
  stand: &Stand,
  carry: &Carry<'_>,
  prompt: &str,
  stopping: &AtomicBool,
  closes_at: Option<i64>,
) -> Result<Carried, Refusal> {
  let agency = &stand.ground.agency;
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

  let prepared = agency.prepare_turn(&asking).await.map_err(agency_failed)?;

  agency.send_turn_prompt(&prepared.turn, prompt).await.map_err(agency_failed)?;

  let mut interrupted: Option<String> = None;

  loop {
    tokio::time::sleep(POLL).await;

    let Some(turn) = agency.work_turn(&prepared.turn).await.map_err(agency_failed)? else {
      return Ok(Carried::Failed("the turn is gone".to_owned()));
    };
    let stop = stopping.load(Ordering::SeqCst) || closes_at.is_some_and(|at| now_ms() >= at);

    if stop
      && interrupted.is_none()
      && let Some(session) = turn.session.clone()
    {
      let id = SessionId(session);
      let written = agency
        .session_output(&id)
        .await
        .ok()
        .flatten()
        .map(|output| output.text)
        .unwrap_or_default();

      let _ = agency.interrupt_session(&id).await;
      interrupted = Some(written);
    }

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
      TurnState::Reprepared => {
        agency.send_turn_prompt(&prepared.turn, prompt).await.map_err(agency_failed)?
      }
      TurnState::Running {
        question: Some(question),
      } => {
        let answer = ConfirmAnswer::Refuse {
          words: Some(NO_QUESTION.to_owned()),
        };

        agency
          .answer_question(&prepared.turn, &question.id, &answer)
          .await
          .map_err(agency_failed)?;
      }
      TurnState::Running { question: None } => {}
    }
  }
}

/// What a run opens its ledger entry with beside its agent, room and unit.
#[derive(Clone, Debug, Default)]
pub struct Opening {
  pub depth: u32,
  pub cap_ms: Option<i64>,
  pub started_by: Option<String>,
  pub search_id: Option<String>,
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
      started_at: now_ms(),
      started_by: opening.started_by.clone(),
      search_id: opening.search_id.clone(),
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
    };
    let carried = carry_turn(self.stand, &carry, prompt, &self.stopping, closes_at).await?;

    if let Carried::Answered(reply) = &carried {
      self.context_tokens = reply.context_tokens.and_then(|tokens| u64::try_from(tokens).ok());
    }

    Ok(carried)
  }

  /// The run ended with how it ended: every place it holds in the pool given back, and its ledger entry
  /// closed with the context it ended carrying. Whether this was the ending that closed the entry: the first
  /// reached is the real one.
  pub async fn close(&mut self, outcome: &str) -> bool {
    if let Some(owner) = &self.owner
      && let Err(why) = finish_leases(self.stand, owner, &format!("{} run {outcome}", self.source)).await
    {
      eprintln!("{} run: its pool places could not be given back: {why}", self.source);
    }

    let Some(run_id) = self.run_id else { return false };

    if self.closed {
      return false;
    }

    self.closed = true;

    let closing = RunClosing {
      finished_at: now_ms(),
      outcome: outcome.to_owned(),
      context_tokens: self.context_tokens,
    };

    if let Err(why) = self.stand.ground.agency.close_run(run_id, &closing).await {
      eprintln!("{} run: its ledger entry could not be closed: {why}", self.source);
    }

    true
  }
}
