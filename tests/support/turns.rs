//! A job's calls beyond the queue's, stood in (docs/api.md, "Calls on other units"): bmo-agency's work turns
//! played from a script the test wrote — each turn's states in order, and what the worker does in its checkout
//! once the turn is sent its prompt — and every other call of a run answered as the test set and recorded.

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use futures_util::future::BoxFuture;
use job_agency::{
  AsideOutcome, ConfirmAnswer, Handover, JobDocuments, LiveTask, LogNote, ModelSpend, Prepared, Question,
  Reply, RunWorkflow, SshReach, TurnAsking, TurnFailure, TurnSpan, TurnState, WentOut, WorkTurn,
};
use job_base::Refusal;
use job_gate::{KnowledgeAsked, Piece, Sanitise, Sanitised};
use job_graph::Named;
use job_testbed::{Relinked, UnitPlan};
use serde_json::Value;

use bmo_job::job_run::calls::Calls;

/// The Claude model every played answer names.
pub const OPUS: &str = "claude-opus-5";

/// The model a job's turn on the Claude road goes out on, as the queue's stand-in resolves the job's route at
/// the job's tier.
pub const MAPPED: &str = "claude-code-frontier";

/// The road a worker's own turn goes out on.
pub const CLAUDE: &str = "claude-code";

/// The trailer a job's own text is followed by, as the worker's job documents give it.
pub const TRAILER: &str = "How a job ends is the `closing-a-job` skill.";

/// What a work step's trailer carries of the graph.
pub const GRAPH_TOLD: &str = "The code dependency graph is read with the graph tools.";

/// A report that did the work and says so in the shape a report is held to.
pub const FINISHED: &str = "Owner, the work is done.\n\nWhat changed\nThe file was fixed.\n\n1 done";

/// What the worker does once its turn is sent its prompt, handed that prompt.
pub type Act = Box<dyn FnOnce(String) -> BoxFuture<'static, ()> + Send>;

/// One turn as it is played: the states it is read in, the last standing once reached, and what the worker does
/// as it starts.
pub struct Played {
  pub states: Vec<TurnState>,
  pub act: Option<Act>,
  pub session: Option<String>,
  pub spans: Vec<TurnSpan>,
  pub went_out: Option<WentOut>,
}

impl Played {
  /// A turn that answered `reply` at once.
  pub fn answered(reply: Reply) -> Self {
    Self {
      session: reply.session_id.clone(),
      states: vec![TurnState::Answered {
        reply: Box::new(reply),
      }],
      act: None,
      spans: vec![],
      went_out: None,
    }
  }

  /// A turn that failed as bmo-agency read it.
  pub fn failed(failure: Option<TurnFailure>, evidence: Option<&str>) -> Self {
    Self {
      states: vec![TurnState::Failed {
        failure,
        evidence: evidence.map(str::to_owned),
      }],
      act: None,
      session: None,
      spans: vec![],
      went_out: None,
    }
  }

  /// The same turn, the worker doing `act` as it starts, after whatever it was doing already.
  pub fn doing(mut self, act: Act) -> Self {
    self.act = Some(match self.act.take() {
      None => act,
      Some(first) => Box::new(move |prompt: String| {
        Box::pin(async move {
          first(prompt.clone()).await;
          act(prompt).await;
        })
      }),
    });
    self
  }

  /// The same turn, the worker running `script` in `dir` as it starts.
  pub fn running(self, dir: &Path, script: &str) -> Self {
    self.doing(shell(dir, script))
  }

  /// The same turn, leaving these spans.
  pub fn spent(mut self, spans: Vec<TurnSpan>) -> Self {
    self.spans = spans;
    self
  }

  /// The same turn, read running first on each of `questions` in turn, none standing where it is none.
  pub fn asking(mut self, questions: Vec<Option<Question>>) -> Self {
    let running = questions.into_iter().map(|question| TurnState::Running { question });

    self.states = running.chain(self.states).collect();
    self
  }
}

/// An act that runs a shell script in a directory, as a worker's own commands do.
pub fn shell(dir: &Path, script: &str) -> Act {
  let (dir, script) = (dir.to_path_buf(), script.to_owned());

  Box::new(move |_| {
    Box::pin(async move {
      let ran = std::process::Command::new("sh")
        .arg("-ec")
        .arg(&script)
        .current_dir(&dir)
        .envs(GIT_ENV)
        .output()
        .expect("the worker's script runs");

      assert!(ran.status.success(), "{script}: {}", String::from_utf8_lossy(&ran.stderr));
    })
  })
}

/// What git is run with here: no configuration of the machine's, and a fixed author.
pub const GIT_ENV: [(&str, &str); 6] = [
  ("GIT_CONFIG_GLOBAL", "/dev/null"),
  ("GIT_CONFIG_SYSTEM", "/dev/null"),
  ("GIT_AUTHOR_NAME", "bmo"),
  ("GIT_AUTHOR_EMAIL", "bmo@test"),
  ("GIT_COMMITTER_NAME", "bmo"),
  ("GIT_COMMITTER_EMAIL", "bmo@test"),
];

/// A script that makes these changes and commits everything as the job's own.
pub fn commit_work(lines: &[&str]) -> String {
  format!("{}\ngit add -A\ngit commit -qm 'The job did its work'", lines.join("\n"))
}

/// A Claude answer on a session, spending ten tokens on the Claude model.
pub fn reply(session: &str, text: &str) -> Reply {
  Reply {
    mode: CLAUDE.into(),
    model: Some(OPUS.into()),
    effort: None,
    text: text.into(),
    session_id: Some(session.into()),
    account: None,
    context_ratio: None,
    context_tokens: None,
    session_usage: None,
    spend: vec![spend(OPUS, 10)],
    live_tasks: vec![],
    workflows: vec![],
    upstream: None,
    request_usages: None,
  }
}

/// What one model spent, all of it input.
pub fn spend(model: &str, tokens: i64) -> ModelSpend {
  ModelSpend {
    mode: Some(CLAUDE.into()),
    model: model.into(),
    tokens,
    input_tokens: tokens,
    output_tokens: 0,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    cost: None,
  }
}

/// The same answer with a background task of its own still running.
pub fn with_live_task(mut reply: Reply) -> Reply {
  reply.live_tasks = vec![LiveTask {
    id: "byu6vztqy".into(),
    description: "seed the repository".into(),
  }];
  reply
}

/// The same answer having launched a Claude Workflow that ended as `status` says, or still runs.
pub fn with_workflow(mut reply: Reply, status: Option<&str>) -> Reply {
  reply.workflows = vec![RunWorkflow {
    call: "toolu_1".into(),
    name: "two-answers".into(),
    status: status.map(str::to_owned),
  }];
  reply
}

/// A span of time spent in one tool, called so many times.
pub fn tool_span(name: &str, times: i64) -> TurnSpan {
  TurnSpan {
    kind: "tool".into(),
    name: name.into(),
    seconds: 1.0,
    times,
    tokens: None,
    input_tokens: None,
    output_tokens: None,
    cache_read_tokens: None,
    cache_write_tokens: None,
  }
}

fn locked<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
  mutex.lock().expect("a test's record is not poisoned")
}

fn ready<'a, T: Send + 'a>(value: T) -> BoxFuture<'a, Result<T, Refusal>> {
  Box::pin(async move { Ok(value) })
}

/// The run's calls, answering what the test set and keeping what each was asked.
/// What a run asked of bmo-agency's record of the worker's session: whether it still continues `session` on
/// `model`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeptAsked {
  pub model: String,
  pub session: String,
}

pub struct Agent {
  plays: Mutex<VecDeque<Played>>,
  // The states and session of the last turn played, which every turn past the script plays again.
  last: Mutex<Option<(Vec<TurnState>, Option<String>)>>,
  // Each prepared turn as it is still to be played.
  turns: Mutex<HashMap<String, Played>>,
  /// Every turn prepared, as it was asked.
  pub asked: Mutex<Vec<TurnAsking>>,
  /// Every prompt sent, in order.
  pub prompts: Mutex<Vec<String>>,
  /// Every asking whether a session is still kept, in order.
  pub kept_asked: Mutex<Vec<KeptAsked>>,
  /// Every session handed over.
  pub handed: Mutex<Vec<Handover>>,
  /// Every line written into the run's log.
  pub notes: Mutex<Vec<String>>,
  /// Every word pushed into a running turn.
  pub asides: Mutex<Vec<String>>,
  /// Every answer given a question a turn stood on, by the question's id.
  pub answers: Mutex<Vec<(String, ConfirmAnswer)>>,
  /// Every login rested, until when.
  pub rests: Mutex<Vec<(String, i64)>>,
  /// The upkeep calls in order, by name.
  pub upkeep: Mutex<Vec<String>>,
  /// Every text handed the sanitiser.
  pub sanitised: Mutex<Vec<String>>,
  /// Whether bmo-agency's record still continues the session asked of; false where it dropped it.
  pub kept: Mutex<bool>,
  /// What a worker's agent definition gives a job.
  pub documents: Mutex<JobDocuments>,
  /// Which files touch the prefix the worker's session was opened on.
  pub prefix: Mutex<Vec<String>>,
  /// The files each asking of what touches the prefix named.
  pub prefix_asked: Mutex<Vec<Vec<String>>>,
  /// What the checkout's graph names.
  pub named: Mutex<Named>,
  /// Whether a registered model's engine answers its health.
  pub healthy: Mutex<bool>,
  /// What bmo-agency refuses a turn's preparation with, where it refuses it.
  pub refusal: Mutex<Option<Refusal>>,
}

impl Default for Agent {
  fn default() -> Self {
    Self {
      plays: Mutex::default(),
      last: Mutex::default(),
      turns: Mutex::default(),
      asked: Mutex::default(),
      prompts: Mutex::default(),
      kept_asked: Mutex::default(),
      handed: Mutex::default(),
      notes: Mutex::default(),
      asides: Mutex::default(),
      answers: Mutex::default(),
      rests: Mutex::default(),
      upkeep: Mutex::default(),
      sanitised: Mutex::default(),
      kept: Mutex::new(true),
      documents: Mutex::new(JobDocuments {
        graph: GRAPH_TOLD.into(),
        trailer: TRAILER.into(),
        handoff: None,
      }),
      prefix: Mutex::default(),
      prefix_asked: Mutex::default(),
      named: Mutex::new(Named {
        directory: "graph".into(),
        files: vec!["kept.ts".into()],
      }),
      healthy: Mutex::new(true),
      refusal: Mutex::default(),
    }
  }
}

impl Agent {
  /// The turns to play, in the order the run asks for them.
  pub fn play(&self, plays: impl IntoIterator<Item = Played>) {
    locked(&self.plays).extend(plays);
  }

  pub fn asked(&self) -> Vec<TurnAsking> {
    locked(&self.asked).clone()
  }

  pub fn prompts(&self) -> Vec<String> {
    locked(&self.prompts).clone()
  }

  pub fn kept_asked(&self) -> Vec<KeptAsked> {
    locked(&self.kept_asked).clone()
  }

  pub fn handed(&self) -> Vec<Handover> {
    locked(&self.handed).clone()
  }

  pub fn notes(&self) -> Vec<String> {
    locked(&self.notes).clone()
  }

  pub fn asides(&self) -> Vec<String> {
    locked(&self.asides).clone()
  }

  pub fn answers(&self) -> Vec<(String, ConfirmAnswer)> {
    locked(&self.answers).clone()
  }

  pub fn rests(&self) -> Vec<(String, i64)> {
    locked(&self.rests).clone()
  }

  pub fn prefix_asked(&self) -> Vec<Vec<String>> {
    locked(&self.prefix_asked).clone()
  }

  pub fn sanitised(&self) -> Vec<String> {
    locked(&self.sanitised).clone()
  }

  pub fn upkeep(&self) -> Vec<String> {
    locked(&self.upkeep).clone()
  }

  pub fn set<T>(field: &Mutex<T>, value: T) {
    *locked(field) = value;
  }

  fn did(&self, call: &str) {
    locked(&self.upkeep).push(call.to_owned());
  }
}

impl Calls for Agent {
  fn prepare_turn<'a>(&'a self, asking: &'a TurnAsking) -> BoxFuture<'a, Result<Prepared, Refusal>> {
    self.did("prepare_turn");

    let mut asked = locked(&self.asked);

    asked.push(asking.clone());

    if let Some(refusal) = locked(&self.refusal).clone() {
      return Box::pin(async move { Err(refusal) });
    }

    let turn = format!("turn-{}", asked.len());
    // A turn past the script answers as the last one did, its worker doing nothing more.
    let played = locked(&self.plays).pop_front().unwrap_or_else(|| {
      let (states, session) = locked(&self.last)
        .clone()
        .unwrap_or_else(|| panic!("no turn is played for the run's turn {}: {asking:?}", asked.len()));

      Played {
        states,
        act: None,
        session,
        spans: vec![],
        went_out: None,
      }
    });

    *locked(&self.last) = Some((played.states.clone(), played.session.clone()));
    // The session the turn goes on: the one it resumes, or the one its answer names.
    let session = match &asking.resume {
      job_agency::Resume::Session { id } => Some(id.clone()),
      _ => played.session.clone(),
    };

    locked(&self.turns).insert(turn.clone(), played);

    ready(Prepared { turn, session })
  }

  fn send_turn_prompt<'a>(&'a self, turn: &'a str, prompt: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.prompts).push(prompt.to_owned());

    let act = locked(&self.turns).get_mut(turn).and_then(|played| played.act.take());
    let prompt = prompt.to_owned();

    Box::pin(async move {
      if let Some(act) = act {
        act(prompt).await;
      }

      Ok(())
    })
  }

  fn work_turn<'a>(&'a self, turn: &'a str) -> BoxFuture<'a, Result<Option<WorkTurn>, Refusal>> {
    let mut turns = locked(&self.turns);
    let Some(played) = turns.get_mut(turn) else {
      return ready(None);
    };
    // The last state stands once it is reached.
    let state = if played.states.len() > 1 {
      played.states.remove(0)
    } else {
      played.states.first().cloned().expect("a turn has a state")
    };
    let ends = !matches!(state, TurnState::Running { .. });

    ready(Some(WorkTurn {
      turn: turn.to_owned(),
      state,
      spans: if ends { std::mem::take(&mut played.spans) } else { vec![] },
      compacted: false,
      session: played.session.clone(),
      went_out: played.went_out.clone(),
    }))
  }

  fn push_aside<'a>(&'a self, _: &'a str, text: &'a str) -> BoxFuture<'a, Result<AsideOutcome, Refusal>> {
    locked(&self.asides).push(text.to_owned());

    ready(AsideOutcome::Delivered)
  }

  fn answer_question<'a>(
    &'a self,
    _: &'a str,
    question: &'a str,
    answer: &'a ConfirmAnswer,
  ) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.answers).push((question.to_owned(), answer.clone()));

    ready(())
  }

  fn session_kept<'a>(
    &'a self,
    _: &'a str,
    model: &'a str,
    session: &'a str,
  ) -> BoxFuture<'a, Result<bool, Refusal>> {
    self.did("session_kept");
    locked(&self.kept_asked).push(KeptAsked {
      model: model.to_owned(),
      session: session.to_owned(),
    });

    ready(*locked(&self.kept))
  }

  fn hand_over_session<'a>(
    &'a self,
    _: &'a str,
    handover: &'a Handover,
  ) -> BoxFuture<'a, Result<(), Refusal>> {
    self.did("hand_over_session");
    locked(&self.handed).push(handover.clone());

    ready(())
  }

  fn note_run<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    self.did("note_run");

    ready(())
  }

  fn wake_keepalive<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    self.did("wake_keepalive");

    ready(())
  }

  fn make_bench<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<(), Refusal>> {
    self.did("make_bench");

    ready(())
  }

  fn sweep_servers<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Vec<String>, Refusal>> {
    self.did("sweep_servers");

    ready(vec![])
  }

  fn job_documents<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<JobDocuments, Refusal>> {
    ready(locked(&self.documents).clone())
  }

  fn prefix_touched<'a>(
    &'a self,
    _: &'a str,
    files: &'a [String],
  ) -> BoxFuture<'a, Result<Vec<String>, Refusal>> {
    let prefix = locked(&self.prefix).clone();

    locked(&self.prefix_asked).push(files.to_vec());
    ready(files.iter().filter(|file| prefix.contains(file)).cloned().collect())
  }

  fn model_health<'a>(&'a self, _: &'a str, _: &'a str) -> BoxFuture<'a, Result<bool, Refusal>> {
    ready(*locked(&self.healthy))
  }

  fn rest_login<'a>(&'a self, account: &'a str, until: i64) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.rests).push((account.to_owned(), until));

    ready(())
  }

  fn usage_rest<'a>(&'a self, _: &'a str, _: &'a Value) -> BoxFuture<'a, Result<(), Refusal>> {
    ready(())
  }

  fn write_note<'a>(&'a self, note: &'a LogNote) -> BoxFuture<'a, Result<(), Refusal>> {
    locked(&self.notes).push(note.text.clone());

    ready(())
  }

  fn node_copy<'a>(&'a self, _: &'a str, checkout: &'a str) -> BoxFuture<'a, Result<String, Refusal>> {
    ready(checkout.to_owned())
  }

  fn ssh_reach<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Option<SshReach>, Refusal>> {
    ready(None)
  }

  fn sanitise<'a>(&'a self, asked: &'a Sanitise) -> BoxFuture<'a, Result<Sanitised, Refusal>> {
    locked(&self.sanitised).push(asked.text.clone());

    ready(Sanitised {
      text: asked.text.clone(),
      changed: false,
    })
  }

  fn kept<'a>(
    &'a self,
    _: &'a str,
    _: &'a [String],
  ) -> BoxFuture<'a, Result<HashMap<String, String>, Refusal>> {
    ready(HashMap::new())
  }

  fn knowledge<'a>(&'a self, _: &'a KnowledgeAsked) -> BoxFuture<'a, Result<Vec<Piece>, Refusal>> {
    ready(vec![])
  }

  fn graph_named<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Named, Refusal>> {
    ready(locked(&self.named).clone())
  }

  fn plan_units<'a>(
    &'a self,
    _: &'a Path,
    _: &'a str,
    _: bool,
  ) -> BoxFuture<'a, Result<UnitPlan, Refusal>> {
    ready(UnitPlan {
      relinks: vec![],
      running: vec![],
      unmatched: vec![],
    })
  }

  fn relink_units<'a>(&'a self, _: &'a Path) -> BoxFuture<'a, Result<Relinked, Refusal>> {
    self.did("relink_units");

    ready(Relinked {
      relinked: vec![],
      dangling: vec![],
      unmatched: vec![],
    })
  }

  fn queue_moved(&self) -> BoxFuture<'_, ()> {
    self.did("queue_moved");

    Box::pin(async {})
  }
}

/// A plan that holds on the bench's roster: a scout finding things out, then the builder.
pub const PLAN: &str = r#"{"stages":[{"title":"Investigate","kind":"investigate","tasks":[{"title":"Read the panel","role":"scout","model":null,"instruction":"Read how the panel is drawn."}]},{"title":"Build","kind":"investigate","tasks":[{"title":"Raise the panel","role":"builder","model":null,"instruction":"Raise the panel and keep its touch input."}]}]}"#;

/// What a control turn means to do, which is never parsed: the shape is asked for after it.
pub const DESIGN: &str =
  "Two scouts in an investigation stage, then this session reads what they found and one builder fixes it.";

/// An act that marks the item done, as `bmo todo --done` would at the end of the worker's turn.
pub fn closing(db: &sea_orm::DatabaseConnection, todo: i64) -> Act {
  let db = db.clone();

  Box::new(move |_| {
    Box::pin(async move {
      job_base::run(
        &db,
        r#"UPDATE "todos" SET "done" = 1, "done_at" = $2, "report" = 'Done.' WHERE "id" = $1"#,
        vec![todo.into(), bmo_job::now_ms().into()],
      )
      .await
      .expect("the item is marked done");
    })
  })
}
