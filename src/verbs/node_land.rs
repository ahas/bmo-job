//! The operations machine's side of a landing from a checkout on a work node (docs/verbs.md, "Landing a
//! branch"). It takes the repository's landing lock — the one every landing of the repository takes — and
//! says `held`, then waits for the node's word: `landed <commit>` once the node has pushed main, or
//! `nothing`. Landed, the base checkout here follows the remote's main, set up again where what its setup is
//! made from moved, and with `--record` the commit is recorded as waiting to go live through bmo-testbed.

use std::io::{BufRead, Write};

use job_testbed::{PendingDeploy, Testbed};

use super::stand::{Answer, RunningJob, Stand, VerbError};
use crate::job_run::checkout::Checkout;
use crate::job_run::checkout::landing::LAND_LOCK_WAIT;
use crate::order_run::hold::SETUP_RUNTIME;
use crate::queue::job_start::{hold_within, lock_file_name};
use crate::queue::workers::workers;

// The files a checkout's setup is made from: a package's manifest and its lockfile, and the submodules'.
const SETUP_INPUTS: [&str; 5] = ["package.json", "bun.lock", "Cargo.toml", "Cargo.lock", ".gitmodules"];

/// Whether a checkout whose files moved over these paths needs its setup run again.
pub fn setup_inputs_moved(paths: &[String]) -> bool {
  paths.iter().any(|path| SETUP_INPUTS.contains(&path.rsplit('/').next().unwrap_or(path)))
}

// Git in the base checkout: what it printed where it worked.
fn git(cwd: &str, args: &[&str]) -> Option<String> {
  let argv: Vec<&str> = std::iter::once("git").chain(args.iter().copied()).collect();
  let ran = Checkout::local(cwd).run(&argv, &[], None);

  (ran.code == Some(0)).then(|| String::from_utf8_lossy(&ran.stdout).into_owned())
}

/// A commit landed in the checkout at `cwd` recorded as waiting to go live through bmo-testbed, its job's item
/// and worker beside it, the worker given standing in where the job names none; the line that says so.
pub async fn record_pending(
  stand: &Stand,
  cwd: &str,
  commit: &str,
  job: Option<&RunningJob>,
  worker: Option<&str>,
  reason: &str,
) -> String {
  let said = &stand.policy.close.close;
  let pending = PendingDeploy {
    checkout: cwd.into(),
    commit: commit.to_owned(),
    item: job.and_then(|job| job.todo_id),
    worker: job.and_then(|job| job.worker.clone()).or_else(|| worker.map(str::to_owned)),
    job: job.map(|job| job.id),
    reason: reason.trim().to_owned(),
  };

  match stand.ground.testbed.record_pending(&pending).await {
    Err(why) => {
      eprintln!("node-land: {why}");
      said.unrecorded.clone()
    }
    Ok(recorded) => {
      let item = recorded.item.map_or_else(|| "no item".to_owned(), |item| format!("#{item}"));
      let worker = recorded.worker.map(|worker| format!(" · {worker}")).unwrap_or_default();
      let urgent = if recorded.urgent { " · urgent" } else { "" };
      let words = format!(
        "{} ({item}{worker}{urgent}) — {}",
        recorded.commit.chars().take(12).collect::<String>(),
        recorded.reason
      );

      stand.policy.fill(&said.recorded, &[("words", &words)])
    }
  }
}

// The node's word, the first line of the standing input.
fn word() -> String {
  let mut line = String::new();
  let _ = std::io::stdin().lock().read_line(&mut line);

  line.trim().to_owned()
}

/// One landing held for the node checkout of the worker named, the commit recorded for `record` where it is
/// given.
pub async fn node_land(stand: &Stand, worker: &str, record: Option<&str>) -> Result<Answer, VerbError> {
  let refused = |said: String| VerbError::Ended { code: 2, said };
  let known = workers(&stand.db, &stand.ground.source).await?;
  let Some(named) = known.iter().find(|one| one.name == worker) else {
    return Err(refused(format!("node-land: {worker} is no worker to land for")));
  };
  let base = match named.support_of.as_deref() {
    Some(base) => known.iter().find(|one| one.name == base),
    None => Some(named),
  };
  let Some(base) = base else {
    return Err(refused(format!(
      "node-land: {worker}'s base is no worker with a checkout on this machine"
    )));
  };
  let base_cwd = base.cwd.clone();
  let checkout = Checkout::local(base_cwd.clone());
  let main = if base.node.is_none() { checkout.main_branch() } else { "main".to_owned() };
  let lock = stand.ground.host.locks.join(lock_file_name(&checkout.landing_lock_name()));
  let Some(_held) = hold_within(&lock, LAND_LOCK_WAIT).await else {
    return Ok(Answer {
      lines: vec![format!("another landing of {base_cwd} held the lock past the wait")],
      code: 1,
    });
  };

  println!("held");
  let _ = std::io::stdout().flush();

  let said = tokio::task::spawn_blocking(word).await.unwrap_or_default();
  let Some(commit) = said.strip_prefix("landed ").map(str::trim) else {
    return Ok(Answer::default());
  };
  let mut lines = Vec::new();

  // A base standing on a node itself has no checkout here to follow.
  if base.node.is_none() {
    let head = |reference: &str| {
      git(&base_cwd, &["rev-parse", reference])
        .map(|out| out.trim().to_owned())
        .unwrap_or_default()
    };
    let before = head(&main);

    if let Err(why) = checkout.follow_remote_main(&main) {
      return Ok(Answer {
        lines: vec![format!(
          "the base checkout {base_cwd} could not follow origin/{main} and catches up at the next landing: {why}"
        )],
        code: 1,
      });
    }

    let changed: Vec<String> = git(&base_cwd, &["diff", "--name-only", &before, &head(&main)])
      .unwrap_or_default()
      .lines()
      .filter(|line| !line.is_empty())
      .map(str::to_owned)
      .collect();

    if setup_inputs_moved(&changed) {
      lines.push(match checkout.run_setup(SETUP_RUNTIME) {
        Ok(()) => format!("{base_cwd} stands on the landed {main} and what its setup is made from moved, so it was set up again"),
        Err(why) => format!("{base_cwd} stands on the landed {main} and what its setup is made from moved, but its setup failed: {why}"),
      });
    }
  }

  if let Some(reason) = record {
    lines.push(record_pending(stand, &base_cwd, commit, None, Some(worker), reason).await);
  }

  Ok(Answer::from(lines))
}
