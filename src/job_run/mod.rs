//! A job's run (docs/runs.md): the program a job's unit runs, whole — the locks, the job's checkout before and
//! after, its prompt, its turns (drive.rs), the ending judged and written, and the session handed over as it
//! ends — and the pieces it stands on: its checkout and what it holds open, a work order's shaping turn and
//! the check of its answer, the walk over the roads and what a failed run says of its road, and the words a
//! run's ending and its record carry. Every turn is carried by bmo-agency as a work turn.

pub mod branch_run;
pub mod calls;
pub mod checkout;
pub mod claude;
pub mod drive;
pub mod ending;
pub mod notes;
pub mod plan_check;
pub mod prompt;
pub mod recovery;
pub mod siren;
pub mod verdict;
pub mod worktree;

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use job_agency::{LiveTask, RunWorkflow, TurnSeat};
use job_codes::Fact;
use job_policy::Policy;
use job_receiver::{Instruction, Said};
use queue_read::lines::BUILTIN_WORKER;
use queue_rows::closing::Reader;
use queue_rows::wording::BannedTerm;
use sea_orm::DatabaseConnection;
use serde_json::Value;
use url::Url;

use self::calls::Calls;
use self::checkout::{Checkout, is_work_branch, item_branch, jobs_work_on_branches};
use self::claude::turn::{ClaudeRoad, Opening, PROVIDER};
use self::drive::{Driven, Driver, Given};
use self::ending::{Ending, RunFacts};
use self::prompt::{Asked, Branching, Trailer};
use self::recovery::{Walk, unit_step};
use self::worktree::{
  WorktreeState, clear_baseline, new_work, nothing_open, preserved_work, record_baseline,
};
use crate::order_run::hold::SETUP_RUNTIME;
use crate::queue::job_start::{Held, Host, hold, lock_file_name};
use crate::queue::outcomes::{
  JOB_DONE, JOB_ERROR, JOB_FAILED, JOB_LIVE_TASKS, JOB_UNCLOSED, JOB_WORK_STEP,
};
use crate::queue::telling::{JOB_SORT, Teller};
use crate::queue::units::Units;
use crate::{now_ms, one};

/// The session word a builder turn of a co-work is launched with: the worker's own session carried on.
pub const WORK_SESSION_RESUME: &str = "resume";

/// The session word a co-work run that throws its session away is launched with.
pub const WORK_SESSION_FRESH: &str = "fresh";

/// The session word a stopped stage's retry is launched with: the worker's kept session handed over first, so
/// the turn opens a fresh one.
pub const WORK_SESSION_HANDOVER: &str = "handover";

/// The phase a step of a work order runs in, before its final turn.
pub const WORK_STEP_PHASE: &str = "work-step";

/// The phase a work order's final turn runs in.
pub const WORK_FINAL_PHASE: &str = "work-final";

/// The label the line naming what a run left running opens with, which a reader looks for in a record.
pub const LIVE_LABEL: &str = "Left running";

/// The label the line naming the Claude Workflows a run walked away from opens with.
pub const ABANDONED_LABEL: &str = "Workflows not finished";

/// The variable every run a middle step of a work order makes tells the tools it calls that it closes no item.
pub const WORK_ORDER_STEP_VARIABLE: &str = "BMO_WORK_ORDER_STEP";

// The structures a work order's middle turn may be asked to answer in.
const WORK_STRUCTURES: [&str; 2] = ["plan", "synthesis"];

// How long the job's prompt waits for its gates, which a unit restarting for a deployment leaves unanswered for
// a while, and how often it asks again meanwhile.
const PROMPT_PATIENCE: Duration = Duration::from_secs(180);
const PROMPT_RETRY: Duration = Duration::from_secs(5);

// What a checkout on a node is reached over: one batch login, held open as one connection a run of commands
// shares and kept a minute past the last of them.
const SSH_BATCH: [&str; 4] = ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10"];

/// What the runner stands on: the database, the queue's calls on other units and the run's own, the policy and
/// the closing's patterns, the words the owner banned, the telling its ending goes through, the host its locks
/// stand on, the checkout the built-in worker works in, the platform's tree, where its documents and records
/// are written, the testbed's log folder, the cache siren, the owner's chat, the deployment's agent by its key
/// and its name, and the owner's name.
pub struct Ground<'a> {
  pub db: &'a DatabaseConnection,
  pub units: &'a dyn Units,
  pub calls: &'a dyn Calls,
  pub policy: &'a Policy,
  pub reader: &'a Reader,
  pub glossary: &'a [BannedTerm],
  pub teller: Teller<'a>,
  pub host: &'a Host,
  pub source: PathBuf,
  pub tree: PathBuf,
  pub documents: PathBuf,
  pub testbed_logs: Option<PathBuf>,
  pub siren: Option<Url>,
  pub owner_chat: i64,
  pub agent_key: String,
  pub agent_name: String,
  /// The deployment's owner, as the deployment names them.
  pub owner_name: String,
}

impl Ground<'_> {
  /// A fact told to the agent in the owner's chat; a failure to queue it is the receiver's, written to the
  /// service's log and never the run's.
  pub async fn tell(&self, fact: Fact) {
    let instruction = Instruction {
      agent: None,
      chat: Some(self.owner_chat),
      said: Said::Fact(fact),
      kind: None,
      sort: JOB_SORT.to_owned(),
      summary: None,
      expires_at: None,
      events: vec![],
      rides_until: None,
      via: None,
      run: None,
    };

    if let Err(failed) = self.units.queue_instruction(&instruction).await {
      eprintln!("bmo-job: a job's fact was not queued: {}", failed.error);
    }
  }

  /// The screens told the queue moved, so one showing it reads it again at once.
  pub async fn screens_moved(&self) {
    self.calls.queue_moved().await;
  }

  /// Where the runner keeps its records of a checkout: its baseline and what it landed.
  pub fn records(&self) -> &Path {
    &self.documents
  }
}

/// What the runner is handed, in the order the job's unit passes it: the chat, the prompt file, the job, the
/// model, the unit, the worker and its directory, and for a work order's run its phase, its session word, the
/// structure a middle turn answers in, the baseline a final turn is measured against and the model a stopped
/// stage's retry runs on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobArgs {
  pub chat_id: i64,
  pub prompt_path: String,
  pub job_id: i64,
  /// The selection the job asked for; `auto` where it named none.
  pub model: String,
  pub unit: String,
  pub worker: Option<String>,
  pub cwd: Option<String>,
  pub phase: String,
  pub word: String,
  structure: String,
  pub final_baseline: String,
  pub retry_model: Option<String>,
}

impl JobArgs {
  /// The arguments read, or none where the chat, the prompt file or the job is missing.
  pub fn read(given: &[String]) -> Option<Self> {
    let at = |index: usize| given.get(index).cloned().unwrap_or_default();
    let chat_id = at(0).parse().ok()?;
    let job_id = at(2).parse().ok()?;
    let prompt_path = at(1);

    if prompt_path.is_empty() {
      return None;
    }

    Some(Self {
      chat_id,
      prompt_path,
      job_id,
      model: given.get(3).cloned().unwrap_or_else(|| "auto".to_owned()),
      unit: at(4),
      worker: given.get(5).cloned(),
      cwd: given.get(6).cloned(),
      phase: at(7),
      word: at(8),
      structure: at(9),
      final_baseline: at(10),
      retry_model: given.get(11).filter(|model| !model.is_empty()).cloned(),
    })
  }

  /// Whether the run is a step of a work order.
  pub fn managed(&self) -> bool {
    self.phase == WORK_STEP_PHASE || self.phase == WORK_FINAL_PHASE
  }

  /// The structure a work order's middle turn answers in, which its shaping turns ask for: a plan, or the
  /// tasks a synthesis builds; none for any other run.
  pub fn work_output(&self) -> Option<&str> {
    (self.phase == WORK_STEP_PHASE && WORK_STRUCTURES.contains(&self.structure.as_str()))
      .then_some(self.structure.as_str())
  }

  /// What every run the job makes tells the tools it calls of a work order's middle step, which closes no item:
  /// the variable and its value in such a step, and none in any other.
  pub fn work_step_var(&self) -> (&'static str, Option<&'static str>) {
    (WORK_ORDER_STEP_VARIABLE, (self.phase == WORK_STEP_PHASE).then_some("1"))
  }

  /// Whether the run opens a session it throws away: a co-work branch in a place of its own, which drives no
  /// pane of the worker's.
  pub fn ephemeral(&self) -> bool {
    self.managed() && self.word == WORK_SESSION_FRESH
  }
}

// One worker's row as the runner reads it.
struct WorkerRow {
  cwd: Option<String>,
  support_of: Option<String>,
  support_count: i64,
  node: Option<String>,
}

async fn worker_row(ground: &Ground<'_>, name: &str) -> Option<WorkerRow> {
  let row = one(
    ground.db,
    r#"SELECT "cwd", "support_of", "support_count", "node" FROM "workers" WHERE "name" = $1"#,
    vec![name.into()],
  )
  .await
  .ok()??;

  Some(WorkerRow {
    cwd: row.try_get("", "cwd").ok()?,
    support_of: row.try_get("", "support_of").ok()?,
    support_count: row.try_get("", "support_count").ok()?,
    node: row.try_get("", "node").ok()?,
  })
}

/// The login a checkout on a node is reached through: the batch login, held open as one connection a run of
/// commands shares, kept a minute past the last of them. None where bmo-agency names no reach for the node.
pub async fn node_login(ground: &Ground<'_>, node: &str) -> Option<Vec<String>> {
  let reach = ground.calls.ssh_reach(node).await.ok()??;
  let mut args: Vec<String> = SSH_BATCH.iter().map(|word| (*word).to_owned()).collect();

  args.extend(["-p".to_owned(), reach.port.to_string()]);

  if let Some(key) = &reach.key_path {
    args.extend(["-i".to_owned(), key.clone()]);
  }

  if let Some(runtime) = std::env::var("XDG_RUNTIME_DIR").ok().filter(|dir| !dir.is_empty()) {
    args.extend([
      "-o".to_owned(),
      "ControlMaster=auto".to_owned(),
      "-o".to_owned(),
      format!("ControlPath={runtime}/bmo-job-ssh-%C"),
      "-o".to_owned(),
      "ControlPersist=60".to_owned(),
    ]);
  }

  args.push(format!("{}@{}", reach.user, reach.host));
  Some(args)
}

/// One job, run from its locks to its handoff.
pub(crate) struct Run<'a> {
  args: JobArgs,
  ending: Ending<'a>,
  worker: String,
  todo_id: Option<i64>,
  checkout: Checkout,
  node: Option<String>,
  base_on_branch: bool,
  branch_main: Option<String>,
  job_branch: Option<String>,
  // Whether the worker's directory is a worktree of its own: a support worker's, or a linked one.
  own_worktree: bool,
  start: Option<WorktreeState>,
  start_head: Option<String>,
  own_files: Option<Vec<String>>,
  main_restored: bool,
  borrowed_let_go: bool,
  // The item the job still left open after it was asked to close it.
  left_open_item: Option<i64>,
  // What its report still did not answer for the graph its commits reached after it was asked, and those files.
  left_impact: (verdict::ImpactFaults, Vec<String>),
}

impl<'a> Run<'a> {
  fn ground(&self) -> &'a Ground<'a> {
    self.ending.ground
  }

  // Where a middle turn's refused shapes are kept: under the item's own folder of the documents, or the job's.
  fn output_folder(&self) -> PathBuf {
    let item = self.todo_id.map_or_else(|| "jobs".to_owned(), |todo| todo.to_string());

    self.ground().documents.join(item).join(format!("work-output-{}", self.args.job_id))
  }

  // Every file this job's own commits touched, each path once, read once.
  fn own_commit_files(&mut self) -> Vec<String> {
    if let Some(files) = &self.own_files {
      return files.clone();
    }

    let records = self.ground().records();
    let files = match (&self.branch_main, &self.start_head) {
      // On a branch the landing rebases it onto a main other jobs have moved, so what is the job's own is what it
      // landed and what is still on its branch past main.
      (Some(main), _) => self.checkout.commit_files(&self.checkout.own_branch_commits(records, main)),
      (None, Some(head)) => {
        let mut seen = HashSet::new();

        self
          .checkout
          .commits_between(head)
          .unwrap_or_default()
          .into_iter()
          .flat_map(|commit| commit.files)
          .filter(|file| seen.insert(file.clone()))
          .collect()
      }
      (None, None) => Vec::new(),
    };

    self.own_files = Some(files.clone());
    files
  }

  // What the graph in the job's checkout names and where it stands; nothing where the graph cannot be read, since
  // the job's work is done and its report is worth more than the questions this answers.
  async fn graph_named(&self) -> (HashSet<String>, String) {
    let calls = self.ground().calls;
    let place = match &self.node {
      Some(node) => calls.node_copy(node, &self.checkout.cwd).await.unwrap_or_else(|why| {
        eprintln!("bmo-job: the copy of the node's checkout could not be read: {}", why.error);
        self.checkout.cwd.clone()
      }),
      None => self.checkout.cwd.clone(),
    };

    match calls.graph_named(&place).await {
      Ok(named) => (named.files.into_iter().collect(), named.directory),
      Err(why) => {
        eprintln!("bmo-job: the code dependency graph could not be read: {}", why.error);
        (HashSet::new(), String::new())
      }
    }
  }

  // Which of the job's own commit files the graph names while the graph itself was left untouched.
  async fn unrecorded_by_graph(&mut self) -> Vec<String> {
    let (named, directory) = self.graph_named().await;

    if named.is_empty() {
      return Vec::new();
    }

    verdict::unrecorded_change(&self.own_commit_files(), &named, &directory)
  }

  // Which of them the graph names, the change the report answers for node by node.
  async fn graph_mapped(&mut self) -> Vec<String> {
    let (named, _) = self.graph_named().await;

    verdict::graph_mapped(&self.own_commit_files(), &named)
  }

  fn current_state(&self) -> WorktreeState {
    worktree::worktree_state(&self.checkout).unwrap_or_else(|why| {
      eprintln!("bmo-job: {}: {why}", self.checkout.cwd);
      nothing_open()
    })
  }

  // What of this job's own leftovers is an unclosed change to a file git already tracks.
  fn tracked_changes(&self) -> Vec<String> {
    let start = self.start.clone().unwrap_or_else(nothing_open);

    new_work(&start, &self.current_state()).tracked
  }

  fn uncommitted_note(&self) -> String {
    let start = self.start.clone().unwrap_or_else(nothing_open);
    let current = self.current_state();

    verdict::uncommitted_note(&new_work(&start, &current).all(), &preserved_work(&start, &current).all())
  }

  // The units linked into a checkout on this machine relinked through bmo-testbed once it stands on main.
  async fn relink(&self, cwd: &str) {
    if let Err(why) = self.ground().calls.relink_units(Path::new(cwd)).await {
      eprintln!("bmo-job: the units linked into {cwd} were not relinked: {}", why.error);
    }
  }

  // The checkouts this job borrowed let go as it ends, and what it left open in a borrowed support worktree
  // parked on that worktree's branch. Answers what the report says of it; asked once.
  async fn let_borrowed_go(&mut self) -> String {
    if self.borrowed_let_go {
      return String::new();
    }
    self.borrowed_let_go = true;

    let ground = self.ground();
    let job_id = self.args.job_id;
    let mut lines: Vec<String> = Vec::new();
    let names = match self.ending.release_leases().await {
      Ok(names) => names,
      Err(why) => {
        eprintln!("bmo-job: the borrowed checkouts could not be let go: {}", why.error);
        Vec::new()
      }
    };

    for name in names {
      let Some(lent) = worker_row(ground, &name).await else { continue };
      let Some(cwd) = lent.cwd.clone() else { continue };
      let place = Checkout::local(&cwd);
      // A worker of its own in a linked worktree is let go as a support worker's worktree is.
      let base = lent.support_of.is_none() && place.main_checkout().is_none();
      let message =
        format!("Job {job_id} parks on this branch the change it left uncommitted in the borrowed {name}");

      if base && is_work_branch(place.current_branch().as_deref()) {
        let main = place.main_branch();
        let state = match worktree::worktree_state(&place) {
          Ok(state) => state,
          Err(why) => {
            eprintln!("bmo-job: the borrowed checkouts could not be let go: {why}");
            break;
          }
        };
        let own = new_work(&worktree::recorded_baseline(ground.records(), &cwd), &state).all();
        let back = place.restore_main(&main, &own, &message);

        clear_baseline(ground.records(), &cwd);

        if back.on_main {
          self.relink(&cwd).await;
        }

        if let Some(parked) = &back.parked {
          lines.push(format!(
            "{} uncommitted change(s) left in the borrowed {name} were parked on the branch {} as commit {}.",
            parked.paths.len(),
            parked.branch,
            &parked.commit[..parked.commit.len().min(12)]
          ));
        }

        if !back.on_main {
          lines.push(format!(
            "The borrowed {name} could not be put back on {main}: {}",
            back.said.as_deref().unwrap_or("the cause could not be read")
          ));
        }

        continue;
      }

      if base {
        let open = place.open_paths();

        if !open.is_empty() {
          lines.push(format!(
            "Uncommitted changes are left in the borrowed checkout {cwd}: {}",
            verdict::change_names(&open, 8)
          ));
        }

        continue;
      }

      match place.park_open(&message) {
        Ok(Some(parked)) => lines.push(format!(
          "{} uncommitted change(s) left in the borrowed {name} were parked on the branch {} as commit {}.",
          parked.paths.len(),
          parked.branch,
          &parked.commit[..parked.commit.len().min(12)]
        )),
        Ok(None) => {}
        Err(why) => {
          eprintln!("bmo-job: the borrowed checkouts could not be let go: {why}");
          break;
        }
      }
    }

    if lines.is_empty() { String::new() } else { format!("\n\n{}", lines.join("\n")) }
  }

  // A base checkout on the job's branch put back on main as the job ends, before its report goes out: what the
  // job left open goes onto its branch, what was open before it began is carried back as it stands. Answers
  // what the report says of it; asked once.
  async fn back_on_main(&mut self) -> String {
    let (Some(main), true, false) = (self.branch_main.clone(), self.base_on_branch, self.main_restored)
    else {
      return String::new();
    };
    self.main_restored = true;

    // What the job committed is read off its branch past main, which the switch takes out of view.
    self.own_commit_files();

    let job_branch = self.job_branch.clone().unwrap_or_default();
    let own = match &self.start {
      None => Vec::new(),
      Some(start) => match worktree::worktree_state(&self.checkout) {
        Ok(now) => new_work(start, &now).all(),
        Err(why) => {
          eprintln!("bmo-job: the base checkout could not be put back on main: {why}");
          return format!("\n\nThe base checkout could not be put back on {main}: {why}");
        }
      },
    };
    let back = self.checkout.restore_main(
      &main,
      &own,
      &format!("Job {} parks on {job_branch} the change it left uncommitted", self.args.job_id),
    );

    if back.on_main && !self.checkout.is_remote() {
      self.relink(&self.checkout.cwd.clone()).await;
    }

    let parked = back.parked.as_ref().map_or_else(String::new, |parked| {
      format!(
        "{} uncommitted change(s) were parked on the branch {} as commit {}. ",
        parked.paths.len(),
        parked.branch,
        &parked.commit[..parked.commit.len().min(12)]
      )
    });

    println!(
      "{}: {}{}{}",
      self.worker,
      if back.on_main {
        format!("back on {main}")
      } else {
        format!("still on {job_branch}: {}", back.said.as_deref().unwrap_or_default())
      },
      back.parked.as_ref().map_or_else(String::new, |parked| format!(
        ", parked {} open file(s) as {}",
        parked.paths.len(),
        &parked.commit[..parked.commit.len().min(12)]
      )),
      back
        .deleted
        .as_ref()
        .map_or_else(String::new, |deleted| format!(", deleted the landed {deleted}"))
    );

    if !back.on_main {
      return format!(
        "\n\n{parked}The base checkout could not be put back on {main} and stands on {job_branch}: {}",
        back.said.as_deref().unwrap_or("the cause could not be read")
      );
    }

    if parked.is_empty() {
      String::new()
    } else {
      format!("\n\n{parked}The base checkout is back on {main}.")
    }
  }

  // The ending written: the checkout let go and put back first, so the report says what became of it and every
  // session stands on main again the moment the job is over.
  async fn report(&mut self, text: &str, outcome: &str, gated: bool, facts: &RunFacts) {
    let lent = self.let_borrowed_go().await;
    let back = self.back_on_main().await;

    self.ending.write(&format!("{text}{lent}{back}"), outcome, gated, facts).await;
  }

  // An ending before the job reached its first turn, its checkout already prepared: the report says why, and
  // the ending is written as any run's is.
  async fn end_before(&mut self, text: &str) -> ExitCode {
    eprintln!("{text}");

    let now = now_ms();
    let facts = RunFacts {
      requested: self.args.model.clone(),
      actual: None,
      run_started_at: now,
      run_ended_at: now,
      ..RunFacts::default()
    };

    self.report(text, JOB_ERROR, true, &facts).await;
    self.ending.hand_off(JOB_ERROR).await;
    ExitCode::from(1)
  }

  // An ending before the work began: the row closed and the queue told, nothing else written.
  async fn refuse(&self, text: &str) -> ExitCode {
    eprintln!("{text}");

    if let Err(why) =
      self.ground().teller.close_job(self.args.job_id, text, false, JOB_ERROR, now_ms()).await
    {
      eprintln!("bmo-job: job {}'s ending could not be written: {}", self.args.job_id, why.error);
    }

    self.ending.tell_finished(JOB_ERROR).await;
    ExitCode::from(1)
  }
}

/// The job run where the runner holds its locks: everything from the worker's checkout to the handoff.
pub async fn run(args: JobArgs, ground: &Ground<'_>) -> ExitCode {
  let started_at = now_ms();
  let worker = args.worker.clone().unwrap_or_else(|| BUILTIN_WORKER.to_owned());
  let cwd = args.cwd.clone().unwrap_or_else(|| ground.source.to_string_lossy().into_owned());
  let ending = Ending {
    ground,
    job_id: args.job_id,
    worker: worker.clone(),
    phase: args.phase.clone(),
    ephemeral: args.ephemeral(),
    started_at,
  };
  let row = worker_row(ground, &worker).await;
  let node = row.as_ref().and_then(|row| row.node.clone());
  let todo_id = one_todo(ground, args.job_id).await;
  let mut run = Run {
    ending,
    worker: worker.clone(),
    todo_id,
    checkout: Checkout::local(&cwd),
    node: node.clone(),
    base_on_branch: false,
    branch_main: None,
    job_branch: None,
    own_worktree: false,
    start: None,
    start_head: None,
    own_files: None,
    main_restored: false,
    borrowed_let_go: false,
    left_open_item: None,
    left_impact: Default::default(),
    args,
  };

  // A worker placed on a work node has its checkout there, every git question going to the node over one shared
  // login.
  if let Some(node) = &node {
    match node_login(ground, node).await {
      Some(login) => run.checkout = Checkout::on_node(&cwd, login),
      None => {
        return run
          .refuse(&format!(
            "The job did not start: there is no SSH road to the node {node} that {worker} stands on."
          ))
          .await;
      }
    }
  }

  let support_base = match row.as_ref().and_then(|row| row.support_of.clone()) {
    Some(base) => worker_row(ground, &base).await,
    None => None,
  };
  let base_cwd =
    |base: &WorkerRow| base.cwd.clone().unwrap_or_else(|| ground.source.to_string_lossy().into_owned());
  let linked_main = if support_base.is_none() && row.is_some() && run.checkout.is_checkout() {
    run.checkout.main_checkout()
  } else {
    None
  };
  let own_worktree = support_base.is_some() || linked_main.is_some();

  run.own_worktree = own_worktree;

  let base_branches = !own_worktree
    && row
      .as_ref()
      .is_some_and(|row| jobs_work_on_branches(row.support_of.as_deref(), row.support_count))
    && run.checkout.is_checkout();

  run.base_on_branch = base_branches && !run.args.managed();
  run.branch_main = match (&support_base, &linked_main) {
    (Some(base), _) => Some(Checkout::local(base_cwd(base)).main_branch()),
    (None, Some(main)) => Some(Checkout::local(main).main_branch()),
    (None, None) if base_branches => Some(run.checkout.main_branch()),
    _ => None,
  };
  run.job_branch = run.branch_main.as_ref().map(|_| item_branch(todo_id, run.args.job_id));

  // The job's branch prepared before anything reads the checkout: made from main where it is not there yet,
  // what the last job left open parked on the branch that job was on, and this job's item branch taken up or
  // made.
  if let (Some(branch), Some(main)) = (run.job_branch.clone(), run.branch_main.clone()) {
    let message = format!(
      "Job {} parks on its branch the change the job before it left uncommitted in {worker}, on its way to {branch}",
      run.args.job_id
    );
    let prepared = (|| {
      if node.is_some() {
        run.checkout.follow_remote_main(&main)?;
      } else if let Some(base) = &support_base {
        Checkout::local(base_cwd(base)).ensure_support_checkout(&cwd, SETUP_RUNTIME)?;
      }

      run.checkout.prepare_branch(&branch, &main, &message, SETUP_RUNTIME)
    })();

    match prepared {
      Ok(prepared) => {
        checkout::clear_landed(ground.records(), &cwd);
        println!(
          "{worker}: on {branch}{}{}{}",
          if prepared.made { format!(" (made from {main})") } else { String::new() },
          prepared.parked.as_ref().map_or_else(String::new, |parked| format!(
            ", parked {} open file(s) on {} as {}",
            parked.paths.len(),
            parked.branch,
            &parked.commit[..parked.commit.len().min(12)]
          )),
          prepared
            .deleted
            .as_ref()
            .map_or_else(String::new, |deleted| format!(", deleted the landed {deleted}"))
        );
      }
      Err(why) => {
        let place = if support_base.is_some() {
          format!("the worktree of the support worker {worker}")
        } else if linked_main.is_some() {
          format!("{worker}'s own worktree")
        } else {
          format!("{worker}'s base checkout")
        };

        return run
          .refuse(&format!(
            "The job did not start: {place} could not be put on the job's branch {branch}: {why}"
          ))
          .await;
      }
    }
  }

  // Where the checkout stood before this job touched anything: a final turn's own, carried across its order;
  // nothing open in a worktree of the worker's own, which is this job's alone; the checkout as it stands
  // otherwise. Left where a deployment the job runs reads it.
  let start = if run.args.phase == WORK_FINAL_PHASE && !run.args.final_baseline.is_empty() {
    std::fs::read_to_string(&run.args.final_baseline)
      .ok()
      .and_then(|text| serde_json::from_str::<Value>(&text).ok())
      .map_or_else(nothing_open, |value| worktree::read_worktree_state(&value))
  } else if own_worktree {
    nothing_open()
  } else {
    match worktree::worktree_state(&run.checkout) {
      Ok(state) => state,
      Err(why) => {
        eprintln!("{why}");
        run.back_on_main().await;
        return ExitCode::from(1);
      }
    }
  };

  record_baseline(ground.records(), &cwd, &start, now_ms());
  run.start = Some(start);
  run.start_head = run.checkout.head_commit();

  let code = carry(&mut run).await;

  clear_baseline(ground.records(), &cwd);
  code
}

// The job's item, read off its row.
async fn one_todo(ground: &Ground<'_>, job_id: i64) -> Option<i64> {
  one(ground.db, r#"SELECT "todo_id" FROM "jobs" WHERE "id" = $1"#, vec![job_id.into()])
    .await
    .ok()
    .flatten()
    .and_then(|row| row.try_get::<Option<i64>>("", "todo_id").ok().flatten())
}

// The road of a job: on the worker's own session, or for a co-work branch on a session it throws away, in a
// standing branch place of its repository labelled with the item and the role it runs for.
async fn claude_road<'a>(run: &Run<'a>, walk: &Walk<'_>) -> Result<ClaudeRoad<'a>, String> {
  let ground = run.ground();
  let cwd = run.args.cwd.clone().unwrap_or_else(|| ground.source.to_string_lossy().into_owned());
  let branch = if run.args.ephemeral() {
    let role = match walk.managed_task().await {
      Ok(task) => task.and_then(|task| task.get("role").and_then(Value::as_str).map(str::to_owned)),
      Err(_) => None,
    };

    Some(TurnSeat::Branch {
      todo: walk.todo_id,
      role,
    })
  } else {
    None
  };
  let opened = ClaudeRoad::open(Opening {
    ground,
    job_id: run.args.job_id,
    chat_id: run.args.chat_id,
    unit: run.args.unit.clone(),
    worker: run.worker.clone(),
    cwd,
    managed: run.args.managed(),
    branch,
  })
  .await;

  opened.map_err(|why| why.error)
}

// The walk each run of the job goes through: the job, its item and its worker.
fn walking<'g>(run: &Run<'g>) -> Walk<'g> {
  Walk {
    ground: run.ground(),
    job_id: run.args.job_id,
    worker: run.worker.clone(),
    unit: run.args.unit.clone(),
    todo_id: run.todo_id,
    managed: run.args.managed(),
    ephemeral: run.args.ephemeral(),
  }
}

// The trailer the job's text, and every later turn's, is followed by.
fn trailer_of(args: &JobArgs) -> Trailer {
  if args.phase == WORK_STEP_PHASE { Trailer::WorkStep } else { Trailer::Job }
}

// The header over the job's turns, read by a person watching them: the item by its number and its title as the
// worker reads it, or the job's own words where no item stands behind it, and for a step of a work order which
// step.
async fn turn_title(run: &Run<'_>, task: &str) -> String {
  let item = match run.todo_id {
    Some(todo) => prompt::item_heading(run.ground(), todo).await,
    None => None,
  };
  let step = if run.args.managed() { unit_step(&run.args.unit) } else { None };

  format!(
    "{}{}",
    item.unwrap_or_else(|| task.to_owned()),
    step.map_or_else(String::new, |step| format!(" · {step}"))
  )
}

// The job's own text as it was filed, taken off its file; none where it is missing or blank.
fn take_task(path: &str) -> Option<String> {
  let task = std::fs::read_to_string(path).ok();
  let _ = std::fs::remove_file(path);

  task.filter(|task| !task.trim().is_empty())
}

// The worker's bench made ready by bmo-agency. A co-work branch stands in no row and is handed no bench, since
// its name lives one task.
async fn bench(run: &Run<'_>) {
  if run.args.ephemeral() {
    return;
  }

  if let Err(why) = run.ground().calls.make_bench(&run.worker).await {
    eprintln!("bmo-job: {}: its bench could not be made ready: {}", run.worker, why.error);
  }
}

// The job's prompt, its gates asked again while they do not answer, up to the patience a restart is given; the
// last failure's words where it never came. The text the worker reads for the job's own text is kept on its row
// as its English once it is had.
async fn compose_prompt(
  run: &Run<'_>,
  task: &str,
  knowledge: Option<&str>,
  impact: Option<&str>,
) -> Result<String, String> {
  let ground = run.ground();
  let asked =
    Asked {
      ground,
      worker: &run.worker,
      chat_id: run.args.chat_id,
      task,
      knowledge,
      impact,
      trailer: trailer_of(&run.args),
      branching: run.job_branch.as_deref().zip(run.branch_main.as_deref()).map(|(branch, main)| {
        Branching {
          branch,
          main,
          own_worktree: run.own_worktree,
        }
      }),
      remote: run.checkout.is_remote(),
    };
  let by = tokio::time::Instant::now() + PROMPT_PATIENCE;

  loop {
    match prompt::compose(&asked).await {
      Ok(composed) => {
        if composed.defused {
          println!("worker prompt: an order to attack a real target was defused before the run");
        }

        prompt::keep_english(ground, run.args.job_id, &composed.english).await;
        return Ok(composed.prompt);
      }
      Err(why) if tokio::time::Instant::now() + PROMPT_RETRY < by => {
        eprintln!("bmo-job: the job's prompt is asked again: {}", why.error);
        tokio::time::sleep(PROMPT_RETRY).await;
      }
      Err(why) => return Err(why.error),
    }
  }
}

// The job's place and prompt made ready and its road opened, then its turns driven; an ending with the reason
// where the job never reached its first turn.
async fn carry(run: &mut Run<'_>) -> ExitCode {
  let ground = run.ground();

  // A missing or blank task is a run that never reached the work: the row is left to the dead-job sweep, as a
  // runner that died leaves it.
  let Some(task) = take_task(&run.args.prompt_path) else {
    eprintln!("the task file is empty");
    run.back_on_main().await;
    return ExitCode::from(1);
  };

  bench(run).await;

  let walk = walking(run);
  let worker = run.worker.clone();
  let mut road = match claude_road(run, &walk).await {
    Ok(road) => road,
    Err(why) => {
      return run
        .end_before(&format!("The job did not start: no turn of it could be opened for {worker}: {why}"))
        .await;
    }
  };
  // The knowledge the job's words call for is picked once, ahead of the gates the prompt may ask again, and the
  // job's log names the pieces attached.
  let item = prompt::job_item(ground, run.todo_id).await;
  let knowledge = prompt::job_knowledge(ground, run.args.chat_id, item.as_ref(), &task).await;
  // The impact grounds are the job's own to weigh again; a branch is handed its task alone.
  let impact = item
    .as_ref()
    .and_then(|item| item.impact.as_ref())
    .filter(|_| !run.args.ephemeral())
    .map(|grounds| prompt::impact_block(ground.policy, grounds));

  if !knowledge.paths.is_empty() {
    road
      .note(&format!("knowledge attached: {}", knowledge.paths.join(", ")), None, None)
      .await;
  }

  let composed = match compose_prompt(run, &task, knowledge.block.as_deref(), impact.as_deref()).await {
    Ok(prompt) => prompt::trailer(ground, &run.worker, trailer_of(&run.args))
      .await
      .map(|trailer| (prompt, trailer))
      .map_err(|why| why.error),
    Err(why) => Err(why),
  };
  let (prompt, trailer) = match composed {
    Ok(composed) => composed,
    Err(why) => {
      return run
        .end_before(&format!("The job did not start: its prompt could not be made: {why}"))
        .await;
    }
  };

  // The item's texts are put into English ahead of the header that names it and of the job's first look at its
  // item.
  if let Some(todo) = run.todo_id {
    prompt::keep_item_english(ground, todo).await;
  }

  let given = Given {
    title: turn_title(run, &task).await,
    prompt,
    trailer,
  };

  driven(run, &mut road, &walk, given).await
}

// The job's turns driven here, and the ending they call for: the session the last answer came back on recorded,
// the answer judged and the ending written, the session handed over with the rest a done job's usage calls for,
// and the queue told.
async fn driven<'g>(
  run: &mut Run<'g>,
  road: &mut ClaudeRoad<'g>,
  walk: &Walk<'g>,
  given: Given,
) -> ExitCode {
  let mut driver = Driver::new(run, road, walk, given);
  let reply = match driver.drive().await {
    Driven::Answered(reply) => *reply,
    Driven::Halted(halted) => {
      let facts = driver.facts();

      driver.run.report(&halted.text, halted.outcome, halted.gated, &facts).await;
      driver.run.ending.hand_off(halted.outcome).await;
      return ExitCode::from(1);
    }
  };
  let step = driver.run.args.phase == WORK_STEP_PHASE;
  let ephemeral = driver.run.args.ephemeral();

  // A middle turn lets the tasks it left running past, and says so in its log.
  if step && !reply.live_tasks.is_empty() {
    let names = verdict::live_task_names(&reply.live_tasks, 4);

    eprintln!("work step: left running, and let past: {names}");
    driver
      .note(&format!("The step ended with background tasks left running: {names}"))
      .await;
  }

  // Why the session is handed over is read before the checkout goes back on main.
  let hands_over = !step && !ephemeral && reply.mode == PROVIDER && reply.session_id.is_some();
  // bmo-agency recorded the session the reply came back on as the turn answered, or dropped it where it fell to a
  // lesser model; only a session its record still continues for the job's model is handed over.
  let stands = hands_over && driver.road.session_stands(&reply, driver.model()).await;
  let reason = if hands_over { driver.handover_reason().await } else { "ended its job".to_owned() };
  let facts = driver.facts();
  let (judged, outcome, gated) = driver.judge(&reply).await;

  driver.run.report(&judged, outcome, gated, &facts).await;

  // A work order's middle turn hands nothing over and is told nothing.
  if !step {
    driver.hand_over(&reply, outcome, stands, &reason).await;
    driver.run.ending.hand_off(outcome).await;
  }

  if outcome == JOB_DONE || outcome == JOB_WORK_STEP {
    ExitCode::SUCCESS
  } else {
    ExitCode::from(1)
  }
}

// The run's last reply judged: a work order's middle turn by whether it produced its result, and any other job
// by whether it reported at all, left work running, walked away from a workflow or left its change open, or left
// the graph behind. Answers the report, the outcome and whether the report passes the gate.
async fn judge(
  run: &mut Run<'_>,
  text: &str,
  live_tasks: &[LiveTask],
  workflows: &[RunWorkflow],
) -> (String, &'static str, bool) {
  let ground = run.ground();
  let said = || ["", "What the job left:", text.trim()].map(str::to_owned);

  if run.args.phase == WORK_STEP_PHASE {
    let nothing = verdict::nothing_reported(text);
    let ok = !nothing && verdict::abandoned_workflows(workflows).is_empty();

    if ok {
      return (text.trim().to_owned(), JOB_WORK_STEP, false);
    }

    let failure = if nothing {
      format!("The step produced no result. What the run answered: {}", verdict::answered_with(text))
    } else {
      std::iter::once("The step ended without closing its work.".to_owned())
        .chain(verdict::unfinished_lines(live_tasks, workflows))
        .chain(said())
        .collect::<Vec<_>>()
        .join("\n")
    };

    return (failure, JOB_FAILED, false);
  }

  if verdict::nothing_reported(text) {
    let report = if text.trim().is_empty() {
      format!(
        "The job ended with no report while doing what it was asked. What the run left does not tell why, and the whole of job {}'s run record is kept (the console's job screen, `bmo room jobs`).",
        run.args.job_id
      )
    } else {
      format!(
        "The job ended without doing any work. All the run put down was a room's silence marker, which is no report: {}",
        verdict::answered_with(text)
      )
    };

    return (report, JOB_FAILED, true);
  }

  if !live_tasks.is_empty() {
    let report = std::iter::once(
      "The job ended while its work was still running. Those tasks ended with the job, and no more results will come.".to_owned(),
    )
    .chain(verdict::unfinished_lines(live_tasks, &[]))
    .chain(said())
    .collect::<Vec<_>>()
    .join("\n");

    return (report, JOB_LIVE_TASKS, true);
  }

  let unclosed =
    "The job ended without closing its work. This job ends here, and no more results will come.";
  let abandoned = verdict::abandoned_workflows(workflows);
  let left_behind = run.tracked_changes();

  if !abandoned.is_empty() || !left_behind.is_empty() {
    let report = std::iter::once(unclosed.to_owned())
      .chain(verdict::unfinished_lines(&[], workflows))
      .chain(
        (!left_behind.is_empty())
          .then(|| format!("Uncommitted changes are left: {}", verdict::change_names(&left_behind, 8))),
      )
      .chain(said())
      .collect::<Vec<_>>()
      .join("\n");

    return (report, JOB_UNCLOSED, true);
  }

  let unrecorded = run.unrecorded_by_graph().await;

  if !unrecorded.is_empty() {
    let report = [
      unclosed.to_owned(),
      format!(
        "It changed files the graph names and left the graph as it was: {}",
        verdict::change_names(&unrecorded, 8)
      ),
    ]
    .into_iter()
    .chain(said())
    .collect::<Vec<_>>()
    .join("\n");

    return (report, JOB_UNCLOSED, true);
  }

  let policy = ground.policy;
  let body = verdict::report_body(text);
  let own_files = run.own_commit_files();

  (
    format!(
      "{body}{}{}{}{}{}{}{}",
      verdict::shape_note(&body),
      verdict::sweep_note(&own_files, &body, &policy.closing.words.swept),
      verdict::unchecked_note(policy, ground.reader, &body),
      verdict::wording_note(&body, ground.glossary),
      run.uncommitted_note(),
      verdict::open_item_note(run.left_open_item),
      verdict::impact_note(policy, run.left_impact.0, &run.left_impact.1)
    ),
    JOB_DONE,
    true,
  )
}

/// The runner's locks, taken as a job takes them before it does anything: the checkout's job lock and the job
/// hold that says a job is at work there, each in the host's lock folder, each waited for whoever holds it.
pub async fn hold_locks(host: &Host, cwd: &str) -> Vec<Held> {
  vec![
    hold(&host.locks.join(lock_file_name(cwd))).await,
    hold(&host.locks.join(lock_file_name(&format!("job-hold:{cwd}")))).await,
  ]
}
