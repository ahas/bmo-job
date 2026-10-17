//! The job's tool hook: `job guard`, with the hook's input on standard input (docs/verbs.md, "The shell
//! hook").
//!
//! The worker's settings run it before every shell command and every file edit a job makes. A command that
//! writes into the agent program's session records — every login's, as bmo-agency names their homes — starts
//! the program itself or forks a session is refused with the reason, unless the job has opened the guard for
//! itself by saying what for; a command let through that way is recorded against the opening. A command that
//! starts the program in print mode is refused whatever the job has opened. A command or an edit that changes
//! the structure document is refused unless the job carries the owner's own request naming it. A removal the
//! program would stop on for a confirmation is refused with the way to write it, and nothing opens that. A
//! job's first write into the brain holds the brain's editors off through bmo-brain, and a write while an
//! editor holds its lock is refused with the way to wait for it. A command that writes a file of the checkout
//! with an inline script, an in-place editor or a redirect is refused with the verb that does it. A command or
//! an edit that writes into another registered worker's checkout is refused with the verb that borrows one. A
//! shell habit that stalls a job is refused with the way to write it, and so is a heavy test or build command
//! started outside the testbed. A commit message, a tag or a branch name holding Hangul where the command line
//! shows it is refused. A shell command let through goes to the tool behind a line closing its standard input;
//! a file edit let through is answered with nothing.

pub mod git_language;
pub mod habits;
pub mod reading;
pub mod records;
pub mod removal;
pub mod shell;
pub mod testbed;
pub mod writes;

use job_agency::Agency;
use job_base::Settings;
use job_brain::Brain;
use job_policy::Policy;
use job_receiver::Receiver;
use job_testbed::Testbed;
use queue_read::lines::BUILTIN_WORKER;
use sea_orm::DatabaseConnection;
use serde_json::{Map, Value, json};

use self::git_language::{EnglishTest, git_language_refusal};
use self::habits::{ShellCall, closed_stdin_answer, habit_refusal};
use self::records::{CliRecords, command_refusal, print_mode_refusal};
use self::removal::removal_refusal;
use self::testbed::testbed_refusal;
use self::writes::{
  ForeignCheckout, STRUCTURE_DOCUMENT, brain_command_write, brain_file_write, checkout_write_refusal,
  foreign_checkout_message, foreign_command_write, foreign_file_write, structure_command_refusal,
  structure_file_refusal,
};
use super::stand::{Answer, JobProcess, running_job};
use crate::queue::ground::QueueGround;
use crate::{all, now_ms, one, run};

/// What a hook is handed before a tool call: the tool, the command of a shell call with the wait it asked
/// for and whether it runs in the background, or the file a file tool writes, and the tool's input as it was
/// handed, which an answer that changes the call gives back whole.
#[derive(Clone, Debug, PartialEq)]
pub struct HookInput {
  pub tool: String,
  pub command: Option<String>,
  pub path: Option<String>,
  pub call: ShellCall,
  pub given: Map<String, Value>,
}

/// The hook's input read, or why it could not be.
pub fn hook_input(text: &str) -> Result<HookInput, String> {
  let parsed: Value = serde_json::from_str(text).map_err(|why| why.to_string())?;
  let given = parsed.get("tool_input").and_then(Value::as_object).cloned().unwrap_or_default();
  let text_of = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_owned);

  Ok(HookInput {
    tool: text_of(parsed.get("tool_name")).unwrap_or_else(|| "Bash".into()),
    command: text_of(given.get("command")),
    path: text_of(given.get("file_path")).or_else(|| text_of(given.get("notebook_path"))),
    call: ShellCall {
      timeout_ms: given.get("timeout").and_then(Value::as_u64),
      background: given.get("run_in_background") == Some(&Value::Bool(true)),
    },
    given,
  })
}

// The file tools that write.
const FILE_EDITORS: [&str; 4] = ["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// The answer refusing a call, with the reason the job reads.
pub fn hook_refusal(reason: &str) -> String {
  json!({
    "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason },
  })
  .to_string()
}

/// What a refusal of a command the live-Claude guard has not been opened for says, `cli` the platform's command
/// line that opens it.
pub fn live_claude_refusal(refused: &str, cli: &str) -> String {
  format!(
    "refused before this job has said what for: {refused}, which writes a whole prefix to the cache again. To open it for this job: {cli} job live-claude --purpose '<what you measure and why>'. Nobody is asked; every run it lets through is recorded and the count goes into the item's report (docs/verbs.md, \"The live-Claude guard\")."
  )
}

/// What a refusal of a change to the structure document says.
pub fn structure_refusal_message(reason: &str) -> String {
  format!(
    "refused: {reason}. {STRUCTURE_DOCUMENT} is the owner's statement of the system, and only a job carrying his own request that names it may change it. Where the code differs from it, report the difference instead."
  )
}

/// What a refusal of a brain write while an editor holds its lock says.
pub fn brain_lock_refusal(write: &str, taken: &[String]) -> String {
  format!(
    "refused for now: {write}, and a brain editor holds its editing lock ({}). This job's brain hold is set, so no new editor starts; wait for the running one to finish, then run this again (docs/verbs.md, \"The shell hook\").",
    taken.join(", ")
  )
}

/// Which registered workers' checkouts one job may not write in: every worker's checkout but its own worker's
/// and the ones it has borrowed. The job's own is none where the job or its worker cannot be read.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobCheckouts {
  pub own: Option<String>,
  pub foreign: Vec<ForeignCheckout>,
}

fn resolved(path: &str) -> String {
  let trimmed = path.trim_end_matches('/');

  if trimmed.is_empty() { "/".into() } else { trimmed.into() }
}

/// The checkouts of the job running as `run`, as the rows hold them.
pub async fn job_checkouts(db: &DatabaseConnection, run_name: &str) -> JobCheckouts {
  checkouts_of(db, run_name).await.unwrap_or_default()
}

async fn checkouts_of(db: &DatabaseConnection, run_name: &str) -> Option<JobCheckouts> {
  let job = running_job(db, run_name).await.ok()??;
  let worker = job.worker.unwrap_or_else(|| BUILTIN_WORKER.into());
  let rows = all(
    db,
    r#"SELECT "name", "cwd", "support_of" FROM "workers" WHERE "cwd" IS NOT NULL"#,
    vec![],
  )
  .await
  .ok()?;
  let mut workers = vec![];

  for row in rows {
    let name: String = row.try_get("", "name").ok()?;
    let cwd: String = row.try_get("", "cwd").ok()?;
    let support_of: Option<String> = row.try_get("", "support_of").ok()?;

    workers.push((name, resolved(&cwd), support_of));
  }

  let (own_name, own_cwd, _) = workers.iter().find(|(name, _, _)| *name == worker)?.clone();
  let lent: Vec<String> =
    all(db, r#"SELECT "worker" FROM "checkout_leases" WHERE "job_id" = $1"#, vec![job.id.into()])
      .await
      .ok()?
      .iter()
      .filter_map(|row| row.try_get::<String>("", "worker").ok())
      .collect();

  Some(JobCheckouts {
    foreign: workers
      .into_iter()
      .filter(|(name, cwd, _)| *name != own_name && !lent.contains(name) && *cwd != own_cwd)
      .map(|(name, cwd, support_of)| ForeignCheckout {
        base: support_of.unwrap_or_else(|| name.clone()),
        worker: name,
        cwd,
      })
      .collect(),
    own: Some(own_cwd),
  })
}

/// Who may change the structure document: a job whose item the owner himself requested and whose own words
/// name the document.
pub async fn structure_edit_approved(db: &DatabaseConnection, run_name: &str, owner: i64) -> bool {
  let items = all(
    db,
    r#"SELECT t."title", t."instruction" FROM "jobs" j JOIN "todos" t ON t."id" = j."todo_id"
       WHERE j."unit" = $1 AND t."requester_id" = $2"#,
    vec![run_name.into(), owner.into()],
  )
  .await
  .unwrap_or_default();

  // The words are searched here rather than in the statement, so every backend reads them alike.
  items.iter().any(|item| {
    ["title", "instruction"].iter().any(|part| {
      item
        .try_get::<Option<String>>("", part)
        .ok()
        .flatten()
        .is_some_and(|words| words.contains(STRUCTURE_DOCUMENT))
    })
  })
}

async fn live_claude_opening(db: &DatabaseConnection, run_name: &str) -> Option<i64> {
  one(
    db,
    r#"SELECT o."id" FROM "live_claude_openings" o JOIN "jobs" j ON j."id" = o."job_id"
       WHERE j."unit" = $1 ORDER BY o."id" DESC LIMIT 1"#,
    vec![run_name.into()],
  )
  .await
  .ok()
  .flatten()
  .and_then(|row| row.try_get::<i64>("", "id").ok())
}

// The database the hook asks, opened once and only where a reading needs it. A database it cannot reach is
// one it can say nothing about: every lookup then answers that nothing is known.
struct Rows<'a> {
  settings: &'a Settings,
  opened: Option<Option<DatabaseConnection>>,
}

impl Rows<'_> {
  async fn db(&mut self) -> Option<&DatabaseConnection> {
    if self.opened.is_none() {
      self.opened = Some(self.settings.database.url.connect(1).await.ok());
    }

    self.opened.as_ref().and_then(Option::as_ref)
  }
}

/// Where the hook runs: the settings it builds what it asks from, the job's markers, the home its paths are
/// spelled under, and the directory it runs in.
pub struct Hooked<'a> {
  pub settings: &'a Settings,
  pub process: &'a JobProcess,
  pub home: &'a str,
  pub here: &'a str,
}

/// The hook run on its input: the answer it prints, or nothing for a call it lets through as it was.
pub async fn hook(hooked: &Hooked<'_>, text: &str) -> Answer {
  match judged(hooked, text).await {
    Ok(answer) => answer.into_iter().collect::<Vec<_>>().into(),
    Err(why) => {
      // A hook that cannot read its input or its settings stops nothing: every shell command of every job
      // would stand behind it otherwise.
      eprintln!("job-guard: {why}");

      Vec::<String>::new().into()
    }
  }
}

async fn judged(hooked: &Hooked<'_>, text: &str) -> Result<Option<String>, String> {
  let input = hook_input(text)?;
  let ground = QueueGround::of_process(hooked.settings).map_err(|why| why.to_string())?;
  let policy = Policy::load(hooked.settings).map_err(|why| why.to_string())?;
  let english = EnglishTest::of(&policy);
  let command = (input.tool == "Bash").then(|| input.command.clone()).flatten();
  let home = hooked.home;
  let mut rows = Rows {
    settings: hooked.settings,
    opened: None,
  };
  let job = hooked.process.in_job;
  let run_name = hooked.process.run_name.clone();
  let cli = ground.tree.join("bin").join("bmo").to_string_lossy().into_owned();

  // Every login's records are the program's records, and which logins there are is bmo-agency's; the home's
  // own spelling stands beside them whatever it says.
  let refused = match &command {
    Some(command) => {
      let homes = ground.agency.claude_homes().await.unwrap_or_default();

      command_refusal(command, &CliRecords::of(&homes, home))
    }
    None => None,
  };
  // What no opening lets through: the program in print mode, a removal it would stop on, a habit that stalls
  // the job, and Hangul going into git.
  let unsafe_call = command.as_deref().and_then(|command| {
    print_mode_refusal(command)
      .or_else(|| removal_refusal(command))
      .or_else(|| habit_refusal(command, input.call))
      .or_else(|| git_language_refusal(command, &english))
  });
  let heavy = match command.as_deref().filter(|_| unsafe_call.is_none()) {
    Some(command) => ground
      .testbed
      .classify(command)
      .await
      .ok()
      .and_then(|classified| testbed_refusal(&classified)),
    None => None,
  };
  // Whose checkouts these are: the job's own, which the shell writes into through the verbs, and every other
  // registered worker's, which it does not write into at all.
  let checkouts = match (&run_name, job) {
    (Some(run_name), true) => match rows.db().await {
      Some(db) => job_checkouts(db, run_name).await,
      None => JobCheckouts::default(),
    },
    _ => JobCheckouts::default(),
  };
  let root = checkouts.own.clone().unwrap_or_else(|| hooked.here.to_owned());
  let shell_write = command.as_deref().and_then(|command| checkout_write_refusal(command, &root, home));
  let file_edit = FILE_EDITORS.contains(&input.tool.as_str());
  let foreign = match (&command, &input.path) {
    (Some(command), _) => foreign_command_write(command, &checkouts.foreign, home),
    (None, Some(path)) if file_edit => foreign_file_write(path, &checkouts.foreign)
      .map(|checkout| (checkout, "it edits a file there".to_owned())),
    _ => None,
  };
  let structure = match (&command, &input.path) {
    (Some(command), _) => structure_command_refusal(command),
    (None, Some(path)) if input.tool != "Bash" => structure_file_refusal(path),
    _ => None,
  };
  // The opening is read only where there is a job to have made one.
  let opening = match (&refused, &unsafe_call, job, &run_name) {
    (Some(_), None, true, Some(run_name)) => match rows.db().await {
      Some(db) => live_claude_opening(db, run_name).await,
      None => None,
    },
    _ => None,
  };

  if let Some(unsafe_call) = unsafe_call {
    return Ok(Some(hook_refusal(&unsafe_call)));
  }

  if let Some(heavy) = heavy {
    return Ok(Some(hook_refusal(&heavy)));
  }

  if let Some(refused) = refused.as_deref().filter(|_| opening.is_none()) {
    return Ok(Some(hook_refusal(&live_claude_refusal(refused, &cli))));
  }

  if let Some(structure) = structure {
    let owner = ground.receiver.owner().await.ok().map(|owner| owner.sender);
    let approved = match (rows.db().await, run_name.as_deref(), owner) {
      (Some(db), Some(run_name), Some(owner)) => structure_edit_approved(db, run_name, owner).await,
      _ => false,
    };

    if !approved {
      return Ok(Some(hook_refusal(&structure_refusal_message(&structure))));
    }
  }

  if let Some((checkout, why)) = foreign {
    return Ok(Some(hook_refusal(&foreign_checkout_message(checkout, &why))));
  }

  if let Some(shell_write) = shell_write {
    return Ok(Some(hook_refusal(&shell_write)));
  }

  if let (Some(opening), Some(command)) = (opening, &command)
    && let Some(db) = rows.db().await
  {
    let what = format!("shell: {}", command.chars().take(200).collect::<String>());

    let _ = run(
      db,
      r#"INSERT INTO "live_claude_runs" ("opening_id", "what", "ran_at") VALUES ($1, $2, $3)"#,
      vec![opening.into(), what.into(), now_ms().into()],
    )
    .await;
  }

  let brain = ground
    .brain_root
    .as_ref()
    .map(|root| root.to_string_lossy().into_owned())
    .unwrap_or_default();
  let brain_write = match (&command, &input.path) {
    (Some(command), _) => brain_command_write(command, &brain, home),
    (None, Some(path)) if file_edit && brain_file_write(path, &brain) => Some("it edits the brain".into()),
    _ => None,
  };

  // The hold is set before the locks are looked at, so an editor beginning after this look waits for the job,
  // and one already writing is the only thing the job has to wait for.
  if let (Some(brain_write), true, Some(run_name)) = (&brain_write, job, &run_name)
    && let Ok(hold) = ground.brain.hold(run_name).await
    && hold.held
    && !hold.taken.is_empty()
  {
    return Ok(Some(hook_refusal(&brain_lock_refusal(brain_write, &hold.taken))));
  }

  // A command let through runs with the shell's standard input closed.
  Ok(command.map(|command| closed_stdin_answer(&input.given, &command)))
}
