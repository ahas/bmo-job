//! A work turn carried on bmo-agency's session contract (docs/api.md, "Work turns"): prepared on the worker's
//! session or on one thrown away, sent in the background with its parts, its job and its item, watched on the
//! session's state while it goes, read from the turn's end once the send is answered, and read as it stands
//! under an id of the service's own.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use tokio::time::MissedTickBehavior;

use crate::{
  Agency, AgencyError, Answer, AsideOutcome, ConfirmAnswer, Delivery, EngineError, Output, Prepared,
  Question, Reply, Resume, Scope, SessionId, Start, Turn, TurnAsking, TurnContext, TurnEnd, TurnFailure,
  TurnOutcome, TurnSeat, TurnSpan, TurnState, WentOut, WorkTurn,
};

/// How often the session's state is read while a turn goes, for the question it waits on and what it went out
/// on.
const WATCH_EVERY: Duration = Duration::from_secs(1);

/// The work turns being carried, each under its own id, and the session contract they are carried on.
pub struct WorkTurns<A> {
  agency: A,
  turns: Arc<Mutex<HashMap<String, Carrying>>>,
  next: AtomicU64,
}

type Carried = Arc<Mutex<HashMap<String, Carrying>>>;

/// One work turn from its preparation until it is read in its ending.
struct Carrying {
  /// bmo-agency's session the turn goes into.
  session: SessionId,
  asking: TurnAsking,
  /// The harness session the turn goes on, none for a fresh one.
  continues: Option<String>,
  /// Which session the next send of the prompt goes on.
  resume: Resume,
  sent: bool,
  /// What the session's state last showed while the turn went.
  question: Option<Question>,
  went_out: Option<WentOut>,
  ended: Option<WorkTurn>,
}

fn locked(turns: &Mutex<HashMap<String, Carrying>>) -> MutexGuard<'_, HashMap<String, Carrying>> {
  turns.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn uncarried(call: &'static str, why: &str) -> AgencyError {
  AgencyError::Uncarried {
    call,
    why: why.to_owned(),
  }
}

/// The session a send goes on once the turn is to continue `continues`: that one, or a fresh one where it is
/// none.
fn resume_on(continues: Option<&str>) -> Resume {
  match continues {
    Some(id) => Resume::Session { id: id.to_owned() },
    None => Resume::Fresh,
  }
}

impl<A: Agency + Clone + 'static> WorkTurns<A> {
  /// Work turns carried on `agency`.
  pub fn new(agency: A) -> Self {
    Self {
      agency,
      turns: Arc::default(),
      next: AtomicU64::new(1),
    }
  }

  /// Prepares a work turn: a worker's starts the worker's session and names the harness session the turn
  /// continues, which its prompt is composed for; a branch's starts a session thrown away, on which every turn
  /// is fresh.
  pub async fn prepare(&self, asking: &TurnAsking) -> Answer<Prepared> {
    let scope = match &asking.seat {
      TurnSeat::Worker => Scope::Worker,
      TurnSeat::Branch { todo, role } => Scope::Apart {
        provider: asking.provider.clone(),
        model: asking.model.clone(),
        cwd: asking.cwd.clone(),
        todo: *todo,
        role: role.clone(),
      },
    };
    let start = Start {
      agent: asking.agent.clone(),
      scope,
    };
    let session = self.agency.start_session(&start).await?;
    let (continues, resume) = match (&asking.seat, &asking.resume) {
      (TurnSeat::Branch { .. }, resume) => (None, resume.clone()),
      (TurnSeat::Worker, Resume::Fresh) => (None, Resume::Fresh),
      (TurnSeat::Worker, Resume::Session { id }) => (Some(id.clone()), resume_on(Some(id))),
      (TurnSeat::Worker, Resume::Auto) => {
        let state = self.agency.session_state(&session.id, Some(&asking.model)).await?;
        let continues = state.and_then(|state| state.continues);
        let resume = resume_on(continues.as_deref());

        (continues, resume)
      }
    };
    let turn = format!("{}#{}", session.id, self.next.fetch_add(1, Ordering::Relaxed));

    locked(&self.turns).insert(
      turn.clone(),
      Carrying {
        session: session.id,
        asking: asking.clone(),
        continues: continues.clone(),
        resume,
        sent: false,
        question: None,
        went_out: None,
        ended: None,
      },
    );

    Ok(Prepared {
      turn,
      session: continues,
    })
  }

  /// Sends a prepared work turn its prompt with every part of its asking, carried to its end in the background
  /// with the session's state watched meanwhile.
  pub async fn send(&self, turn: &str, prompt: &str) -> Answer<()> {
    const CALL: &str = "send_turn_prompt";

    let sent = {
      let mut turns = locked(&self.turns);
      let Some(carrying) = turns.get_mut(turn) else {
        return Err(uncarried(CALL, "there is no such work turn"));
      };

      if carrying.sent {
        return Err(uncarried(CALL, "the work turn is not waiting for its prompt"));
      }

      carrying.sent = true;
      carrying.ended = None;

      Turn {
        session: carrying.session.clone(),
        prompt: prompt.to_owned(),
        context: TurnContext {
          job: carrying.asking.job,
          item: carrying.asking.item,
          work: Some(carrying.asking.parts(carrying.resume.clone())),
        },
      }
    };
    let agency = self.agency.clone();
    let turns = Arc::clone(&self.turns);
    let id = turn.to_owned();

    tokio::spawn(async move {
      let end = watched(&agency, &turns, &id, &sent).await;

      if let Some(carrying) = locked(&turns).get_mut(&id) {
        carrying.end(&id, end);
      }
    });

    Ok(())
  }

  /// Reads a work turn as it stands: running until its send is answered, then its ending, after which it is let
  /// go; a turn that came back reprepared stays, waiting for its prompt again.
  pub fn read(&self, turn: &str) -> Option<WorkTurn> {
    let mut turns = locked(&self.turns);
    let carrying = turns.get_mut(turn)?;

    match carrying.ended.take() {
      None => Some(WorkTurn {
        turn: turn.to_owned(),
        state: TurnState::Running {
          question: carrying.question.clone(),
        },
        spans: vec![],
        compacted: false,
        session: carrying.continues.clone(),
        went_out: carrying.went_out.clone(),
      }),
      Some(read) if matches!(read.state, TurnState::Reprepared) => Some(read),
      Some(read) => {
        turns.remove(turn);
        Some(read)
      }
    }
  }

  /// Pushes words into a work turn's session.
  pub async fn push(&self, turn: &str, words: &str) -> Answer<AsideOutcome> {
    let session = self.session_of(turn, "push_aside")?;

    self.agency.push_words(&session, words).await
  }

  /// Answers the question a work turn stands on, by the question's id.
  pub async fn answer(&self, turn: &str, question: &str, answer: &ConfirmAnswer) -> Answer<()> {
    let session = self.session_of(turn, "answer_question")?;

    self.agency.answer_question(&session, question, answer).await
  }

  /// Interrupts a work turn.
  pub async fn interrupt(&self, turn: &str) -> Answer<()> {
    let session = self.session_of(turn, "interrupt_turn")?;

    self.agency.interrupt_session(&session).await
  }

  /// Reads what a work turn has written so far, while it is carried.
  pub async fn output(&self, turn: &str) -> Answer<Option<Output>> {
    let session = self.session_of(turn, "turn_output")?;

    self.agency.session_output(&session).await
  }

  fn session_of(&self, turn: &str, call: &'static str) -> Answer<SessionId> {
    locked(&self.turns)
      .get(turn)
      .map(|carrying| carrying.session.clone())
      .ok_or_else(|| uncarried(call, "there is no such work turn"))
  }
}

/// The turn sent, and the session's state read meanwhile for the question it waits on and what it went out on;
/// a read that fails leaves what was shown last.
async fn watched<A: Agency>(agency: &A, turns: &Carried, id: &str, turn: &Turn) -> Answer<TurnEnd> {
  let send = agency.send_turn(turn);
  let mut every = tokio::time::interval(WATCH_EVERY);

  every.set_missed_tick_behavior(MissedTickBehavior::Delay);
  tokio::pin!(send);

  loop {
    tokio::select! {
      end = &mut send => return end,
      _ = every.tick() => {
        if let Ok(Some(state)) = agency.session_state(&turn.session, None).await
          && let Some(carrying) = locked(turns).get_mut(id)
        {
          carrying.question = state.question;
          carrying.went_out = state.went_out.or_else(|| carrying.went_out.take());
        }
      }
    }
  }
}

impl Carrying {
  /// The turn's end, or the refusal of its send, kept for its reading.
  fn end(&mut self, id: &str, end: Answer<TurnEnd>) {
    let failed =
      |failure: Option<TurnFailure>, evidence: Option<String>| TurnState::Failed { failure, evidence };

    self.question = None;

    let end = match end {
      Ok(end) => end,
      Err(refused) => {
        self.ended = Some(self.reading(id, failed(None, Some(refused.to_string())), vec![], false));
        return;
      }
    };
    let state = match (end.delivery, end.outcome) {
      (Delivery::Blocked, _) | (_, TurnOutcome::Blocked) => {
        failed(Some(end.failure.unwrap_or(TurnFailure::Person)), None)
      }
      (_, TurnOutcome::Reprepared) => {
        self.continues = end.continues.clone();
        self.resume = resume_on(self.continues.as_deref());
        self.sent = false;
        TurnState::Reprepared
      }
      (_, TurnOutcome::Answered) => {
        let reply = end.reply.unwrap_or_else(|| Box::new(self.bare_reply(end.answer.unwrap_or_default())));

        self.continues = reply.session_id.clone().or_else(|| self.continues.take());
        TurnState::Answered { reply }
      }
      (_, TurnOutcome::Stopped) => failed(Some(end.failure.unwrap_or(TurnFailure::Stopped)), None),
      (_, TurnOutcome::Failed) => match end.failure {
        Some(failure) => failed(Some(failure), None),
        None => failed(None, Some(evidence(end.reason.as_deref(), end.engine_error.as_ref()))),
      },
    };

    if let Some(out) = end.went_out {
      self.went_out = Some(out);
    }

    self.ended = Some(self.reading(id, state, end.spans, end.compacted));
  }

  fn reading(&self, id: &str, state: TurnState, spans: Vec<TurnSpan>, compacted: bool) -> WorkTurn {
    WorkTurn {
      turn: id.to_owned(),
      state,
      spans,
      compacted,
      session: self.continues.clone(),
      went_out: self.went_out.clone(),
    }
  }

  /// The reply of an answered end that carries none of its own: its answer's text, on the asking's provider and
  /// model and the session the turn went on.
  fn bare_reply(&self, text: String) -> Reply {
    Reply {
      mode: self.asking.provider.clone(),
      model: Some(self.asking.mapping.model.clone()),
      effort: self.asking.mapping.effort.clone(),
      text,
      session_id: self.continues.clone(),
      account: None,
      context_ratio: None,
      context_tokens: None,
      session_usage: None,
      spend: vec![],
      live_tasks: vec![],
      workflows: vec![],
      upstream: None,
      request_usages: None,
    }
  }
}

/// What a failed turn left for the service to read: why it failed, and the engine's error kind on a line after.
fn evidence(reason: Option<&str>, engine: Option<&EngineError>) -> String {
  let reason = reason.unwrap_or_default();

  match engine.and_then(|engine| engine.kind.as_deref()) {
    Some(kind) => format!("{reason}\nengine error: {kind}"),
    None => reason.to_owned(),
  }
}
