//! A job's own mark on a long wait, set before it goes quiet on purpose and cleared when the wait is over
//! (docs/verbs.md, "A job's mark on a long wait"). The sweep reads it before it calls the job quiet, and
//! the screens show it.
//!
//! The mark belongs to the job running the item named, so the verb is refused where none is. A target named
//! is asked once here as well, so a mark is never set on something already over. A command handed after
//! `--` is run with the mark standing on this verb's own process, which clears it the moment the command
//! ends and exits with the command's own code.

use std::path::Path;

use job_codes::job_wait::WaitTarget;
use job_codes::{JobError, JobWaitRefusal};
use queue_rows::todos::running_job_for;
use sea_orm::DatabaseConnection;

use super::stand::{Answer, Stand, VerbError, clock};
use crate::queue::settings;
use crate::queue::telling::{JobWait, WAIT_PREFIX, probe_target};

/// What the verb takes.
pub const USAGE: &str = "usage: job wait --todo <id> --what '<what the job waits on>' (--unit <unit> | --pid <pid>) --until <40m | 2h | 1h30m | 14:30> | --todo <id> --what '<…>' --until <…> -- <command…> | --todo <id> --clear";

const DAY_MS: i64 = 86_400_000;

/// The verb's words as the command line gave them, before they are read into a request.
#[derive(Debug, Default)]
pub struct WaitWords {
  pub todo: Option<String>,
  pub what: Option<String>,
  pub unit: Option<String>,
  pub pid: Option<String>,
  pub until: Option<String>,
  pub clear: bool,
  /// What followed `--`, where the line had one.
  pub command: Option<Vec<String>>,
}

/// What a call of the verb asks for: a mark, a mark around a command it runs, or the mark cleared.
#[derive(Debug, PartialEq)]
pub enum WaitRequest {
  Clear {
    todo_id: i64,
  },
  Mark {
    todo_id: i64,
    what: String,
    target: Option<WaitTarget>,
    until: i64,
    command: Vec<String>,
  },
}

/// Why the words do not read as a request: a fault of their form, or a mark missing a part it must have.
#[derive(Debug, PartialEq)]
pub enum WordsFault {
  Form(String),
  Mark(JobWaitRefusal),
}

// A run of decimal digits standing alone, or none.
fn digits(part: &str) -> Option<&str> {
  (!part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit())).then_some(part)
}

/// The end a job gives, read as a length from now (`40m`, `2h`, `1h30m`) or as a clock time in UTC today,
/// or tomorrow where that time has passed (`14:30`). Epoch milliseconds; none where the words are neither.
pub fn wait_end(text: &str, now: i64) -> Option<i64> {
  let text = text.trim();

  if let Some((hours, minutes)) = text.split_once(':') {
    let hours: i64 = digits(hours).filter(|hours| hours.len() <= 2)?.parse().ok()?;
    let minutes: i64 = digits(minutes).filter(|minutes| minutes.len() == 2)?.parse().ok()?;

    if hours > 23 || minutes > 59 {
      return None;
    }

    let today = now.div_euclid(DAY_MS) * DAY_MS + (hours * 60 + minutes) * 60_000;

    return Some(if today > now { today } else { today + DAY_MS });
  }

  let (hours, rest) = match text.split_once('h') {
    Some((hours, rest)) => (Some(digits(hours)?), rest),
    None => (None, text),
  };
  let minutes = match rest {
    "" => None,
    rest => Some(digits(rest.strip_suffix('m')?)?),
  };

  if hours.is_none() && minutes.is_none() {
    return None;
  }

  let minutes = hours.map_or(Some(0), |hours| hours.parse::<i64>().ok())? * 60
    + minutes.map_or(Some(0), |minutes| minutes.parse::<i64>().ok())?;

  (minutes > 0).then_some(now + minutes * 60_000)
}

fn form(why: &str) -> WordsFault {
  WordsFault::Form(why.to_owned())
}

/// The verb's words read, or why they cannot be. A command after `--` is its own target, so a unit or a
/// pid beside it is refused rather than silently set aside.
pub fn read_request(words: WaitWords, now: i64) -> Result<WaitRequest, WordsFault> {
  let todo_id = words
    .todo
    .as_deref()
    .and_then(|todo| todo.trim().parse::<i64>().ok())
    .filter(|id| *id > 0);
  let Some(todo_id) = todo_id else {
    return Err(form("the item is named by its number: --todo <id>."));
  };

  if words.clear {
    return Ok(WaitRequest::Clear { todo_id });
  }

  let what = words.what.as_deref().unwrap_or_default().trim().to_owned();

  if what.is_empty() {
    return Err(WordsFault::Mark(JobWaitRefusal::WaitNeedsWhat));
  }

  let Some(until) = words.until.as_deref().and_then(|until| wait_end(until, now)) else {
    return Err(form(
      "the expected end is a length or a clock time in UTC: --until 40m, 2h, 1h30m or 14:30.",
    ));
  };

  if let Some(command) = words.command {
    if command.is_empty() {
      return Err(form("nothing follows --: hand the command to run after it."));
    }

    if words.unit.is_some() || words.pid.is_some() {
      return Err(form("a command after -- is its own target: leave --unit and --pid out."));
    }

    return Ok(WaitRequest::Mark {
      todo_id,
      what,
      target: None,
      until,
      command,
    });
  }

  let target = match (words.unit, words.pid) {
    (Some(unit), None) if unit.trim().is_empty() => return Err(form("--unit takes a unit name.")),
    (Some(unit), None) => WaitTarget::Unit {
      name: unit.trim().to_owned(),
    },
    (None, Some(pid)) => match pid.trim().parse::<u32>() {
      Ok(pid) if pid > 0 => WaitTarget::Pid { pid },
      _ => return Err(form("--pid takes a process number.")),
    },
    _ => return Err(WordsFault::Mark(JobWaitRefusal::WaitNeedsTarget)),
  };

  Ok(WaitRequest::Mark {
    todo_id,
    what,
    target: Some(target),
    until,
    command: vec![],
  })
}

/// A refusal of the verb's words: a fault of their form with the usage under it and the code a usage fault
/// ends with, or the mark's own code.
pub fn refused_words(fault: WordsFault) -> VerbError {
  match fault {
    WordsFault::Form(why) => VerbError::Ended {
      code: 2,
      said: format!("wait: {why}\n{USAGE}"),
    },
    WordsFault::Mark(refusal) => VerbError::Coded(JobError::from(refusal).into()),
  }
}

/// A target as a line names it.
pub fn target_words(target: &WaitTarget) -> String {
  match target {
    WaitTarget::Unit { name } => format!("unit {name}"),
    WaitTarget::Pid { pid } => format!("process {pid}"),
  }
}

// The mark the job carries, where it has one.
async fn mark_of(db: &DatabaseConnection, job_id: i64) -> Result<Option<JobWait>, VerbError> {
  Ok(
    settings::get(db, &format!("{WAIT_PREFIX}{job_id}"))
      .await?
      .as_deref()
      .and_then(JobWait::read),
  )
}

/// The verb asked for one request at `now`: the mark cleared, or set, with the command after `--` run under
/// it. The line saying a mark is set is printed before the command starts, so the caller sees it while the
/// wait lasts.
pub async fn waiting(stand: &Stand, request: WaitRequest, now: i64) -> Result<Answer, VerbError> {
  let db = &stand.db;
  let todo_id = match &request {
    WaitRequest::Clear { todo_id } | WaitRequest::Mark { todo_id, .. } => *todo_id,
  };
  let job = running_job_for(db, todo_id).await?;
  let WaitRequest::Mark {
    what,
    target,
    until,
    command,
    ..
  } = request
  else {
    let Some(job_id) = job else {
      return Ok(vec![format!("wait: #{todo_id} carries no mark")].into());
    };

    if mark_of(db, job_id).await?.is_none() {
      return Ok(vec![format!("wait: #{todo_id} carries no mark")].into());
    }

    settings::forget(db, &format!("{WAIT_PREFIX}{job_id}")).await?;
    stand.moved("wait").await;

    return Ok(vec![format!("wait: #{todo_id}'s mark is cleared")].into());
  };
  let Some(job_id) = job else {
    return Err(VerbError::Ended {
      code: 1,
      said: format!("wait: #{todo_id} has no running job to carry the mark"),
    });
  };
  let systemctl: &Path = &stand.ground.systemctl;

  if let Some(given) = &target
    && probe_target(given, Some(systemctl)) == Some(false)
  {
    return Err(VerbError::Ended {
      code: 1,
      said: format!("wait: {} is not alive, so there is nothing to wait on", target_words(given)),
    });
  }

  let target = target.unwrap_or(WaitTarget::Pid {
    pid: std::process::id(),
  });
  let wait = JobWait {
    what: what.clone(),
    target,
    since: now,
    until,
    ended_at: None,
  };
  let key = format!("{WAIT_PREFIX}{job_id}");

  settings::set(db, &key, &serde_json::to_string(&wait).unwrap_or_default()).await?;
  stand.moved("wait").await;

  let marked = format!(
    "wait: #{todo_id} waits on {what} ({}) until {}",
    target_words(&wait.target),
    clock(wait.until)
  );

  if command.is_empty() {
    return Ok(vec![marked].into());
  }

  println!("{marked}");

  let code = run_under_mark(&command).await;

  // Only the mark this call set is taken away: one the job set since, for another wait, stands.
  if mark_of(db, job_id).await?.is_some_and(|standing| standing.since == wait.since) {
    settings::forget(db, &key).await?;
    stand.moved("wait").await;
  }

  Ok(Answer {
    lines: vec![format!("wait: #{todo_id} is done waiting on {what} (exit {code})")],
    code,
  })
}

// The command run with this process's own input and output, every signal meant for the wait passed to it;
// its exit code, a signal's death read as the shell reads it, and 127 where it could not start.
async fn run_under_mark(command: &[String]) -> u8 {
  use tokio::signal::unix::{SignalKind, signal};

  let mut child = match tokio::process::Command::new(&command[0]).args(&command[1..]).spawn() {
    Ok(child) => child,
    Err(why) => {
      eprintln!("wait: {} could not be started: {why}", command[0]);
      return 127;
    }
  };
  let pid = child.id().and_then(|pid| libc::pid_t::try_from(pid).ok());
  let streams = [SignalKind::interrupt(), SignalKind::terminate(), SignalKind::hangup()];
  let [mut interrupt, mut terminate, mut hangup] = streams.map(|kind| signal(kind).ok());
  let pass = |number: libc::c_int| {
    if let Some(pid) = pid {
      // SAFETY: a signal sent to the child this process started and still waits on.
      unsafe { libc::kill(pid, number) };
    }
  };

  loop {
    tokio::select! {
      status = child.wait() => {
        return match status {
          Ok(status) => std::os::unix::process::ExitStatusExt::signal(&status)
            .map_or_else(|| status.code().unwrap_or(1), |signal| 128 + signal) as u8,
          Err(_) => 1,
        };
      }
      () = next(&mut interrupt) => pass(libc::SIGINT),
      () = next(&mut terminate) => pass(libc::SIGTERM),
      () = next(&mut hangup) => pass(libc::SIGHUP),
    }
  }
}

// The next signal of one kind, or never where the kind could not be listened for.
async fn next(stream: &mut Option<tokio::signal::unix::Signal>) {
  match stream {
    Some(stream) => {
      stream.recv().await;
    }
    None => std::future::pending().await,
  }
}
