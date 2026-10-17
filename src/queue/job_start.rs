//! A job a launch has judged may go, started (docs/queue.md, "Starting a job"): its unit started by the
//! user's service manager in the first of its checkouts whose job lock is free, its row opened under that
//! lock, and the text it was launched with kept as its item's brief.
//!
//! The job's text is written to a file only the service's user can read and the unit is handed that
//! file's path, never the text: arguments stand in the process list for anyone on the machine to read.

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;

use job_codes::{JobError, TaskRefusal};
use sea_orm::ConnectionTrait;

use super::outcomes::JOB_ERROR;
use super::{jobs, launches};
use crate::{Refusal, now_ms, one, run};

/// The verb a job's unit runs on the runner's program.
pub const JOB_RUNNER_VERB: &str = "job-run";

/// The variable the user whose work a job is rides its unit in.
pub const TURN_USER_VARIABLE: &str = "BMO_TURN_USER";

// How long a checkout before the last is given its lock before the launch goes on to the next.
const NEXT_CHECKOUT_WAIT: Duration = Duration::from_secs(1);

// How often a lock that is held is tried again.
const LOCK_LOOK: Duration = Duration::from_millis(100);

// How much of a name a lock file keeps.
const SLUG_MOST: usize = 64;

/// Where and how a job's unit is started: the program whose `job-run` verb runs it and the directory it
/// runs in, the variables it is started with and the file of variables it reads, where the checkout locks
/// and the prompt files stand, the environment the user's service manager is reached on, the program that
/// asks it, and how long the last checkout's lock is waited for.
#[derive(Clone, Debug)]
pub struct Host {
  pub program: PathBuf,
  pub cwd: PathBuf,
  pub setenv: Vec<(String, String)>,
  pub environment_file: Option<PathBuf>,
  pub locks: PathBuf,
  pub prompts: PathBuf,
  pub env: BTreeMap<String, String>,
  pub systemd_run: PathBuf,
  pub lock_wait: Duration,
}

/// Whether what a program printed says it never reached the user's service manager — "Failed to connect
/// to" a bus, on one line — as against refusing what it was asked.
pub fn bus_unreachable(said: &str) -> bool {
  said.lines().any(|line| {
    let line = line.to_lowercase();

    line
      .find("failed to connect to ")
      .is_some_and(|at| line[at + "failed to connect to ".len()..].contains("bus"))
  })
}

/// One checkout a job may land in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
  pub name: String,
  pub cwd: String,
}

/// A job a launch has judged may go: what it runs, for whom, and the checkouts it may land in, in order.
#[derive(Clone, Debug)]
pub struct JobStart {
  pub chat_id: i64,
  pub prompt: String,
  pub model: Option<String>,
  pub started_by: Option<String>,
  pub todo_id: Option<i64>,
  pub purpose: Option<String>,
  /// The user whose work it is, riding the job's unit, or none for the platform's own.
  pub user: Option<String>,
  /// The model the item names, which a launch naming none runs on.
  pub item_model: Option<String>,
  pub places: Vec<Place>,
}

/// How a job's start came out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Started {
  Job {
    job_id: i64,
    unit: String,
    landed: Place,
  },
  /// Every checkout's job lock held, which the caller names the holders of.
  Held,
}

fn refused(refusal: TaskRefusal) -> Refusal {
  JobError::from(refusal).into()
}

/// The environment the user's service manager is reached on: the one given, the runtime directory named
/// where it is missing, and a bus a session disabled taken away.
pub fn bus_environment(mut env: BTreeMap<String, String>) -> BTreeMap<String, String> {
  if env.get("XDG_RUNTIME_DIR").is_none_or(String::is_empty)
    && let Ok(own) = std::fs::metadata("/proc/self")
  {
    env.insert("XDG_RUNTIME_DIR".into(), format!("/run/user/{}", own.uid()));
  }

  if env.get("DBUS_SESSION_BUS_ADDRESS").map(String::as_str) == Some("disabled:") {
    env.remove("DBUS_SESSION_BUS_ADDRESS");
  }

  env
}

/// Why the manager cannot be reached on that environment, or none where it can: a runtime directory
/// holding neither its private socket nor the user bus is a session that is not there.
pub fn bus_refusal(env: &BTreeMap<String, String>) -> Option<String> {
  let runtime = env.get("XDG_RUNTIME_DIR").map(String::as_str).unwrap_or_default();

  if runtime.is_empty() {
    return Some("no runtime directory is named".into());
  }

  let runtime = Path::new(runtime);

  if runtime.join("systemd").join("private").exists() || runtime.join("bus").exists() {
    return None;
  }

  Some(format!("the runtime directory {} holds no socket of the manager", runtime.display()))
}

// FNV-1a over the name: a digest that reads the same in every build, so two builds side by side during
// a deployment take the same lock for one checkout.
fn digest(name: &str) -> u64 {
  name.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
    (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
  })
}

/// The file a checkout's lock stands in: its path made safe for a file, cut from the front where it is
/// long — the tail names the checkout — with the digest of the whole path carrying what the cut threw
/// away.
pub fn lock_file_name(name: &str) -> String {
  let mut slug = String::new();
  let mut replacing = false;

  // Each run of characters a file name does not take becomes one dash.
  for character in name.chars() {
    let kept = character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-');

    if kept {
      slug.push(character);
    } else if !replacing {
      slug.push('-');
    }

    replacing = !kept;
  }

  let slug = slug.trim_matches('-');
  let kept = slug[slug.len().saturating_sub(SLUG_MOST)..].trim_start_matches('-');

  format!("{}-{:016x}.lock", if kept.is_empty() { "root" } else { kept }, digest(name))
}

/// A lock held for as long as this stands; a lock of a process that died is no lock at all.
pub struct Held(File);

impl Held {
  /// The file the lock is held on.
  pub fn file(&self) -> &File {
    &self.0
  }
}

fn try_hold(path: &Path) -> Option<Held> {
  if let Some(dir) = path.parent() {
    std::fs::create_dir_all(dir).ok()?;
  }

  let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).ok()?;

  file.try_lock().ok().map(|()| Held(file))
}

/// Whether a checkout's job lock is held by somebody now: a lock nobody holds is let go again at once, and
/// a lock file that is not there is held by nobody.
pub fn lock_held(host: &Host, cwd: &str) -> bool {
  let path = host.locks.join(lock_file_name(cwd));

  path.exists() && try_hold(&path).is_none()
}

/// The lock taken once whoever holds it lets go, waited for as long as given and no longer; none where it
/// stayed held or its file could not be opened.
pub async fn hold_within(path: &Path, wait: Duration) -> Option<Held> {
  let until = tokio::time::Instant::now() + wait;

  loop {
    if let Some(held) = try_hold(path) {
      return Some(held);
    }

    if tokio::time::Instant::now() >= until {
      return None;
    }

    tokio::time::sleep(LOCK_LOOK).await;
  }
}

struct Unit<'a> {
  name: &'a str,
  prompt_path: &'a Path,
}

/// What systemd-run is asked to start a job's unit on: the runner's `job-run` verb under the unit's name,
/// with the host's variables, the user whose work it is where one is, and the runner's arguments.
pub fn runner_command(host: &Host, unit: &str, user: Option<&str>, runner: Vec<String>) -> Vec<String> {
  let mut args = vec!["--user".to_owned(), "--quiet".into(), "--collect".into(), format!("--unit={unit}")];

  args.extend(host.setenv.iter().map(|(name, value)| format!("--setenv={name}={value}")));

  if let Some(user) = user {
    args.push(format!("--setenv={TURN_USER_VARIABLE}={user}"));
  }

  if let Some(file) = &host.environment_file {
    args.push(format!("--property=EnvironmentFile=-{}", file.display()));
  }

  args.extend([
    format!("--working-directory={}", host.cwd.display()),
    host.program.to_string_lossy().into_owned(),
    JOB_RUNNER_VERB.to_owned(),
    "--".to_owned(),
  ]);
  args.extend(runner);

  args
}

/// A unit started by systemd-run on the arguments given and the environment the manager is reached on:
/// whether it started, and what systemd-run said.
pub async fn start_unit(host: &Host, args: &[String]) -> (bool, String) {
  let output = tokio::process::Command::new(&host.systemd_run)
    .args(args)
    .env_clear()
    .envs(&host.env)
    .stdin(std::process::Stdio::null())
    .output()
    .await;

  match output {
    Ok(output) => (output.status.success(), String::from_utf8_lossy(&output.stderr).into_owned()),
    Err(why) => (false, why.to_string()),
  }
}

// The job started in one checkout under its job lock: its row, or none where the lock stayed held or a job
// borrowed the checkout meanwhile.
async fn launch_in(
  db: &impl ConnectionTrait,
  host: &Host,
  start: &JobStart,
  unit: &Unit<'_>,
  place: &Place,
  wait: Duration,
) -> Result<Option<i64>, Refusal> {
  let Some(_held) = hold_within(&host.locks.join(lock_file_name(&place.cwd)), wait).await else {
    return Ok(None);
  };

  // A job may have borrowed the checkout between the reading before and this lock; a borrowed checkout
  // takes no job, so the launch goes on to the next.
  if launches::leases(db).await?.iter().any(|lease| lease.worker == place.name) {
    return Ok(None);
  }

  let row = one(
    db,
    r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "model", "started_by", "todo_id", "worker", "purpose", "activity_version")
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 1) RETURNING "id""#,
    vec![
      start.chat_id.into(),
      unit.name.into(),
      now_ms().into(),
      start.prompt.as_str().into(),
      start.model.clone().into(),
      start.started_by.clone().into(),
      start.todo_id.into(),
      place.name.as_str().into(),
      start.purpose.clone().into(),
    ],
  )
  .await?
  .ok_or_else(|| Refusal::from(sea_orm::DbErr::RecordNotInserted))?;
  let job_id: i64 = row.try_get("", "id")?;
  let args = runner_command(
    host,
    unit.name,
    start.user.as_deref(),
    vec![
      start.chat_id.to_string(),
      unit.prompt_path.to_string_lossy().into_owned(),
      job_id.to_string(),
      // A nameless launch follows the enabled provider order and the job-worker route.
      start
        .model
        .clone()
        .or_else(|| start.item_model.clone())
        .unwrap_or_else(|| "auto".into()),
      unit.name.to_owned(),
      place.name.clone(),
      place.cwd.clone(),
    ],
  );
  let (ok, said) = start_unit(host, &args).await;

  if !ok && bus_unreachable(&said) {
    // A manager the check found and systemd still could not reach started nothing either, so the row is
    // taken back rather than closed as a run that died.
    run(
      db,
      r#"DELETE FROM "jobs" WHERE "id" = $1 AND "finished_at" IS NULL"#,
      vec![job_id.into()],
    )
    .await?;
    let _ = std::fs::remove_file(unit.prompt_path);

    return Err(refused(TaskRefusal::ManagerUnreachable {
      why: said.trim().to_owned(),
    }));
  }

  if !ok {
    jobs::finish(db, job_id, said.trim(), false, JOB_ERROR, now_ms()).await?;
    let _ = std::fs::remove_file(unit.prompt_path);

    return Err(refused(TaskRefusal::UnitNotStarted { job: job_id }));
  }

  // The prompt an item's job is launched with is the item's organized instruction from now on, which every
  // later reading for a worker reads.
  if let Some(todo) = start.todo_id {
    queue_rows::todos::set_brief(db, todo, start.prompt.trim()).await?;
  }

  Ok(Some(job_id))
}

// The stamp a task unit and its prompt file are named by: the launch's millisecond, moved on past the last
// one this process gave, so two launches in one millisecond never share a unit or a prompt file.
fn unit_stamp() -> i64 {
  static LAST: AtomicI64 = AtomicI64::new(0);

  let now = now_ms();
  let last = LAST
    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |last| Some(now.max(last + 1)))
    .unwrap_or(now);

  now.max(last + 1)
}

fn write_prompt(path: &Path, prompt: &str) -> std::io::Result<()> {
  use std::io::Write as _;

  if let Some(dir) = path.parent() {
    std::fs::create_dir_all(dir)?;
  }

  let mut file = OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)?;

  file.write_all(prompt.as_bytes())
}

/// The job's unit started in the first of its checkouts whose job lock is free, its row opened under that
/// lock. A checkout whose lock is held sends the launch on to the next, given a second; the last is given
/// the host's whole wait. Where the user's service manager is not there, the launch is refused before any
/// row is opened, since nothing would run.
pub async fn start(db: &impl ConnectionTrait, host: &Host, start: &JobStart) -> Result<Started, Refusal> {
  if let Some(why) = bus_refusal(&host.env) {
    return Err(refused(TaskRefusal::ManagerUnreachable { why }));
  }

  let name = format!("bmo-task-{}", unit_stamp());
  let prompt_path = host.prompts.join(format!("{name}.txt"));

  write_prompt(&prompt_path, &start.prompt).map_err(|why| {
    refused(TaskRefusal::PromptNotWritten {
      why: why.to_string(),
    })
  })?;

  let unit = Unit {
    name: &name,
    prompt_path: &prompt_path,
  };
  let last = start.places.len().saturating_sub(1);

  for (at, place) in start.places.iter().enumerate() {
    let wait = if at == last { host.lock_wait } else { NEXT_CHECKOUT_WAIT };

    if let Some(job_id) = launch_in(db, host, start, &unit, place, wait).await? {
      return Ok(Started::Job {
        job_id,
        unit: name.clone(),
        landed: place.clone(),
      });
    }
  }

  let _ = std::fs::remove_file(&prompt_path);

  Ok(Started::Held)
}
