//! A work turn carried on bmo-agency's session contract (docs/api.md, "Work turns"): prepared on the worker's
//! session, sent in the background with its job and item, its end heard on the event stream, and read as it
//! stands under an id of the service's own.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use futures_util::StreamExt;

use crate::{
  Agency, AgencyError, Answer, AsideOutcome, ConfirmAnswer, Delivery, Prepared, Reply, Scope, SessionId,
  SessionState, Start, Turn, TurnAsking, TurnContext, TurnEnded, TurnEvent, TurnEvents, TurnFailure,
  TurnOutcome, TurnSeat, TurnState, WentOut, WorkTurn,
};

/// How long the event stream is given to tell a turn's end once its send has been answered; bmo-agency
/// publishes the end before it answers, so only the stream's own lag is waited for.
const END_WAIT: Duration = Duration::from_secs(30);

/// The work turns being carried, each under its own id, and the session contract they are carried on.
pub struct WorkTurns<A> {
  agency: A,
  turns: Arc<Mutex<HashMap<String, Carrying>>>,
  next: AtomicU64,
}

/// One work turn from its preparation until it is read in its ending.
struct Carrying {
  session: SessionId,
  context: TurnContext,
  /// The provider and model the asking named, which a reply names where the session's state reads none.
  provider: String,
  model: String,
  sent: bool,
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

impl<A: Agency + Clone + 'static> WorkTurns<A> {
  /// Work turns carried on `agency`.
  pub fn new(agency: A) -> Self {
    Self {
      agency,
      turns: Arc::default(),
      next: AtomicU64::new(1),
    }
  }

  /// Prepares a work turn: a worker's starts the worker's session. The session contract names no session a
  /// turn continues, so none is answered.
  pub async fn prepare(&self, asking: &TurnAsking) -> Answer<Prepared> {
    if asking.seat != TurnSeat::Worker {
      return Err(uncarried(
        "prepare_turn",
        "bmo-agency's session contract keeps no session thrown away for a branch's turn",
      ));
    }

    let start = Start {
      agent: asking.agent.clone(),
      scope: Scope::Worker,
    };
    let session = self.agency.start_session(&start).await?;
    let turn = format!("{}#{}", session.id, self.next.fetch_add(1, Ordering::Relaxed));

    locked(&self.turns).insert(
      turn.clone(),
      Carrying {
        session: session.id,
        context: TurnContext {
          job: asking.job,
          item: asking.item,
        },
        provider: asking.provider.clone(),
        model: asking.mapping.model.clone(),
        sent: false,
        ended: None,
      },
    );

    Ok(Prepared {
      turn,
      session: None,
    })
  }

  /// Sends a prepared work turn its prompt: the event stream opened first, then the turn sent and carried to its
  /// end in the background.
  pub async fn send(&self, turn: &str, prompt: &str) -> Answer<()> {
    const CALL: &str = "send_turn_prompt";

    let (session, context, provider, model) = match locked(&self.turns).get(turn) {
      None => return Err(uncarried(CALL, "there is no such work turn")),
      Some(carrying) if carrying.sent => {
        return Err(uncarried(CALL, "the work turn is not waiting for its prompt"));
      }
      Some(carrying) => (
        carrying.session.clone(),
        carrying.context.clone(),
        carrying.provider.clone(),
        carrying.model.clone(),
      ),
    };
    let events = self.agency.turn_events().await?;

    if let Some(carrying) = locked(&self.turns).get_mut(turn) {
      carrying.sent = true;
    }

    let agency = self.agency.clone();
    let turns = Arc::clone(&self.turns);
    let id = turn.to_owned();
    let sent = Turn {
      session,
      prompt: prompt.to_owned(),
      context,
    };

    tokio::spawn(async move {
      let ended = carried(&agency, &sent, events).await;
      let state = agency.session_state(&sent.session).await.ok().flatten();
      let read = ended_turn(&id, &sent.session, ended, state.as_ref(), &provider, &model);

      if let Some(carrying) = locked(&turns).get_mut(&id) {
        carrying.ended = Some(read);
      }
    });

    Ok(())
  }

  /// Reads a work turn as it stands: running until it has ended, then its ending, after which it is let go.
  pub fn read(&self, turn: &str) -> Option<WorkTurn> {
    let mut turns = locked(&self.turns);
    let carrying = turns.get(turn)?;

    if carrying.ended.is_none() {
      return Some(WorkTurn {
        turn: turn.to_owned(),
        state: TurnState::Running { question: None },
        spans: vec![],
        compacted: false,
        session: Some(carrying.session.0.clone()),
        went_out: None,
      });
    }

    turns.remove(turn).and_then(|carrying| carrying.ended)
  }

  /// Pushes words into a work turn's session.
  pub async fn push(&self, turn: &str, words: &str) -> Answer<AsideOutcome> {
    let session = locked(&self.turns).get(turn).map(|carrying| carrying.session.clone());
    let Some(session) = session else {
      return Err(uncarried("push_aside", "there is no such work turn"));
    };

    self.agency.push_words(&session, words).await
  }

  /// Answers the question a work turn stands on, which the session contract neither shows nor takes.
  pub fn answer(&self, _: &str, _: &str, _: &ConfirmAnswer) -> Answer<()> {
    Err(uncarried(
      "answer_question",
      "bmo-agency's session contract takes no answer to a question a turn stands on",
    ))
  }
}

/// What the send of a turn was answered, and how its event stream told it ended.
struct Ended {
  delivery: Answer<Delivery>,
  heard: Result<TurnEnded, String>,
}

/// The turn sent, its events read meanwhile so the stream never stands unread: the first start on the session
/// names the turn's run, and the end of that run is how it ended.
async fn carried<A: Agency>(agency: &A, turn: &Turn, mut events: TurnEvents) -> Ended {
  let session = &turn.session;
  let heard = async {
    let mut run: Option<String> = None;

    while let Some(event) = events.next().await {
      match event {
        Ok(TurnEvent::Started(started)) if &started.session == session && run.is_none() => {
          run = Some(started.run);
        }
        Ok(TurnEvent::Ended(ended)) if &ended.session == session && run.as_ref() == Some(&ended.run) => {
          return Ok(ended);
        }
        Ok(_) => {}
        Err(why) => return Err(why.to_string()),
      }
    }

    Err("bmo-agency's event stream ended before it told how the turn ended".to_owned())
  };
  let send = agency.send_turn(turn);

  tokio::pin!(heard, send);

  let mut told = None;
  let delivery = loop {
    tokio::select! {
      delivery = &mut send => break delivery,
      ended = &mut heard, if told.is_none() => told = Some(ended),
    }
  };
  let heard = match told {
    Some(heard) => heard,
    None if delivery.is_err() => Err(String::new()),
    None => tokio::time::timeout(END_WAIT, heard)
      .await
      .unwrap_or_else(|_| Err("bmo-agency's event stream did not tell how the turn ended".to_owned())),
  };

  Ended { delivery, heard }
}

/// A turn read in its ending, from what its send was answered, how it ended and the session's state after it.
fn ended_turn(
  turn: &str,
  session: &SessionId,
  ended: Ended,
  state: Option<&SessionState>,
  provider: &str,
  model: &str,
) -> WorkTurn {
  let failed =
    |failure: Option<TurnFailure>, evidence: Option<String>| TurnState::Failed { failure, evidence };
  let state_read = match (ended.delivery, ended.heard) {
    (Err(refused), _) => failed(None, Some(refused.to_string())),
    (Ok(Delivery::Blocked), _) => failed(Some(TurnFailure::Person), None),
    (Ok(Delivery::Taken), Err(why)) => failed(None, Some(why)),
    (Ok(Delivery::Taken), Ok(heard)) => match heard.outcome {
      TurnOutcome::Answered => TurnState::Answered {
        reply: Box::new(reply(heard.answer.unwrap_or_default(), session, state, provider, model)),
      },
      TurnOutcome::Blocked => failed(Some(TurnFailure::Person), None),
      TurnOutcome::Stopped => failed(Some(TurnFailure::Stopped), None),
      TurnOutcome::Failed => failed(None, Some(evidence(&heard))),
    },
  };

  WorkTurn {
    turn: turn.to_owned(),
    state: state_read,
    spans: vec![],
    compacted: false,
    session: Some(session.0.clone()),
    went_out: state.map(|state| WentOut {
      provider: state.provider.clone(),
      model: state.model.clone(),
      effort: None,
      tier: None,
    }),
  }
}

/// What a failed turn left for the service to read: why it failed, and the engine's error kind on a line after.
fn evidence(heard: &TurnEnded) -> String {
  let reason = heard.reason.clone().unwrap_or_default();

  match heard.engine_error.as_ref().and_then(|engine| engine.kind.as_deref()) {
    Some(kind) => format!("{reason}\nengine error: {kind}"),
    None => reason,
  }
}

/// An answered turn's reply: its text, on the session it went on and the provider and model the session's state
/// reads, the asking's where it reads none, with the context the state reads.
fn reply(
  text: String,
  session: &SessionId,
  state: Option<&SessionState>,
  provider: &str,
  model: &str,
) -> Reply {
  let context = state.and_then(|state| state.context);

  Reply {
    mode: state.map_or(provider, |state| state.provider.as_str()).to_owned(),
    model: Some(state.map_or(model, |state| state.model.as_str()).to_owned()),
    effort: None,
    text,
    session_id: Some(session.0.clone()),
    account: None,
    context_ratio: context
      .filter(|context| context.window > 0)
      .map(|context| context.carried as f64 / context.window as f64),
    context_tokens: context.and_then(|context| i64::try_from(context.carried).ok()),
    session_usage: None,
    spend: vec![],
    live_tasks: vec![],
    workflows: vec![],
    upstream: None,
    request_usages: None,
  }
}
