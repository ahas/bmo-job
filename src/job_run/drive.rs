//! The turns of a job, driven by the runner itself (docs/runs.md, "The turns"): the job's first turn walked, the
//! worker's keepalive woken, its run noted and each answer's usage recorded; a work step's shaping turns with
//! each refused answer kept; the rounds over the words added to the item after a turn; and the one more turn
//! over the tasks a turn left running, the change it left open, the graph it reached unanswered for or the
//! item it left open, a shaping turn's request composed and its answer checked here (plan_check.rs); and, once
//! the ending is written, the session handed over with the worker's own note or a summary, and the rest a done
//! job's usage calls for. Every turn goes out on the road its selection names (`carry_turn`).

use job_agency::{Handover, ModelSpend, Reply, Resume, TurnMapping};
use job_codes::job_ended::Selection;
use job_codes::job_roads::WorkOutput;
use job_codes::{Fact, JobFact};

use super::branch_run;
use super::claude::failure::Failure;
use super::claude::turn::{self, ClaudeRoad, PROVIDER, Shaping};
use super::ending::RunFacts;
use super::notes;
use super::plan_check::{self, PLAN, Roster};
use super::prompt::{self, verbs_word};
use super::recovery::{AttemptAsked, Attempted, Attempts, ROUTE_EXHAUSTED, Raised, Walk};
use super::verdict::{self, abandoned_workflows, change_names, live_task_names, nothing_reported};
use super::{Run, WORK_FINAL_PHASE, WORK_SESSION_HANDOVER, WORK_STEP_PHASE, judge};
use crate::now_ms;
use crate::order_run::context;
use crate::queue::outcomes::{JOB_BLOCKED, JOB_ERROR, JOB_FAILED, RUN_STOPPED};
use crate::queue::work_orders;

/// One turn on the road it names, carried on the job's road; a co-work branch's on Codex or OpenRouter made
/// something of as a stand-in road's answer.
pub async fn carry_turn(road: &mut ClaudeRoad<'_>, asked: &turn::Asked) -> Result<Reply, Failure> {
  let reply = road.turn(asked).await?;

  if road.throws_session_away() && branch_run::carries(&asked.provider) {
    return branch_run::answered(road, asked, reply).await.map_err(|why| Failure::error(why.error));
  }

  Ok(reply)
}

// How many times one run is asked again over words added after it answered: more than one, since a word added
// during the follow-up is one nobody has been handed either, and bounded, since a stream of them would otherwise
// keep the job alive for as long as somebody kept typing.
const NOTE_ROUNDS: usize = 3;

// How many times a refused shape is asked for again before the step stops.
const SHAPE_CORRECTIONS: u32 = 2;

/// An ending the work came to before its last answer was judged: the report, its outcome and whether a gate
/// reads it.
pub struct Halted {
  pub text: String,
  pub outcome: &'static str,
  pub gated: bool,
}

/// What the turns came to: the run's last answer to judge, or an ending of their own.
pub enum Driven {
  Answered(Box<Reply>),
  Halted(Halted),
}

// What stopped a walk: a failure the ending is written from, or an ending an attempt already came to.
enum Halt {
  Failed(Failure),
  Ended(Halted),
}

/// What the turns stand on beyond the run's frame: the prompt they go out on, the trailer a later turn is
/// held to, and what a person watching reads of the turn.
pub struct Given {
  pub prompt: String,
  pub trailer: String,
  pub title: String,
}

/// One job's turns, driven: the frame they run in, the road they go out on, the walk each run goes through, and
/// what the run measured so far.
pub(super) struct Driver<'d, 'g> {
  pub(super) run: &'d mut Run<'g>,
  pub(super) road: &'d mut ClaudeRoad<'g>,
  walk: &'d Walk<'g>,
  given: Given,
  // The model the last attempt went out on, and the provider and model the facts call the run's own.
  model: String,
  actual: Option<Selection>,
  run_started_at: i64,
  run_ended_at: i64,
  spend: Vec<ModelSpend>,
  // Whether the worker's kept session was handed over ahead of a stopped stage's retry.
  handed_over: bool,
}

impl<'d, 'g> Driver<'d, 'g> {
  pub(super) fn new(
    run: &'d mut Run<'g>,
    road: &'d mut ClaudeRoad<'g>,
    walk: &'d Walk<'g>,
    given: Given,
  ) -> Self {
    let started = run.ending.started_at;

    Self {
      model: run.args.model.clone(),
      actual: selected(&run.args.model),
      run,
      road,
      walk,
      given,
      run_started_at: started,
      run_ended_at: started,
      spend: Vec::new(),
      handed_over: false,
    }
  }

  /// What the run measured, for the ending's record.
  pub fn facts(&self) -> RunFacts {
    RunFacts {
      requested: self.run.args.model.clone(),
      actual: self.actual.clone(),
      run_started_at: self.run_started_at,
      run_ended_at: self.run_ended_at,
      spend: self.spend.clone(),
      spans: self.road.spans(),
    }
  }

  /// The model the last attempt went out on.
  pub fn model(&self) -> &str {
    &self.model
  }

  fn worker(&self) -> String {
    self.run.worker.clone()
  }

  /// A line in the job's run log.
  pub async fn note(&self, text: &str) {
    self.road.note(text, None, None).await;
  }

  async fn note_run(&self) {
    if let Err(why) = self.run.ground().calls.note_run(&self.run.worker).await {
      eprintln!("bmo-job: {}: the run could not be noted: {}", self.run.worker, why.error);
    }
  }

  // A later turn's answer taken as the run's own: its spend added to what the run spent, and its words the report
  // from here on. Its usage is recorded by bmo-agency as the turn answers.
  async fn adopt(&mut self, reply: &mut Reply, answered: Reply) {
    self.spend.extend(answered.spend.iter().cloned());
    *reply = answered;
  }

  // One run walked over the providers and the tiers from the selection given, each attempt carried here.
  async fn recovering(
    &mut self,
    prompt: &str,
    preferred: &str,
    resume: Resume,
    shaping: Option<Shaping>,
  ) -> Result<Reply, Halt> {
    let walk = self.walk;
    let mut steering = Steering {
      driver: self,
      prompt,
      resume,
      shaping,
      raised: None,
      ended: None,
    };
    let walked = walk.walk(preferred, &mut steering).await;

    if let Some(ended) = steering.ended {
      return Err(Halt::Ended(ended));
    }

    match walked {
      Ok(reply) => Ok(reply),
      Err(Failure::Attempt) => Err(Halt::Failed(
        steering
          .raised
          .unwrap_or_else(|| Failure::error("the walk named an attempt that raised nothing")),
      )),
      Err(failure) => Err(Halt::Failed(failure)),
    }
  }

  // A run that stopped before it had an answer, said where the job is read back: a stop and a question waiting
  // on a person as what they are, and anything else with its cause.
  async fn halted(&mut self, halt: Halt) -> Driven {
    let failure = match halt {
      Halt::Ended(ended) => return Driven::Halted(ended),
      Halt::Failed(failure) => failure,
    };

    self.run_ended_at = now_ms();

    let (text, outcome) = match &failure {
      Failure::Stopped => {
        eprintln!("the run was stopped from outside");
        self.note("The run was stopped from outside and ended").await;
        (
          "The run was stopped and ended here. What stood in its pane then is in this run's log."
            .to_owned(),
          RUN_STOPPED,
        )
      }
      Failure::Person => {
        eprintln!("the run is waiting on a person");
        self.note("The run stands waiting for something a person answers").await;
        (
          "The run stopped where a person has to answer. What stood in its pane then is in this run's log."
            .to_owned(),
          JOB_BLOCKED,
        )
      }
      other => {
        let cause = other.cause();

        eprintln!("the selected execution route did not answer: {cause}");
        self.note(&format!("The road did not answer: {cause}")).await;

        let text = match other {
          Failure::Exhausted { attempted } => format!(
            "{ROUTE_EXHAUSTED} every work provider the run could take is shut. Roads tried: {}",
            if attempted.is_empty() { "none".to_owned() } else { attempted.join(", ") }
          ),
          _ => format!("The run did not answer as it should: {cause}"),
        };

        (text, JOB_ERROR)
      }
    };

    Driven::Halted(Halted {
      text,
      outcome,
      gated: true,
    })
  }

  // Another turn of the same work, on the road and the session the last answer came back on, with the worker's
  // run noted on either side of it.
  async fn again(&mut self, reply: &Reply, prompt: &str) -> Result<Reply, Halt> {
    self.note_run().await;

    let answered = self.recovering(prompt, &selection_of(reply), resume_of(reply), None).await?;

    self.run_ended_at = now_ms();
    self.note_run().await;
    Ok(answered)
  }

  // Another turn of the same work over what `asking` asks, its answer taken as the run's own; the ending it came
  // to where it did not answer.
  async fn ask_again(&mut self, reply: &mut Reply, asking: &str) -> Result<(), Driven> {
    match self.again(reply, asking).await {
      Ok(answered) => {
        self.adopt(reply, answered).await;
        Ok(())
      }
      Err(halt) => Err(self.halted(halt).await),
    }
  }

  /// The job's turns, from the first to the last answer the ending is judged on.
  pub async fn drive(&mut self) -> Driven {
    let worker = self.worker();

    if let Err(why) = self.run.ground().calls.wake_keepalive(&worker).await {
      eprintln!("bmo-job: {worker}: its keepalive could not be woken: {}", why.error);
    }

    self.run_started_at = now_ms();

    let prompt = self.given.prompt.clone();
    let requested = self.run.args.model.clone();
    let mut reply = match self.recovering(&prompt, &requested, Resume::Auto, None).await {
      Ok(reply) => reply,
      Err(halt) => return self.halted(halt).await,
    };

    self.run_ended_at = now_ms();
    self.note_run().await;
    self.spend = reply.spend.clone();

    if let Some(kind) = self.run.args.work_output().map(str::to_owned)
      && !nothing_reported(&reply.text)
      && abandoned_workflows(&reply.workflows).is_empty()
      && let Err(driven) = self.shaped(&mut reply, &kind).await
    {
      return driven;
    }

    let step = self.run.args.phase == WORK_STEP_PHASE;

    if !step && let Err(driven) = self.rounds(&mut reply).await {
      return driven;
    }

    let ephemeral = self.run.args.ephemeral();
    let settles =
      |reply: &Reply| !step && !nothing_reported(&reply.text) && !ephemeral && reply.session_id.is_some();

    // A report handed over with background tasks still live is refused, and a task raised only to check the work
    // is the commonest way to hand one over after the work was done: the session is asked once to settle them,
    // and its answer is what the judgement reads.
    if settles(&reply) && !reply.live_tasks.is_empty() {
      let names = live_task_names(&reply.live_tasks, 4);

      println!(
        "{worker}: asking the session to settle {} background task(s) left running: {names}",
        reply.live_tasks.len()
      );
      self
        .note(&format!(
          "Asked once more to settle the background tasks its turn left running: {names}"
        ))
        .await;

      let asking = self.later(&verdict::left_running_task(&reply.live_tasks)).await;

      if let Err(driven) = self.ask_again(&mut reply, &asking).await {
        return driven;
      }
    }

    if settles(&reply) && reply.live_tasks.is_empty() && abandoned_workflows(&reply.workflows).is_empty() {
      // The same second chance for the job's own change still open: an uncommitted change and a graph left behind
      // by its own commits are still the session's to close, and the judgement reads the checkout and the graph
      // again rather than the answer's word.
      let tracked = self.run.tracked_changes();
      let graph = self.run.unrecorded_by_graph().await;

      if !tracked.is_empty() || !graph.is_empty() {
        let named: Vec<String> = tracked.iter().chain(&graph).cloned().collect();
        let names = change_names(&named, 8);

        println!("{worker}: asking the session to close its own change before judging it: {names}");
        self
          .note(&format!("Asked once more to close its own change, which its turn left open: {names}"))
          .await;

        let asking = self.later(&verdict::left_open_change(&tracked, &graph)).await;

        if let Err(driven) = self.ask_again(&mut reply, &asking).await {
          return driven;
        }

        // That turn committed, so what the job's own commits are is read again.
        self.run.own_files = None;
      }
    }

    // A change that reaches the graph is answered for node by node: the impact tool called on the item and the
    // report's impact part. The session is asked once, and the record and the report are read again rather than
    // the answer's word.
    if settles(&reply) && reply.live_tasks.is_empty() {
      let mapped = self.run.graph_mapped().await;
      let faults = self.impact_faults(&mapped, &reply.text).await;

      if faults.any() {
        println!(
          "{worker}: asking the session to answer for the graph its change reached: {}",
          change_names(&mapped, 8)
        );

        let asking = self.later(&verdict::left_impact(self.run.ground().policy, faults, &mapped)).await;

        if let Err(driven) = self.ask_again(&mut reply, &asking).await {
          return driven;
        }

        let left = self.impact_faults(&mapped, &reply.text).await;

        self.run.left_impact = (left, mapped);
      }
    }

    // The job's item is the job's own to close or hold, and one it left open stands in the queue as work still
    // waiting for the worker: the session is asked once, and the item is read again rather than the answer.
    if settles(&reply)
      && reply.live_tasks.is_empty()
      && let Some(todo) = self.road.todo_id
      && self.left_open(todo).await
    {
      println!(
        "{worker}: asking the session to close or hold its item #{todo}, which its answer left open"
      );

      let ground = self.run.ground();
      let bmo = verbs_word(&ground.tree);
      let add_step = (self.run.args.phase == WORK_FINAL_PHASE)
        .then(|| context::add_step(ground.policy, &ground.tree, todo));
      let asking = self.later(&verdict::left_open_item(todo, &bmo, add_step.as_deref())).await;

      if let Err(driven) = self.ask_again(&mut reply, &asking).await {
        return driven;
      }

      if self.left_open(todo).await {
        self.run.left_open_item = Some(todo);
      }
    }

    Driven::Answered(Box::new(reply))
  }

  // What the report and the record leave out of answering for the graph the job's commits reached: the calls of
  // the impact tool are the item's earlier jobs' and this run's own so far, a call made on the plan turn of a
  // co-work counting for its final turn.
  async fn impact_faults(&self, mapped: &[String], text: &str) -> verdict::ImpactFaults {
    let policy = self.run.ground().policy;

    if mapped.is_empty() {
      return verdict::ImpactFaults::default();
    }

    let earlier = match self.road.todo_id {
      Some(todo) => self.earlier_impact_calls(todo).await,
      None => 0,
    };

    verdict::impact_faults(
      policy,
      mapped,
      &verdict::report_body(text),
      earlier + verdict::impact_calls(policy, &self.road.spans()),
    )
  }

  // The impact tool's calls the item's other jobs recorded; none where the record cannot be read, which asks the
  // session for the call rather than passing a change nobody can show was weighed.
  async fn earlier_impact_calls(&self, todo: i64) -> i64 {
    let ground = self.run.ground();
    let rows = crate::all(
      ground.db,
      r#"SELECT "times" FROM "job_spans"
         WHERE "todo_id" = $1 AND "job_id" <> $2 AND "kind" = 'tool' AND "name" LIKE $3"#,
      vec![
        todo.into(),
        self.run.args.job_id.into(),
        format!("%{}", ground.policy.closing.impact.tool).into(),
      ],
    )
    .await;

    match rows {
      Ok(rows) => rows.iter().filter_map(|row| row.try_get::<i64>("", "times").ok()).sum(),
      Err(why) => {
        eprintln!(
          "bmo-job: item #{todo}: the impact tool's earlier calls could not be read: {}",
          why.error
        );
        0
      }
    }
  }

  // Whether the job left its item open where it was its to close: a work order's final turn that added steps
  // ahead of it hands the order back to its build and leaves the item open by design.
  async fn left_open(&self, todo: i64) -> bool {
    if !self.item_open(todo).await {
      return false;
    }

    if self.run.args.phase != WORK_FINAL_PHASE {
      return true;
    }

    !matches!(queue_rows::work_orders::read(self.run.ground().db, todo).await, Ok(Some(order)) if work_orders::steps_ahead(&order))
  }

  // Whether the job's item still stands open: neither marked done, held nor merged into another. An item that
  // cannot be read is taken as closed, since asking about it could only hold a finished job back.
  async fn item_open(&self, todo: i64) -> bool {
    let row = crate::one(
      self.run.ground().db,
      r#"SELECT 1 AS "open" FROM "todos" WHERE "id" = $1 AND "done" = 0 AND "held_at" IS NULL AND "merged_into" IS NULL"#,
      vec![todo.into()],
    )
    .await;

    match row {
      Ok(row) => row.is_some(),
      Err(why) => {
        eprintln!("bmo-job: item #{todo}: whether it stands open could not be read: {}", why.error);
        false
      }
    }
  }

  // A later turn's prompt, through the sanitiser with the job's trailer after it.
  async fn later(&self, task: &str) -> String {
    let composed = prompt::later(self.run.ground(), self.run.args.chat_id, task, &self.given.trailer).await;

    if composed.defused {
      println!("follow-up prompt: an order to attack a real target was defused before the run");
    }

    composed.prompt
  }

  // A control step's design shaped: the shape asked for in a turn of its own whose answer alone is parsed,
  // checked while the run can still ask again, and each refused answer kept with the reason it was refused.
  async fn shaped(&mut self, reply: &mut Reply, kind: &str) -> Result<(), Driven> {
    let ground = self.run.ground();
    let design = reply.text.clone();
    let mut previous = design.clone();
    let mut reason: Option<String> = None;
    let directory = self.run.output_folder();
    let output = if kind == PLAN { WorkOutput::Plan } else { WorkOutput::Synthesis };
    let roster = match Roster::read(ground.units).await {
      Ok(roster) => roster,
      Err(why) => return Err(self.halted(Halt::Failed(Failure::error(why.error))).await),
    };

    for correction in 0.. {
      let request = plan_check::structure_request(&roster, kind, &previous, reason.as_deref());
      let shaping = Shaping {
        job_prompt: self.given.prompt.clone(),
        design_session: reply.session_id.clone(),
      };

      match self.recovering(&request, &selection_of(reply), resume_of(reply), Some(shaping)).await {
        Ok(answered) => {
          self.run_ended_at = now_ms();
          self.adopt(reply, answered).await;
        }
        Err(halt) => return Err(self.halted(halt).await),
      }

      let checked = if kind == PLAN {
        plan_check::read_plan(&roster, ground.policy, &reply.text, &selection_of(reply)).map(drop)
      } else {
        plan_check::read_build_tasks(&roster, &reply.text).map(drop)
      };
      let Err(refused) = checked else { return Ok(()) };

      let _ = std::fs::create_dir_all(&directory);
      let _ = std::fs::write(directory.join(format!("{correction}.json")), &reply.text);
      let _ = std::fs::write(directory.join(format!("{correction}.error.txt")), &refused);
      // The turn that did the thinking is the one worth keeping when the shaping is what failed.
      if correction == 0 {
        let _ = std::fs::write(directory.join("design.txt"), &design);
      }

      ground
        .tell(Fact::Job(JobFact::WorkOutputRefused {
          job_id: self.run.args.job_id,
          worker: self.run.worker.clone(),
          output,
          reason: refused.clone(),
          correction,
        }))
        .await;

      if correction >= SHAPE_CORRECTIONS || !abandoned_workflows(&reply.workflows).is_empty() {
        return Err(Driven::Halted(Halted {
          text: format!(
            "The co-work's {} did not come in the JSON shape it is held to, and asking again for the shape did not mend it, so the step stopped. The validator's reasons and the answers received are in `{}`.",
            if kind == PLAN { "plan" } else { "build list" },
            directory.display()
          ),
          outcome: JOB_FAILED,
          gated: false,
        }));
      }

      previous = reply.text.clone();
      reason = Some(refused);
    }

    Ok(())
  }

  // The words added to the item after the run answered, and the session asked again over them, so what it says
  // the second time is the report. What is added while a turn works reaches that turn instead.
  async fn rounds(&mut self, reply: &mut Reply) -> Result<(), Driven> {
    let ground = self.run.ground();
    let (Some(todo), Some(since)) = (self.road.todo_id, self.road.launched_at) else {
      return Ok(());
    };

    for _ in 0..NOTE_ROUNDS {
      let pending = match notes::undelivered(ground.db, todo, since).await {
        Ok(pending) => pending,
        Err(why) => {
          eprintln!("bmo-job: item #{todo}: the words added to it could not be read: {}", why.error);
          break;
        }
      };

      if pending.is_empty() {
        break;
      }

      println!(
        "item #{todo}: asking the session again over {} change(s) made while the job ran",
        pending.len()
      );

      let texts: Vec<String> = pending.iter().map(|note| note.text.clone()).collect();
      let english = ground.units.english(&texts).await.unwrap_or(texts);
      let handed: Vec<notes::Note> = pending
        .iter()
        .zip(english)
        .map(|(note, text)| notes::Note {
          text,
          ..note.clone()
        })
        .collect();
      let asking = self
        .later(&notes::follow_up_task(todo, &handed, &verbs_word(&ground.tree), &ground.agent_name))
        .await;
      let answered = match self.again(reply, &asking).await {
        Ok(answered) => answered,
        Err(halt) => return Err(self.halted(halt).await),
      };
      let ids: Vec<i64> = pending.iter().map(|note| note.id).collect();

      if let Err(why) = notes::mark_delivered(ground.db, &ids, now_ms()).await {
        eprintln!("bmo-job: item #{todo}: the notes could not be marked delivered: {}", why.error);
      }

      ground.screens_moved().await;
      self.adopt(reply, answered).await;
    }

    Ok(())
  }

  /// Why the session is handed over, read off the job's own commits while the checkout still stands on them.
  pub async fn handover_reason(&mut self) -> String {
    let files = self.run.own_commit_files();
    let touched =
      self
        .run
        .ground()
        .calls
        .prefix_touched(&self.run.worker, &files)
        .await
        .unwrap_or_else(|why| {
          eprintln!(
            "bmo-job: {}: what of its change touches its prefix could not be read: {}",
            self.run.worker, why.error
          );
          vec![]
        });

    if touched.is_empty() {
      "ended its job".to_owned()
    } else {
      format!("changed the prefix it was opened on: {}", touched.join(", "))
    }
  }

  /// The run's last answer judged, with the report, its outcome and whether a gate reads it.
  pub async fn judge(&mut self, reply: &Reply) -> (String, &'static str, bool) {
    judge(self.run, &reply.text, &reply.live_tasks, &reply.workflows).await
  }

  /// What follows a written ending that is not a work order's middle turn: the session the last answer came back
  /// on handed over where bmo-agency's record of it still `stands`, and the rest a done job's session usage calls
  /// for.
  pub async fn hand_over(&mut self, reply: &Reply, outcome: &str, stands: bool, reason: &str) {
    if let (true, Some(held)) = (stands, reply.session_id.clone()) {
      self.hand_over_session(reply, &held, reason).await;
    }

    // The usage is the login's that answered, and a reply that names none rests nothing.
    if outcome == crate::queue::outcomes::JOB_DONE
      && let (Some(account), Some(usage)) = (&reply.account, &reply.session_usage)
      && let Err(why) = self.run.ground().calls.usage_rest(account, usage).await
    {
      eprintln!("bmo-job: the rest the usage calls for could not be begun: {}", why.error);
    }
  }

  // The session the job ran in handed over as the job ends, where nobody waits on it: closed with a note the next
  // run on the worker and model opens with — the worker's own, asked as one more turn of the session where its
  // folder holds a handoff instruction, or a summary out of the session's record.
  async fn hand_over_session(&mut self, reply: &Reply, held: &str, reason: &str) {
    let ground = self.run.ground();
    let worker = self.worker();
    let model = self.model.clone();
    let own = ground.calls.job_documents(&worker).await.ok().and_then(|documents| documents.handoff);

    println!(
      "{worker} session for {model}: {held} {reason}; handing it over through {}",
      if own.is_none() { "a summary" } else { "a note of its own" }
    );

    let note = match own {
      Some(instruction) => self.own_note(reply, &instruction, held).await,
      None => None,
    };
    let handover = Handover {
      model: model.clone(),
      cwd: self.road.cwd.clone(),
      thread: self.road.todo_id.map(|todo| format!("todo:{todo}")),
      session: Some(held.to_owned()),
      note,
    };

    // Upkeep after the work is done: what follows it does not hang on whether it worked.
    if let Err(why) = ground.calls.hand_over_session(&worker, &handover).await {
      eprintln!("bmo-job: {worker} session for {model}: the handover failed — {}", why.error);
    }
  }

  // The note the worker writes for itself, asked as one more turn of the session the job ran in and carried only
  // where that session answered it: none where the turn went elsewhere, said nothing or `none`, or failed.
  async fn own_note(&mut self, reply: &Reply, instruction: &str, held: &str) -> Option<String> {
    let resume = Resume::Session {
      id: held.to_owned(),
    };

    match self.recovering(instruction, &selection_of(reply), resume, None).await {
      Ok(answered) => own_note_of(answered.session_id.as_deref(), held, &answered.text),
      Err(halt) => {
        let why = match halt {
          Halt::Failed(failure) => failure.message(),
          Halt::Ended(ended) => ended.text,
        };

        eprintln!("bmo-job: the worker's own handoff note failed — {why}");
        None
      }
    }
  }

  // A stopped stage retried on a fresh session: the worker's kept session on the model it would resume handed over
  // through a summary and its record dropped, once, so the turn opens a new session with the note ahead of its
  // prompt.
  async fn hand_over_first(&mut self) {
    if self.handed_over {
      return;
    }
    self.handed_over = true;

    let ground = self.run.ground();
    let worker = self.worker();
    let handover = Handover {
      model: self.model.clone(),
      cwd: self.road.cwd.clone(),
      thread: self.road.todo_id.map(|todo| format!("todo:{todo}")),
      session: None,
      note: None,
    };

    println!(
      "{worker} session for {}: the kept session is handed over before a stopped stage is retried on a fresh session",
      self.model
    );

    if let Err(why) = ground.calls.hand_over_session(&worker, &handover).await {
      eprintln!(
        "bmo-job: {worker} session for {}: the handover before the retry failed — {}",
        self.model, why.error
      );
    }
  }
}

/// The note a handoff turn wrote, carried only where the session it was asked of answered it, and not blank or
/// `none`.
pub fn own_note_of(answered_on: Option<&str>, held: &str, text: &str) -> Option<String> {
  let note = if answered_on == Some(held) { text.trim() } else { "" };

  (!note.is_empty() && note.to_lowercase() != "none").then(|| note.to_owned())
}

/// The selection a later turn of the same work asks for: the road and model the last answer came back on.
pub fn selection_of(reply: &Reply) -> String {
  format!("{}::{}", reply.mode, reply.model.as_deref().unwrap_or("null"))
}

/// The session a later turn goes on: the Claude session the last answer named, a fresh one where it named none,
/// and on any other road whatever that road keeps.
pub fn resume_of(reply: &Reply) -> Resume {
  if reply.mode != PROVIDER {
    return Resume::Auto;
  }

  match &reply.session_id {
    Some(id) => Resume::Session { id: id.clone() },
    None => Resume::Fresh,
  }
}

/// The provider and model a selection names, as the facts of a run carry them; nothing for one that names no
/// provider.
pub fn selected(selection: &str) -> Option<Selection> {
  let (provider, model) = selection.split_once("::")?;

  (!provider.is_empty() && !model.is_empty()).then(|| Selection {
    provider: provider.to_owned(),
    model: model.to_owned(),
  })
}

// One run's attempts, each carried here on the road its selection names, with what the last one raised.
struct Steering<'s, 'd, 'g> {
  driver: &'s mut Driver<'d, 'g>,
  prompt: &'s str,
  resume: Resume,
  shaping: Option<Shaping>,
  raised: Option<Failure>,
  ended: Option<Halted>,
}

impl Attempts for Steering<'_, '_, '_> {
  async fn attempt(&mut self, asked: &AttemptAsked) -> Result<Attempted, Failure> {
    let driver = &mut *self.driver;
    let ephemeral = driver.run.args.ephemeral();
    let hands_over_first = driver.run.args.word == WORK_SESSION_HANDOVER;
    let provider = selected(&asked.selection).map(|chosen| chosen.provider).unwrap_or_default();
    let mut resolved = asked.resolved.clone();

    // A stopped stage's last retry runs the Claude road on the model it names, whatever the tier says.
    match driver.run.args.retry_model.clone().filter(|_| provider == PROVIDER) {
      Some(retry) => {
        driver.actual = Some(Selection {
          provider: PROVIDER.to_owned(),
          model: retry.clone(),
        });
        resolved.model = retry;
      }
      None => driver.actual = selected(&asked.selection),
    }

    driver.model.clone_from(&resolved.model);

    // A switch tells the turn the job's prompt went on elsewhere, ahead of a follow-up's own words; a format
    // turn's copy of the job's prompt is wanted only where no continuation put it there.
    let continuation = asked.continuation.as_str();
    let prompt = if !continuation.is_empty() && self.prompt != driver.given.prompt {
      format!("{}\n\n[Further request]\n{}{continuation}", driver.given.prompt, self.prompt)
    } else {
      format!("{}{continuation}", self.prompt)
    };
    let shaping = if continuation.is_empty() { self.shaping.clone() } else { None };

    // A co-work branch never lands on a single model, so its roads are Claude Code's, Codex's and OpenRouter's
    // alone.
    if ephemeral && ![PROVIDER, branch_run::CODEX, branch_run::OPENROUTER].contains(&provider.as_str()) {
      let failure = Failure::error(format!("a co-work branch does not land on {provider}"));
      let raised = Raised::of(&failure);

      self.raised = Some(failure);
      return Ok(Attempted::Raised(raised));
    }

    let road = if provider.is_empty() { PROVIDER.to_owned() } else { provider.clone() };

    // A stopped stage retried on a fresh session hands the worker's kept one over first, once.
    if road == PROVIDER && hands_over_first && !ephemeral {
      driver.hand_over_first().await;
    }

    let unit = &driver.run.args.unit;
    let name = if unit.is_empty() {
      format!("bmo-job-{}", driver.run.args.job_id)
    } else {
      unit.clone()
    };
    let turn = turn::Asked {
      provider: road.clone(),
      prompt,
      model: driver.model.clone(),
      resume: self.resume.clone(),
      attempt: TurnMapping {
        model: resolved.model.clone(),
        effort: resolved.effort.clone(),
        tier: Some(resolved.tier.clone()),
        answer_tokens: resolved.answer_tokens,
      },
      shaping,
      name,
      title: driver.given.title.clone(),
    };

    match carry_turn(driver.road, &turn).await {
      Ok(reply) => {
        // A worker's own Claude answer must name the session the next turn continues.
        if road == PROVIDER && !ephemeral && reply.session_id.is_none() {
          self.ended = Some(Halted {
            text: "The run left no session, so the job cannot go on. This job ends here, and no more results will come.".to_owned(),
            outcome: JOB_ERROR,
            gated: true,
          });
          return Err(Failure::error("the run named no session"));
        }

        Ok(Attempted::Answered(Box::new(reply)))
      }
      Err(failure) => {
        let raised = Raised::of(&failure);

        self.raised = Some(failure);
        Ok(Attempted::Raised(raised))
      }
    }
  }
}
