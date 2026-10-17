//! What the tests of a job's run stand on (docs/runs.md): the runner's ground over a scene's database and
//! units and the run's calls stood in, a host whose folders stand in a scratch directory, a git checkout with
//! a graph the stand-in names, and one job run whole as its unit runs it, from its words to its ending.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use bmo_job::job_run::{self, Ground, JobArgs};
use bmo_job::queue::job_start::Host;
use bmo_job::queue::telling::Teller;
use job_agency::{PoolRole, Tier};
use job_policy::Names;
use queue_rows::closing::Reader;
use tempfile::TempDir;

use super::queue::{OWNER, Scene};
use super::turns::{Agent, GIT_ENV, OPUS};

/// The worker every job runs on unless a test names another.
pub const WORKER: &str = "worker";

/// The unit every job runs under unless a test names another.
pub const UNIT: &str = "bmo-task-timed";

/// What a job is run on: its words as its unit passes them.
#[derive(Clone)]
pub struct Given {
  pub chat: i64,
  pub job: i64,
  pub model: String,
  pub unit: String,
  pub worker: String,
  pub cwd: PathBuf,
  pub task: String,
  pub phase: String,
  pub word: String,
  pub structure: String,
  pub final_baseline: String,
  pub retry_model: String,
}

/// What a run came to: its exit code and the ending its row holds.
#[derive(Debug)]
pub struct Ran {
  pub success: bool,
  pub outcome: Option<String>,
  pub ok: Option<i64>,
  pub report: String,
}

/// The runner over a scene of its own.
pub struct Bench {
  pub scene: Scene,
  pub scratch: TempDir,
  pub host: Host,
  pub agent: Agent,
  reader: Reader,
}

/// One git command in a directory, its output as text; a failed one fails the test.
pub fn git(dir: &Path, words: &[&str]) -> String {
  let ran = std::process::Command::new("git")
    .args(words)
    .current_dir(dir)
    .envs(GIT_ENV)
    .output()
    .expect("git runs");

  assert!(ran.status.success(), "git {words:?}: {}", String::from_utf8_lossy(&ran.stderr));
  String::from_utf8_lossy(&ran.stdout).trim_end().to_owned()
}

impl Bench {
  pub async fn new() -> Self {
    let scene = Scene::new().await;
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let folder = |name: &str| {
      let path = scratch.path().join(name);

      std::fs::create_dir_all(&path).expect("a scratch folder is made");
      path
    };
    let host = Host {
      program: PathBuf::from("/usr/bin/true"),
      cwd: folder("runner"),
      setenv: vec![],
      environment_file: None,
      locks: folder("locks"),
      prompts: folder("prompts"),
      env: BTreeMap::new(),
      systemd_run: PathBuf::from("/usr/bin/true"),
      lock_wait: Duration::from_secs(1),
    };
    let names = Names {
      owner: "Owner".into(),
      agent: "Agent".into(),
      sender: "Agent".into(),
    };
    let reader = Reader::new(&scene.policy.closing, &names);

    for name in ["documents", "tree", "source"] {
      folder(name);
    }

    Self {
      scene,
      scratch,
      host,
      agent: Agent::default(),
      reader,
    }
  }

  /// The co-worker pool's roster as bmo-agency answers it: a ladder of two tiers no co-worker is lent at, the
  /// second the one the policy runs a stage's builder at, and one a co-worker is; the builder at the first and
  /// a pooled scout at the last.
  pub fn roster(&mut self) {
    let tier = |name: &str, coworker: bool| Tier {
      name: name.into(),
      coworker,
    };
    let role = |name: &str, pooled: bool, tier: &str| PoolRole {
      name: name.into(),
      description: format!("the {name}"),
      enabled: true,
      pooled,
      tier: tier.into(),
    };

    self.scene.units.tiers = vec![
      tier("Frontier", false),
      tier(&self.scene.policy.queue.stage_builder_tier, false),
      tier("Fine", true),
    ];
    self.scene.units.roles = vec![role("builder", false, "Frontier"), role("scout", true, "Fine")];
  }

  /// A folder of the scratch directory.
  pub fn at(&self, name: &str) -> PathBuf {
    self.scratch.path().join(name)
  }

  /// The runner's ground: the scene's units, the stand-in calls, no systemd to ask.
  pub fn ground(&self) -> Ground<'_> {
    let scene = &self.scene;

    Ground {
      db: &scene.db,
      units: &scene.units,
      calls: &self.agent,
      policy: &scene.policy,
      reader: &self.reader,
      glossary: &[],
      teller: Teller {
        db: &scene.db,
        units: &scene.units,
        policy: &scene.policy,
        owner_chat: OWNER,
        source: self.at("source"),
        service_unit: "bmo-job".into(),
        systemctl: None,
        events: None,
      },
      host: &self.host,
      source: self.at("source"),
      tree: self.at("tree"),
      documents: self.at("documents"),
      testbed_logs: None,
      siren: None,
      owner_chat: OWNER,
      agent_key: "agent".into(),
      agent_name: "Agent".into(),
    }
  }

  /// A git checkout on `main` holding one file, `kept.ts`, and a graph over it in `graph/`, each its own
  /// commit.
  pub fn checkout(&self, name: &str) -> PathBuf {
    let dir = self.at(name);

    std::fs::create_dir_all(dir.join("graph")).expect("the checkout is made");
    git(&dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join("kept.ts"), "export const kept = 1;\n").unwrap();
    git(&dir, &["add", "kept.ts"]);
    git(&dir, &["commit", "-qm", "first"]);
    std::fs::write(
      dir.join("graph").join("bot.json"),
      "{\"nodes\":[{\"id\":\"bot/kept\",\"files\":[\"kept.ts\"]}]}\n",
    )
    .unwrap();
    git(&dir, &["add", "-A"]);
    git(&dir, &["commit", "-qm", "graph"]);
    dir
  }

  /// A worker of its own working in `cwd`, with so many support workers.
  pub async fn worker(&self, name: &str, cwd: &Path, support: i64) {
    self
      .scene
      .run(
        r#"INSERT INTO "workers" ("name", "label", "cwd", "support_count", "created_at") VALUES ($1, $1, $2, $3, 1)"#,
        vec![name.into(), cwd.display().to_string().into(), support.into()],
      )
      .await;
  }

  /// A support worker of `base` in the slot given, its worktree at `cwd`.
  pub async fn support_worker(&self, name: &str, cwd: &Path, base: &str, slot: i64) {
    self
      .scene
      .run(
        r#"INSERT INTO "workers" ("name", "label", "cwd", "support_of", "support_slot", "created_at")
           VALUES ($1, $1, $2, $3, $4, 1)"#,
        vec![name.into(), cwd.display().to_string().into(), base.into(), slot.into()],
      )
      .await;
  }

  /// One job of an item on a worker, launched a moment ago under the unit; its row's number.
  pub async fn job(&self, todo: Option<i64>, worker: &str, unit: &str) -> i64 {
    self.scene.job(todo, worker, unit, bmo_job::now_ms() - 1000).await
  }

  /// One job asked for in a room, of an item on a worker, launched a moment ago under the unit, on the model
  /// asked for; its row's number.
  pub async fn job_in(&self, chat: i64, todo: Option<i64>, unit: &str, worker: &str, model: &str) -> i64 {
    let row = job_base::one(
      &self.scene.db,
      r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "todo_id", "worker", "model")
         VALUES ($1, $2, $3, 'Do it.', $4, $5, $6) RETURNING "id""#,
      vec![
        chat.into(),
        unit.into(),
        (bmo_job::now_ms() - 1000).into(),
        todo.into(),
        worker.into(),
        model.into(),
      ],
    )
    .await
    .expect("the job is written")
    .expect("the job is numbered");

    row.try_get("", "id").expect("the job's number reads")
  }

  /// The words a job of this bench is run on: the Claude model, the worker and its checkout, no work order.
  pub fn given(&self, job: i64, cwd: &Path) -> Given {
    Given {
      chat: OWNER,
      job,
      model: OPUS.into(),
      unit: UNIT.into(),
      worker: WORKER.into(),
      cwd: cwd.to_path_buf(),
      task: "There is one thing to do in this repository. Report when it is done.".into(),
      phase: String::new(),
      word: String::new(),
      structure: String::new(),
      final_baseline: String::new(),
      retry_model: String::new(),
    }
  }

  /// The words of a work order's run on this bench: its phase and its session word.
  pub fn step(&self, job: i64, cwd: &Path, phase: &str, word: &str) -> Given {
    Given {
      phase: phase.into(),
      word: word.into(),
      ..self.given(job, cwd)
    }
  }

  /// The job run whole, as its unit runs it, and what it came to.
  pub async fn run(&self, given: &Given) -> Ran {
    let prompt = self.at(&format!("prompt-{}.txt", given.job));

    std::fs::write(&prompt, &given.task).expect("the task file is written");

    let words: Vec<String> = [
      given.chat.to_string(),
      prompt.display().to_string(),
      given.job.to_string(),
      given.model.clone(),
      given.unit.clone(),
      given.worker.clone(),
      given.cwd.display().to_string(),
      given.phase.clone(),
      given.word.clone(),
      given.structure.clone(),
      given.final_baseline.clone(),
      given.retry_model.clone(),
    ]
    .into();
    let args = JobArgs::read(&words).expect("the words name a job");
    let code = job_run::run(args, &self.ground()).await;

    self.ran(given.job, code).await
  }

  /// What the job's row holds of its ending, with the run's code.
  pub async fn ran(&self, job: i64, code: ExitCode) -> Ran {
    let row = job_base::one(
      &self.scene.db,
      r#"SELECT "outcome", "ok", "report" FROM "jobs" WHERE "id" = $1"#,
      vec![job.into()],
    )
    .await
    .expect("the job's row reads")
    .expect("the job has a row");

    Ran {
      success: code == ExitCode::SUCCESS,
      outcome: row.try_get("", "outcome").unwrap(),
      ok: row.try_get("", "ok").unwrap(),
      report: row.try_get::<Option<String>>("", "report").unwrap().unwrap_or_default(),
    }
  }

  /// Every word the run handed the agent to tell, each fact's texts one to a line.
  pub fn told(&self) -> String {
    let mut lines = vec![];

    for instruction in self.scene.units.instructions() {
      texts(&serde_json::to_value(&instruction.said).expect("what is said is JSON"), &mut lines);
    }

    lines.join("\n")
  }

  /// What reached one room, one entry to each instruction queued and each line voiced there, its texts one to a
  /// line.
  pub fn told_in(&self, chat: i64) -> Vec<String> {
    let joined = |value: serde_json::Value| {
      let mut lines = vec![];

      texts(&value, &mut lines);
      lines.join("\n")
    };
    let units = &self.scene.units;
    let queued = units
      .instructions()
      .into_iter()
      .filter(|instruction| instruction.chat == Some(chat))
      .map(|instruction| joined(serde_json::to_value(&instruction.said).expect("what is said is JSON")));
    let voiced = units
      .notices()
      .into_iter()
      .filter(|(room, _)| *room == chat)
      .map(|(_, notice)| joined(serde_json::to_value(&notice).expect("a notice is JSON")));

    queued.chain(voiced).collect()
  }

  /// The setting a key holds, where one does.
  pub async fn setting(&self, key: &str) -> Option<String> {
    bmo_job::queue::settings::get(&self.scene.db, key).await.expect("the settings read")
  }
}

impl Ran {
  /// Whether the run ended as `outcome` with its row closed `ok` and its code to match.
  pub fn ended(&self, outcome: &str, ok: bool) -> bool {
    self.outcome.as_deref() == Some(outcome) && self.ok == Some(i64::from(ok)) && self.success == ok
  }
}

// Every text a value holds, at any depth.
fn texts(value: &serde_json::Value, lines: &mut Vec<String>) {
  match value {
    serde_json::Value::String(text) => lines.push(text.clone()),
    serde_json::Value::Array(values) => values.iter().for_each(|value| texts(value, lines)),
    serde_json::Value::Object(fields) => fields.values().for_each(|value| texts(value, lines)),
    _ => {}
  }
}
