//! One turn of a job, carried by bmo-agency as a work turn (docs/api.md, "Work turns") and assembled here
//! (docs/runs.md, "The turns"): the owner's waiting reset asked for, the turn prepared on the session it will
//! continue, its prompt shaped for a format turn and the earlier results it carries opened against what that
//! session already holds, a session that last answered for another item told so, and the turn sent; while it
//! runs, the changes to the item's brief pushed into it and a question the program stops on answered by the
//! job's own places or put to the owner; and once it answered, what it was sent written down under the session
//! that answered. A co-work branch's turn goes on a session it throws away: no reset, record or work result
//! reaches it.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use std::sync::LazyLock;
use std::time::Duration;

use job_agency::{
  AsideOutcome, ConfirmAnswer, Confirmation, LogNote, Reply, Resume, TurnAsking, TurnMapping, TurnSeat,
  TurnSpan, TurnState, WentOut,
};
use job_gate::Sanitise;
use regex::Regex;
use serde::Deserialize;

use super::confirm::{Asker, Asking, OWN_WORK_NOTE, is_destructive};
use super::failure::{Failure, of_unanswered};
use super::own_work::{Places, own_work};

use crate::job_run::Ground;
use crate::job_run::notes::{self, Note};
use crate::job_run::prompt::{LABEL, item_scratch};
use crate::job_run::recovery::execution_root;
use crate::queue::job_start::TURN_USER_VARIABLE;
use crate::queue::routes::{CLAUDE_CODE, JOB_WORK_KEY};
use crate::queue::settings;
use crate::{Refusal, all, now_ms, one, run};

/// The provider a job's turn goes out on where the walk names no other.
pub const PROVIDER: &str = CLAUDE_CODE;

// The words the program says when it is asked to resume a session it no longer holds.
const UNKNOWN_SESSION: &str = "No conversation found with session ID";

// How long a turn that refused words while it waits on a person is left before it is offered them again.
const ASIDE_RETRY_MS: i64 = 60_000;

/// How often a running turn is read, as a job's run stands on it.
pub const TURN_POLL: Duration = Duration::from_secs(2);

// How long a session's sent results and served item are kept after the session's last write.
const KEPT_MS: i64 = 7 * 86_400_000;

/// What a format turn needs of the job: its prompt, which goes with a run that does not continue the session
/// that wrote the design.
#[derive(Clone, Debug)]
pub struct Shaping {
  pub job_prompt: String,
  pub design_session: Option<String>,
}

/// One turn as an attempt asks for it: the road it goes out on, what is said, on which model and session, the
/// mapping it goes out on, a format turn's shaping, and the run's name with what a person watching reads.
#[derive(Clone, Debug)]
pub struct Asked {
  pub provider: String,
  pub prompt: String,
  pub model: String,
  pub resume: Resume,
  pub attempt: TurnMapping,
  pub shaping: Option<Shaping>,
  pub name: String,
  pub title: String,
}

/// The road of one job: what every turn on it stands on, and what one turn leaves the next.
pub struct ClaudeRoad<'a> {
  pub ground: &'a Ground<'a>,
  job_id: i64,
  chat_id: i64,
  pub unit: String,
  worker: String,
  seat: TurnSeat,
  pub cwd: String,
  pub todo_id: Option<i64>,
  /// When the job's row was opened, in epoch milliseconds.
  pub launched_at: Option<i64>,
  managed: bool,
  user: Option<String>,
  // What every turn so far left of where its time went.
  spans: Vec<TurnSpan>,
}

/// What the road is opened with: a co-work branch's seat where the run is one, the worker's otherwise.
pub struct Opening<'a> {
  pub ground: &'a Ground<'a>,
  pub job_id: i64,
  pub chat_id: i64,
  pub unit: String,
  pub worker: String,
  pub cwd: String,
  pub managed: bool,
  pub branch: Option<TurnSeat>,
}

impl<'a> ClaudeRoad<'a> {
  /// The road for one job.
  pub async fn open(opening: Opening<'a>) -> Result<Self, Refusal> {
    let ground = opening.ground;
    let job = one(
      ground.db,
      r#"SELECT "todo_id", "launched_at" FROM "jobs" WHERE "id" = $1"#,
      vec![opening.job_id.into()],
    )
    .await?;
    let todo_id = job.as_ref().and_then(|row| row.try_get::<Option<i64>>("", "todo_id").ok().flatten());
    let launched_at = job
      .as_ref()
      .and_then(|row| row.try_get::<Option<i64>>("", "launched_at").ok().flatten());

    Ok(Self {
      ground,
      job_id: opening.job_id,
      chat_id: opening.chat_id,
      unit: opening.unit,
      worker: opening.worker,
      seat: opening.branch.unwrap_or(TurnSeat::Worker),
      cwd: opening.cwd,
      todo_id,
      launched_at,
      managed: opening.managed,
      user: std::env::var(TURN_USER_VARIABLE)
        .ok()
        .map(|user| user.trim().to_owned())
        .filter(|user| !user.is_empty()),
      spans: vec![],
    })
  }

  /// Whether the turns go on a session thrown away once they end: a co-work branch's, which leaves the worker's
  /// own sessions as they were.
  pub fn throws_session_away(&self) -> bool {
    matches!(self.seat, TurnSeat::Branch { .. })
  }

  /// The name the turns run under: the worker's, or the branch's own.
  pub fn worker(&self) -> &str {
    &self.worker
  }

  /// Where the turns' time went so far.
  pub fn spans(&self) -> Vec<TurnSpan> {
    self.spans.clone()
  }

  /// A line in the job's run log, which a run with no unit keeps none of.
  pub async fn note(&self, text: &str, source: Option<&str>, state: Option<&str>) {
    if self.unit.is_empty() {
      return;
    }

    let note = LogNote {
      agent: self.worker.clone(),
      run: self.unit.clone(),
      chat: Some(self.chat_id),
      job: Some(self.job_id),
      text: text.to_owned(),
      source: source.map(str::to_owned),
      state: state.map(str::to_owned),
    };

    if let Err(why) = self.ground.calls.write_note(&note).await {
      eprintln!("bmo-job: {}: a line of the run's log was not written: {}", self.worker, why.error);
    }
  }

  /// One turn, carried by bmo-agency on the logins the job's key is offered; a reset the owner asked of the
  /// worker's sessions is bmo-agency's to carry out before the turn goes.
  pub async fn turn(&mut self, asked: &Asked) -> Result<Reply, Failure> {
    match self.once(asked, asked.resume.clone()).await? {
      Carried::Answered(reply) => Ok(*reply),
      // A session the program no longer holds is opened again fresh.
      Carried::Unanswered(evidence)
        if asked.resume != Resume::Fresh && evidence.contains(UNKNOWN_SESSION) =>
      {
        match self.once(asked, Resume::Fresh).await? {
          Carried::Answered(reply) => Ok(*reply),
          Carried::Unanswered(evidence) => Err(of_unanswered(&evidence, JOB_WORK_KEY)),
        }
      }
      Carried::Unanswered(evidence) => Err(of_unanswered(&evidence, JOB_WORK_KEY)),
    }
  }

  // One work turn from its preparation to its end.
  async fn once(&mut self, asked: &Asked, resume: Resume) -> Result<Carried, Failure> {
    let ground = self.ground;
    let mapping = &asked.attempt;

    note_job_execution(
      ground,
      &self.unit,
      &asked.provider,
      &mapping.model,
      mapping.effort.as_deref(),
      mapping.tier.as_deref(),
    )
    .await
    .map_err(refused)?;
    self
      .note(
        &format!("{} · {}", asked.provider, mapping.model),
        Some(&asked.provider),
        Some("running"),
      )
      .await;

    let asking = TurnAsking {
      agent: self.worker.clone(),
      seat: self.seat.clone(),
      provider: asked.provider.clone(),
      model: asked.model.clone(),
      mapping: mapping.clone(),
      resume,
      cwd: self.cwd.clone(),
      name: asked.name.clone(),
      title: asked.title.clone(),
      thread: self.todo_id.map(|todo| format!("todo:{todo}")),
      job: Some(self.job_id),
      item: self.todo_id,
      chat: self.chat_id,
      unit: (!self.unit.is_empty()).then(|| self.unit.clone()),
      user: self.user.clone(),
      work_key: JOB_WORK_KEY.to_owned(),
      hears_words: !self.throws_session_away(),
      variables: BTreeMap::new(),
    };
    let prepared = ground.calls.prepare_turn(&asking).await.map_err(refused)?;
    let mut sent = self.send(&prepared.turn, asked, prepared.session.as_deref()).await?;
    let mut watch = Watch::new(self).await;
    let mut noted_out: Option<WentOut> = None;

    loop {
      let Some(turn) = ground.calls.work_turn(&prepared.turn).await.map_err(refused)? else {
        return Err(Failure::Transport {
          message: "the work turn is gone from bmo-agency".to_owned(),
        });
      };

      if let Some(out) = turn.went_out.as_ref().filter(|out| noted_out.as_ref() != Some(*out)) {
        note_job_execution(
          ground,
          &self.unit,
          &out.provider,
          &out.model,
          out.effort.as_deref(),
          out.tier.as_deref(),
        )
        .await
        .map_err(refused)?;
        noted_out = Some(out.clone());
      }

      match turn.state {
        TurnState::Running { question } => {
          watch.question(self, &prepared.turn, question.as_ref()).await;
          watch.offer(self, &prepared.turn).await;
          tokio::time::sleep(ground.turn_poll).await;
        }
        TurnState::Reprepared => {
          sent = self.send(&prepared.turn, asked, turn.session.as_deref()).await?;
        }
        TurnState::Answered { reply } => {
          watch.settle(self).await;
          self.spans.extend(turn.spans);

          // A session thrown away keeps no record of what it was sent.
          if !self.throws_session_away() {
            let session = reply.session_id.clone().or(turn.session.clone()).unwrap_or_default();

            self.note_sent_results(&sent, &session, turn.compacted).await.map_err(refused)?;
          }

          return Ok(Carried::Answered(reply));
        }
        TurnState::Failed { failure, evidence } => {
          watch.settle(self).await;
          self.spans.extend(turn.spans);

          // Stopped by the owner while the turn ran: the ending is the stop, however the pane looked.
          if self.stopped().await {
            return Err(Failure::Stopped);
          }

          return match (failure, evidence) {
            (Some(failure), _) => Err(failure.into()),
            (None, evidence) => Ok(Carried::Unanswered(evidence.unwrap_or_default())),
          };
        }
      }
    }
  }

  // The turn's prompt as the session it continues is handed it, sent; the results it went out with whole.
  async fn send(
    &self,
    turn: &str,
    asked: &Asked,
    session: Option<&str>,
  ) -> Result<Vec<WorkResultHead>, Failure> {
    let db = self.ground.db;
    let mut prompt = asked.prompt.clone();

    if let Some(shaping) = &asked.shaping {
      let continues = shaping.design_session.is_some() && session == shaping.design_session.as_deref();

      prompt = shape_turn_prompt(&prompt, &shaping.job_prompt, continues);
    }

    // Opened only now, on the session the preparation settled: a session it replaced holds nothing, and a
    // result the continued session already read goes as its marker.
    let held = match (session, self.throws_session_away()) {
      (Some(id), false) => loaded_work_results(db, id).await.map_err(refused)?,
      _ => HashMap::new(),
    };
    let (opened, results) = open_work_results(&prompt, &held);
    // A session that last answered for another item is told so, ahead of everything else.
    let previous = match (session, self.managed) {
      (Some(id), true) => work_session_item(db, id).await.map_err(refused)?,
      _ => None,
    };
    let prompt = match self.todo_id.and_then(|todo| item_switch_notice(previous, todo)) {
      Some(switched) => format!("{switched}\n\n{opened}"),
      None => opened,
    };

    self.ground.calls.send_turn_prompt(turn, &prompt).await.map_err(refused)?;

    Ok(results)
  }

  /// Whether this job has been stopped: the owner's stop of its item, or a cancel asked of its unit or of the
  /// root its retries share.
  pub async fn stopped(&self) -> bool {
    let ground = self.ground;

    if settings::get(ground.db, &format!("job_stop:{}", self.job_id))
      .await
      .ok()
      .flatten()
      .is_some()
    {
      return true;
    }

    if self.unit.is_empty() {
      return false;
    }

    let root = execution_root(&self.unit);

    ground.units.open_executions().await.is_ok_and(|executions| {
      executions.iter().any(|execution| {
        execution.cancelled && (execution.id == self.unit || Some(&execution.id) == root.as_ref())
      })
    })
  }

  // What a turn on the worker's own session was sent whole, written under the session that answered; a session
  // whose context was compacted during the turn holds none of it any more.
  async fn note_sent_results(
    &self,
    sent: &[WorkResultHead],
    session: &str,
    compacted: bool,
  ) -> Result<(), Refusal> {
    let db = self.ground.db;
    let now = now_ms();

    if session.is_empty() {
      return Ok(());
    }

    if self.managed
      && let Some(todo) = self.todo_id
    {
      run(
        db,
        r#"INSERT INTO "session_work_items" ("session_id", "todo_id", "served_at") VALUES ($1, $2, $3)
           ON CONFLICT ("session_id") DO UPDATE SET "todo_id" = excluded."todo_id", "served_at" = excluded."served_at""#,
        vec![session.into(), todo.into(), now.into()],
      )
      .await?;
      run(
        db,
        r#"DELETE FROM "session_work_items" WHERE "served_at" < $1"#,
        vec![(now - KEPT_MS).into()],
      )
      .await?;
    }

    if sent.is_empty() && loaded_work_results(db, session).await?.is_empty() {
      return Ok(());
    }

    if compacted {
      run(
        db,
        r#"DELETE FROM "session_work_results" WHERE "session_id" = $1"#,
        vec![session.into()],
      )
      .await?;
      return Ok(());
    }

    for head in sent {
      run(
        db,
        r#"INSERT INTO "session_work_results" ("session_id", "result", "version", "loaded_at") VALUES ($1, $2, $3, $4)
           ON CONFLICT ("session_id", "result") DO UPDATE SET "version" = excluded."version", "loaded_at" = excluded."loaded_at""#,
        vec![session.into(), head.key.clone().into(), head.version.clone().into(), now.into()],
      )
      .await?;
    }

    if !sent.is_empty() {
      run(
        db,
        r#"DELETE FROM "session_work_results" WHERE "session_id" IN
             (SELECT "session_id" FROM "session_work_results" GROUP BY "session_id" HAVING MAX("loaded_at") < $1)"#,
        vec![(now - KEPT_MS).into()],
      )
      .await?;
    }

    Ok(())
  }

  /// Whether the session the run's last reply came back on is the one bmo-agency's record of the worker's
  /// session on `model` still continues: bmo-agency records it as the turn answers, and drops it where it
  /// answered on a lesser model. A branch's session was thrown away, and a reply on another provider or naming no
  /// session leaves no Claude session to stand.
  pub async fn session_stands(&self, reply: &Reply, model: &str) -> bool {
    let worker = &self.worker;

    if self.throws_session_away() {
      return false;
    }

    if reply.mode != PROVIDER {
      println!("{worker}: {} keeps its own session contract; no Claude session stands", reply.mode);
      return false;
    }

    let Some(session) = &reply.session_id else {
      println!("{worker}: the Claude answer named no session; none stands");
      return false;
    };

    match self.ground.calls.session_kept(worker, model, session).await {
      Ok(true) => {
        println!("{worker} session for {model}: {session} kept");
        true
      }
      Ok(false) => {
        println!("{worker} session for {model}: {session} dropped, so the next job opens a fresh one");
        false
      }
      Err(why) => {
        eprintln!("bmo-job: {worker} session for {model}: its record could not be read — {}", why.error);
        false
      }
    }
  }
}

fn refused(why: Refusal) -> Failure {
  Failure::error(why.error)
}

// What one work turn came to where it did not fail outright: its answer, or the evidence of a turn that did not
// answer.
enum Carried {
  Answered(Box<Reply>),
  Unanswered(String),
}

// What a running turn is watched with: the notes offered it and what became of them, the job's own places a
// command it stops on is judged by, and a question put to the owner while it waits.
struct Watch {
  asker: Asker,
  places: Places,
  waiting_since: Option<i64>,
  offered: Option<Vec<Note>>,
  asking: Option<Asking>,
}

impl Watch {
  async fn new(road: &ClaudeRoad<'_>) -> Self {
    let ground = road.ground;
    let item = match road.todo_id {
      Some(todo) => item_scratch(ground, &road.worker, todo).await.ok(),
      None => None,
    };
    let home = std::env::var("HOME").unwrap_or_default();

    Self {
      asker: Asker {
        todo_id: road.todo_id,
        job_id: road.job_id,
      },
      places: Places::of_job(
        Path::new(&road.cwd),
        item.as_deref(),
        ground.testbed_logs.as_deref(),
        &ground.tree,
        Path::new(&home),
      ),
      waiting_since: None,
      offered: None,
      asking: None,
    }
  }

  // The question the turn stands on, answered: by the job's own places where it is a command that stays inside
  // them, by the owner otherwise; one the pane stopped asking before the owner answered it is withdrawn.
  async fn question(&mut self, road: &ClaudeRoad<'_>, turn: &str, question: Option<&job_agency::Question>) {
    let ground = road.ground;

    if let Some(asking) = self.asking.take() {
      if question.is_some_and(|question| question.id == asking.question) {
        match self.asker.settled(ground, &asking).await {
          Ok(Some(answer)) => answer_question(road, turn, &asking.question, &answer).await,
          Ok(None) => self.asking = Some(asking),
          Err(why) => {
            eprintln!(
              "bmo-job: {}: the question put to the owner could not be read: {}",
              road.worker, why.error
            );
            self.asking = Some(asking);
          }
        }

        return;
      }

      if let Err(why) = self.asker.withdraw(ground, &asking).await {
        eprintln!(
          "bmo-job: {}: the question put to the owner could not be withdrawn: {}",
          road.worker, why.error
        );
      }
    }

    let Some(question) = question else { return };

    // A command that stays inside the job's own places is allowed here, and nothing is put to anybody.
    if let Confirmation::Permission { text, .. } = &question.kind
      && !is_destructive(text)
      && own_work(text, &self.places)
    {
      road.note(OWN_WORK_NOTE, None, None).await;
      answer_question(road, turn, &question.id, &ConfirmAnswer::Allow).await;
      return;
    }

    match self.asker.ask(ground, &question.id, &question.kind).await {
      Ok(Ok(asking)) => self.asking = Some(asking),
      Ok(Err(answer)) => answer_question(road, turn, &question.id, &answer).await,
      Err(why) => {
        eprintln!("bmo-job: {}: the question could not be put to the owner: {}", road.worker, why.error)
      }
    }
  }

  // The changes to the item's brief no turn has been handed, pushed into the running turn through the
  // sanitiser; a turn that waits on a person is offered them again a minute later.
  async fn offer(&mut self, road: &ClaudeRoad<'_>, turn: &str) {
    let ground = road.ground;

    if self.waiting_since.is_some_and(|since| now_ms() - since < ASIDE_RETRY_MS) {
      return;
    }

    let (Some(todo), Some(since)) = (road.todo_id, road.launched_at) else { return };
    let pending = match notes::undelivered(ground.db, todo, since).await {
      Ok(pending) => pending,
      Err(why) => {
        eprintln!(
          "bmo-job: {}: the notes for the running turn could not be read: {}",
          road.worker, why.error
        );
        return;
      }
    };

    if pending.is_empty() {
      return;
    }

    let english = match ground
      .units
      .english(&pending.iter().map(|note| note.text.clone()).collect::<Vec<_>>())
      .await
    {
      Ok(english) => english,
      Err(_) => pending.iter().map(|note| note.text.clone()).collect(),
    };
    let handed: Vec<Note> = pending
      .iter()
      .zip(english)
      .map(|(note, text)| Note {
        text,
        ..note.clone()
      })
      .collect();
    let asked = Sanitise {
      text: notes::aside_task(todo, &handed, &ground.agent_name),
      label: LABEL.to_owned(),
      chat: Some(road.chat_id),
    };
    let safe = match ground.calls.sanitise(&asked).await {
      Ok(safe) => safe.text,
      Err(why) => {
        eprintln!(
          "bmo-job: {}: the notes for the running turn were not sanitised: {}",
          road.worker, why.error
        );
        return;
      }
    };
    let worker = &road.worker;
    let count = pending.len();

    self.offered = Some(pending);

    match ground.calls.push_aside(turn, &safe).await {
      Ok(AsideOutcome::Delivered) => {
        let ids: Vec<i64> = self.offered.take().unwrap_or_default().iter().map(|note| note.id).collect();

        if let Err(why) = notes::mark_delivered(ground.db, &ids, now_ms()).await {
          eprintln!("bmo-job: {worker}: the notes could not be marked delivered: {}", why.error);
        }

        ground.screens_moved().await;
        road
          .note(&format!("{count} note(s) were pushed into the running turn"), None, None)
          .await;
        println!("{worker}: {count} note(s) pushed into the turn that is running");
      }
      Ok(AsideOutcome::Waiting) => {
        self.offered = None;
        self.waiting_since = Some(now_ms());
        road
          .note(
            &format!("{count} note(s) were not handed over: the run is waiting on a person"),
            None,
            None,
          )
          .await;
        println!("{worker}: the run is waiting on a person, so {count} note(s) stay undelivered");
      }
      Ok(AsideOutcome::Refused) => {
        self.offered = None;
        eprintln!("bmo-job: {worker}: could not push {count} note(s) into the running turn: refused");
      }
      Err(why) => {
        self.offered = None;
        eprintln!("bmo-job: {worker}: could not push {count} note(s) into the running turn: {}", why.error);
      }
    }
  }

  // A question still put to the owner as the turn ends, withdrawn: the pane no longer asks it.
  async fn settle(&mut self, road: &ClaudeRoad<'_>) {
    if let Some(asking) = self.asking.take()
      && let Err(why) = self.asker.withdraw(road.ground, &asking).await
    {
      eprintln!(
        "bmo-job: {}: the question put to the owner could not be withdrawn: {}",
        road.worker, why.error
      );
    }
  }
}

async fn answer_question(road: &ClaudeRoad<'_>, turn: &str, question: &str, answer: &ConfirmAnswer) {
  if let Err(why) = road.ground.calls.answer_question(turn, question, answer).await {
    eprintln!("bmo-job: {}: the pane's question could not be answered: {}", road.worker, why.error);
  }
}

/// The job's row told the road its execution went out on.
pub async fn note_job_execution(
  ground: &Ground<'_>,
  unit: &str,
  mode: &str,
  model: &str,
  effort: Option<&str>,
  tier: Option<&str>,
) -> Result<(), Refusal> {
  run(
    ground.db,
    r#"UPDATE "jobs" SET "execution_mode" = $1, "execution_model" = $2, "execution_effort" = $3, "execution_tier" = $4, "execution_upstream" = NULL
       WHERE "unit" = $5 AND "finished_at" IS NULL"#,
    // A run that names no effort leaves the column empty.
    vec![
      mode.into(),
      model.into(),
      effort.filter(|effort| !effort.is_empty()).map(str::to_owned).into(),
      tier.map(str::to_owned).into(),
      unit.into(),
    ],
  )
  .await?;

  Ok(())
}

/// What a format turn is sent: the request alone to the session that wrote the design, the job's prompt ahead
/// of it to any other run.
pub fn shape_turn_prompt(request: &str, job_prompt: &str, continues_design_session: bool) -> String {
  if continues_design_session {
    request.to_owned()
  } else {
    format!("{job_prompt}\n\n{request}")
  }
}

/// An earlier result a prompt carries wrapped: the key it is kept under, the version of its text and the
/// document it was saved as.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct WorkResultHead {
  pub key: String,
  pub version: String,
  #[serde(default)]
  pub document: Option<String>,
}

static WORK_RESULT: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?s)WORK-RESULT (\{[^\n]*\})\n(.*?)\nEND WORK-RESULT").expect("the work result pattern")
});

/// The prompt with every wrapped result opened: one the session holds in that very version becomes its marker,
/// every other its own text; what went out whole is named.
pub fn open_work_results(prompt: &str, held: &HashMap<String, String>) -> (String, Vec<WorkResultHead>) {
  if !prompt.contains("WORK-RESULT ") {
    return (prompt.to_owned(), vec![]);
  }

  let mut sent = vec![];
  let opened = WORK_RESULT.replace_all(prompt, |found: &regex::Captures| {
    let text = found[2].to_owned();
    let Some(head) = serde_json::from_str::<WorkResultHead>(&found[1]).ok() else {
      return text;
    };

    if held.get(&head.key) == Some(&head.version) {
      return match &head.document {
        None => format!("({}: already delivered earlier in this session, unchanged)", head.key),
        Some(document) => {
          format!(
            "({}: already delivered earlier in this session, unchanged; result document: {document})",
            head.key
          )
        }
      };
    }

    sent.push(head);
    text
  });

  (opened.into_owned(), sent)
}

/// What a turn is told when the session it continues last answered for another item.
pub fn item_switch_notice(previous: Option<i64>, current: i64) -> Option<String> {
  let previous = previous.filter(|previous| *previous != current)?;

  Some(format!(
    "[Item switch]\nThe previous turn in this session was for TODO #{previous}. This turn is for TODO #{current}: what that turn read and decided belongs to #{previous}, and nothing of it is this item's work unless this prompt says so."
  ))
}

async fn loaded_work_results(
  db: &sea_orm::DatabaseConnection,
  session: &str,
) -> Result<HashMap<String, String>, Refusal> {
  let rows = all(
    db,
    r#"SELECT "result", "version" FROM "session_work_results" WHERE "session_id" = $1"#,
    vec![session.into()],
  )
  .await?;

  rows
    .iter()
    .map(|row| Ok((row.try_get("", "result")?, row.try_get("", "version")?)))
    .collect()
}

async fn work_session_item(
  db: &sea_orm::DatabaseConnection,
  session: &str,
) -> Result<Option<i64>, Refusal> {
  Ok(
    one(
      db,
      r#"SELECT "todo_id" FROM "session_work_items" WHERE "session_id" = $1"#,
      vec![session.into()],
    )
    .await?
    .map(|row| row.try_get::<i64>("", "todo_id"))
    .transpose()?,
  )
}
