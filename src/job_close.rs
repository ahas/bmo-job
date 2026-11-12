//! `bmo-job job close` from a shell (docs/cli.md, docs/deploy.md): the job's commit handed over to its
//! repository's deployment, with the range narrowed to the commit that deployment last wrote down as live,
//! or to the job's own base where that commit is not in the repository the close runs in. Where the close
//! stands is written on the item it was told, as two of the item's labels.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use bmo_job_client::{ClientFailure, Item, ItemChange, JobClient};
use serde::Deserialize;

use crate::again::{Halt, until_answered};
use crate::close_watch::{Running, StepEnd, output_within, run_step, spoken};

/// How long a step may write nothing and touch nothing before it is hung: deliberately longer than
/// bmo-testbed's own ten-minute bound on a silent unit, so the testbed ends a hung unit first and names it.
pub const SILENCE: Duration = Duration::from_secs(15 * 60);
/// How long a process sent a TERM is given before a KILL.
pub const GRACE: Duration = Duration::from_secs(10);
/// How often a running step is looked at.
pub const LOOK: Duration = Duration::from_secs(1);
/// How often a running close says where it stands.
pub const STANDING_EVERY: Duration = Duration::from_secs(60);
/// How long a `git` or service-manager call may run before it is ended and read as failed.
pub const COMMAND_WITHIN: Duration = Duration::from_secs(60);
/// How long one request writing an item's labels is waited for before it is taken as no answer.
pub const ANSWER_WITHIN: Duration = Duration::from_secs(10);
/// How long the labels are written again while the service gives no answer, before the close goes on.
pub const STANDING_WITHIN: Duration = Duration::from_secs(2 * 60);

/// The clocks a close keeps to (docs/cli.md, 「`bmo-job job close`」), each the constant of its name where a
/// close runs for a job.
#[derive(Clone, Debug)]
pub struct Clocks {
  pub silence: Duration,
  pub grace: Duration,
  pub look: Duration,
  pub standing_every: Duration,
  pub look_again: Duration,
}

impl Default for Clocks {
  fn default() -> Self {
    Self {
      silence: SILENCE,
      grace: GRACE,
      look: LOOK,
      standing_every: STANDING_EVERY,
      look_again: LOOK_AGAIN,
    }
  }
}

/// What a close runs its check and suite with: the testbed's command, the clocks it keeps to, the step it is
/// running for a stop to end, and where its lines saying where it stands go.
pub struct Steps<'a> {
  pub testbed: &'a Path,
  pub clocks: Clocks,
  pub running: Running,
  pub say: &'a dyn Fn(&str),
}

/// The label saying where an item's close stands, and the one naming the commit it handed over.
pub const PHASE_LABEL: &str = "close-phase";
pub const COMMIT_LABEL: &str = "close-commit";

const TESTING: &str = "testing";
const WAITING_LIVE: &str = "waiting-live";

/// Where a close stands, as its item says it: its check and suite under way, its commit waiting to go live,
/// or neither.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
  Testing,
  WaitingLive(String),
  Off,
}

/// The commit an item waits on to go live: the one its close handed over, where its labels say it waits.
pub fn waiting_commit(item: &Item) -> Option<&str> {
  (item.labels.get(PHASE_LABEL).map(String::as_str) == Some(WAITING_LIVE))
    .then(|| item.labels.get(COMMIT_LABEL).map_or("", String::as_str))
}

/// Where the close stands written on its item, the item's other labels kept; nothing is written where the
/// item already says it. One read and one write, either of which may fail.
pub async fn write_standing(
  client: &JobClient,
  item: i64,
  standing: &Standing,
) -> Result<(), ClientFailure> {
  let read = client.item(item).await?;
  let mut labels = read.labels.clone();

  labels.remove(PHASE_LABEL);
  labels.remove(COMMIT_LABEL);
  match standing {
    Standing::Testing => {
      labels.insert(PHASE_LABEL.to_owned(), TESTING.to_owned());
    }
    Standing::WaitingLive(commit) => {
      labels.insert(PHASE_LABEL.to_owned(), WAITING_LIVE.to_owned());
      labels.insert(COMMIT_LABEL.to_owned(), commit.clone());
    }
    Standing::Off => {}
  }

  if labels == read.labels {
    return Ok(());
  }

  let change = ItemChange {
    labels: Some(labels),
    ..ItemChange::default()
  };

  client.change_item(item, &change).await.map(drop)
}

/// Where the close stands written on its item, written again while the service gives no answer, since a
/// restart is no answer (docs/cli.md, 「`bmo-job job close`」), until it lands, `within` has passed or `stop`
/// ends. A request unanswered for `ANSWER_WITHIN` is no answer. A refusal, the clock and a stop come back as the
/// one line the close says before it goes on.
pub async fn keep_standing(
  client: &JobClient,
  item: i64,
  standing: &Standing,
  within: Duration,
  stop: impl Future<Output = ()>,
) -> Result<(), String> {
  let stop = std::pin::pin!(stop);
  let write = || async {
    tokio::time::timeout(ANSWER_WITHIN, write_standing(client, item, standing))
      .await
      .unwrap_or_else(|_| {
        Err(ClientFailure::Unreachable(format!("no answer within {}", spoken(ANSWER_WITHIN))))
      })
  };

  match tokio::time::timeout(within, until_answered("job-close", stop, write)).await {
    Ok(written) => written.map_err(|halt| match halt {
      Halt::Answered(why) => format!("item {item} was not written on: {why}"),
      Halt::Stopped => format!("item {item} was not written on: the run was told to stop"),
    }),
    Err(_) => Err(format!(
      "item {item} was not written on: the job service gave no answer within {}",
      spoken(within)
    )),
  }
}

/// What a close is asked: why the commit goes live, and why the suite is skipped where it is.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobCloseArgs {
  pub then_reason: String,
  pub skip_suite: Option<String>,
}

/// A commit of the job's own: its short key and subject for a refusal, and its whole message for the
/// language the repository keeps.
struct OwnCommit {
  short: String,
  subject: String,
  text: String,
}

/// Closes a job's commit: reads the live commit from the deployment record, reads the job's own commits past
/// it, refuses an unpushed commit or one the remote does not know, runs the check and the suite, and hands
/// the commit over to the deployment that records what goes live. The check and the suite run as `steps` says,
/// watched. `tell` is handed where the close stands as it moves: testing once the check starts, waiting once it
/// is handed over, and off where it ends any other way.
pub fn close(
  root: &Path,
  data_root: &Path,
  args: &JobCloseArgs,
  steps: &Steps,
  tell: &dyn Fn(Standing),
) -> Result<(Vec<String>, Option<String>), String> {
  if let Some(refusal) = reasons_refusal(args) {
    return Err(refusal);
  }

  let closed = handed_over(root, data_root, args, steps, tell);

  match &closed {
    Ok(Handed::Waiting(_, commit)) => tell(Standing::WaitingLive(commit.clone())),
    Ok(Handed::Live(_)) | Err(_) => tell(Standing::Off),
  }

  closed.map(|handed| match handed {
    Handed::Waiting(lines, commit) => (lines, Some(commit)),
    Handed::Live(lines) => (lines, None),
  })
}

/// How often the deployment's records are read again while a close waits on its commit going live.
pub const LOOK_AGAIN: Duration = Duration::from_secs(5);

/// How the deployment that took a close's commit ended, as the close reads it (docs/cli.md, 「`bmo-job job
/// close`」): live, or not, with what the deployment said of why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Deployed {
  Live(Vec<String>),
  NotLive(Vec<String>),
}

/// Waits, with no clock, for a deployment begun at `since` or after that carries `commit` to end, reading the
/// deployment's progress record under the checkout's deploy root (bmo-testbed's docs/deploy.md, 「What a
/// deployment says of itself as it runs」) and its record of what went live. A record left unended by a run
/// whose process or unit is gone is a deployment cut off, read once more before it is taken as one. The
/// records are read again every `look_again`, and every `standing_every` `say` is handed one line saying how
/// long the close has waited and whether a deployment carrying its commit has begun.
pub fn wait_deployed(
  root: &Path,
  data_root: &Path,
  commit: &str,
  since: i64,
  clocks: &Clocks,
  say: &dyn Fn(&str),
) -> Deployed {
  // A repository whose declaration reads and names no deployment is never put live: its commit on main is the end.
  if deploys_nothing(root) {
    return Deployed::Live(vec![format!(
      "live: {} stands on main, and this repository deploys nothing",
      short_of(commit)
    )]);
  }

  let deploy_root = declared_deploy_root(root).unwrap_or_else(|| data_root.to_path_buf());
  let short = short_of(commit);
  let began = Instant::now();
  let mut said = began;

  loop {
    let progress = read_progress(&deploy_root);
    let carries = |of: &str| of == commit || is_ancestor(root, commit, of);

    if let Some(progress) = progress.as_ref().filter(|one| one.started_at >= since && carries(&one.commit))
    {
      match &progress.ending {
        Some(ending) if ending.outcome == "live" => {
          return Deployed::Live(vec![format!(
            "live: {short} is live, deployed as {}",
            short_of(&progress.commit)
          )]);
        }
        Some(ending) => {
          let why = ending.why.as_deref().map(|why| format!(": {why}")).unwrap_or_default();

          return Deployed::NotLive(not_live(progress, &short, &format!("ended {}{why}", ending.outcome)));
        }
        None
          if progress.run_gone()
            && read_progress(&deploy_root).is_some_and(|again| again.cut_off(progress)) =>
        {
          return Deployed::NotLive(not_live(
            progress,
            &short,
            "was cut off: its record has no ending, and its process or its unit is gone",
          ));
        }
        None => {}
      }
    } else if progress.as_ref().is_none_or(|one| one.ending.is_some())
      && deployed_commit_under(&deploy_root).is_some_and(|live| carries(&live))
    {
      return Deployed::Live(vec![format!("live: {short} is live")]);
    }

    if said.elapsed() >= clocks.standing_every {
      let under_way = progress
        .as_ref()
        .filter(|one| one.started_at >= since && one.ending.is_none() && carries(&one.commit))
        .map_or_else(
          || "no deployment carrying it has begun".to_owned(),
          |one| format!("the deployment of {} carrying it is under way", short_of(&one.commit)),
        );

      say(&format!(
        "job-close: waiting for the deployment of {short} for {}: {under_way}",
        spoken(began.elapsed())
      ));
      said = Instant::now();
    }

    std::thread::sleep(clocks.look_again);
  }
}

// What a close says of a deployment carrying its commit that did not put it live: how it ended, where its whole
// account stands, and that the commit is the job's to mend.
fn not_live(progress: &Progress, short: &str, ended: &str) -> Vec<String> {
  let mut lines = vec![format!(
    "not live: the deployment of {}, carrying {short}, {ended}",
    short_of(&progress.commit)
  )];

  if let Some(unit) = &progress.unit {
    lines.push(format!("its whole account: journalctl --user -u {unit} --no-pager"));
  }

  lines.push("the commit is not live: find the cause, mend it, commit, push and close again".to_owned());
  lines
}

/// The deployment's progress record as a waiting close reads it.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Progress {
  #[serde(default)]
  commit: String,
  #[serde(default)]
  started_at: i64,
  #[serde(default)]
  unit: Option<String>,
  #[serde(default)]
  pid: Option<u32>,
  #[serde(default)]
  ending: Option<Ending>,
}

impl Progress {
  /// Whether the run that writes this record is gone: its process no longer stands, or the service manager
  /// reads its user unit inactive or failed. A unit the service manager cannot be asked of is taken as running.
  fn run_gone(&self) -> bool {
    self.pid.is_some_and(|pid| !Path::new("/proc").join(pid.to_string()).exists())
      || self.unit.as_deref().is_some_and(unit_gone)
  }

  /// Whether this record, read after `before` was found with its run gone, is still that run's, unended.
  fn cut_off(&self, before: &Progress) -> bool {
    self.ending.is_none() && self.started_at == before.started_at && self.commit == before.commit
  }
}

fn unit_gone(unit: &str) -> bool {
  output_within(Command::new("systemctl").args(["--user", "is-active", unit]), COMMAND_WITHIN)
    .is_ok_and(|output| matches!(String::from_utf8_lossy(&output.stdout).trim(), "inactive" | "failed"))
}

#[derive(Deserialize)]
struct Ending {
  #[serde(default)]
  outcome: String,
  #[serde(default)]
  why: Option<String>,
}

fn read_progress(deploy_root: &Path) -> Option<Progress> {
  serde_json::from_str(&std::fs::read_to_string(deploy_root.join("deploy-progress.json")).ok()?).ok()
}

// How a close ended where it was not refused: its commit already live, or handed over to wait.
enum Handed {
  Live(Vec<String>),
  Waiting(Vec<String>, String),
}

fn handed_over(
  root: &Path,
  data_root: &Path,
  args: &JobCloseArgs,
  steps: &Steps,
  tell: &dyn Fn(Standing),
) -> Result<Handed, String> {
  let head =
    git(root, &["rev-parse", "HEAD"]).map_err(|why| format!("the checkout is on no commit: {why}"))?;
  let short = short_of(&head);
  let mut lines = vec![format!("job-close: {}", args.then_reason.trim())];
  let deployed = deployed_commit_of(root, data_root);

  // A commit its deployment already records as live runs nothing and ends clean.
  if deployed.as_deref() == Some(head.as_str()) {
    lines.push("this commit is already what is deployed".to_owned());
    return Ok(Handed::Live(lines));
  }

  let remote = remote_branch(root);
  let main = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".to_owned());
  let base = match deployed.as_deref() {
    // The record's commit is the range only where this repository holds it and HEAD stands past it.
    Some(commit) if is_ancestor(root, commit, &head) => commit.to_owned(),
    record => {
      let base = job_base(root).ok_or_else(|| no_range_refusal(root, record))?;
      lines.push(match record {
        Some(commit) => format!(
          "the deployment record's {} is not in this repository or not an ancestor of HEAD; the range is {}..{}",
          short_of(commit),
          short_of(&base),
          short
        ),
        None => format!(
          "no deployment record names the live commit; the range is {}..{}",
          short_of(&base),
          short
        ),
      });

      base
    }
  };

  if let Some(refusal) = commits_refusal(root, &base, &main)? {
    return Err(refusal);
  }

  let known = git(root, &["rev-parse", "--verify", &remote]).is_ok();
  if !known || !is_ancestor(root, &head, &remote) {
    return Err(format!("{short} is not on {remote}; an unpushed commit is not deployable"));
  }

  let found = checkout_state(root);

  tell(Standing::Testing);
  step(root, steps, "check", "the static check", found.as_deref())?;
  if args.skip_suite.is_none() {
    step(root, steps, "suite", "the suite", found.as_deref())?;
  }

  lines.push(format!("waiting to go live: {short} stands pushed for its deployment"));
  Ok(Handed::Waiting(lines, head))
}

/// The commit the checkout's own deployment last put live (docs/deploy.md, "A job closes"): the record under
/// the deploy root its declaration names (`testbed.toml`, `[deploy] root`), else the platform's own record
/// under the data root where the checkout declares no root.
pub fn deployed_commit_of(root: &Path, data_root: &Path) -> Option<String> {
  match declared_deploy_root(root) {
    Some(deploy_root) => deployed_commit_under(&deploy_root),
    None => deployed_commit_under(data_root),
  }
}

/// The refusal of the job's own commits, those past the commit the deployment recorded, where any message
/// holds Hangul: each such commit named. None where every message is English.
pub fn commits_refusal(root: &Path, base: &str, main: &str) -> Result<Option<String>, String> {
  let named: Vec<String> = own_commits(root, base)?
    .into_iter()
    .filter(|commit| hangul(&commit.text))
    .map(|commit| format!("{} {}", commit.short, commit.subject))
    .collect();

  Ok((!named.is_empty()).then(|| {
    format!(
      "these commits hold Hangul, and what goes into git is English only: {}. Reword each in English (git commit --amend for the last, git rebase -i {main} for an earlier one) and close again",
      named.join("; ")
    )
  }))
}

// The reasons a close is given: both English, and the suite's skip carrying why it is skipped.
fn reasons_refusal(args: &JobCloseArgs) -> Option<String> {
  if args.then_reason.trim().is_empty() {
    return Some(
      "usage: bmo job close --then '<why this goes live, in English>' [--todo <item>] [--skip-suite '<why, in English>']"
        .to_owned(),
    );
  }
  if args.skip_suite.as_deref().is_some_and(|reason| reason.trim().is_empty()) {
    return Some("--skip-suite takes the reason the change reaches nothing the suite runs".to_owned());
  }
  for (flag, reason) in
    [("then", Some(args.then_reason.as_str())), ("skip-suite", args.skip_suite.as_deref())]
  {
    if reason.is_some_and(hangul) {
      return Some(format!("--{flag} holds Hangul; what goes into git is English only"));
    }
  }

  None
}

// Whether the checkout's declaration reads and declares no deployment at all.
fn deploys_nothing(root: &Path) -> bool {
  std::fs::read_to_string(root.join("testbed.toml"))
    .ok()
    .and_then(|text| toml::from_str::<Declaration>(&text).ok())
    .is_some_and(|declaration| declaration.deploy.is_none())
}

// The deploy root the checkout's declaration names, a bare `~` or `~/` standing for the home and a relative
// path taken from the checkout; none where it declares none or its declaration does not read.
fn declared_deploy_root(root: &Path) -> Option<PathBuf> {
  let text = std::fs::read_to_string(root.join("testbed.toml")).ok()?;
  let declaration: Declaration = toml::from_str(&text).ok()?;
  let named = declaration.deploy?.root?;
  let expanded = if named == "~" {
    home()
  } else if let Some(rest) = named.strip_prefix("~/") {
    home().join(rest)
  } else {
    PathBuf::from(&named)
  };

  Some(if expanded.is_absolute() { expanded } else { root.join(expanded) })
}

// The commit recorded as live under a deploy root: `deployed.json` where it reads, else the last line of
// `deployed.log`, which names the commit after the moment the deployment was made.
fn deployed_commit_under(deploy_root: &Path) -> Option<String> {
  read_deployed_json(deploy_root).or_else(|| last_deployed_commit(deploy_root))
}

fn read_deployed_json(deploy_root: &Path) -> Option<String> {
  let text = std::fs::read_to_string(deploy_root.join("deployed.json")).ok()?;
  let record: Record = serde_json::from_str(&text).ok()?;

  Some(record.commit)
}

fn last_deployed_commit(deploy_root: &Path) -> Option<String> {
  let log = std::fs::read_to_string(deploy_root.join("deployed.log")).ok()?;

  log.lines().rev().find_map(|line| {
    let commit = line.split_whitespace().nth(1)?;

    (commit.len() == 40 && commit.chars().all(|letter| letter.is_ascii_hexdigit()))
      .then(|| commit.to_owned())
  })
}

fn own_commits(root: &Path, base: &str) -> Result<Vec<OwnCommit>, String> {
  let list = git(root, &["rev-list", &format!("{base}..HEAD")])?;
  let mut commits = Vec::new();

  for hash in list.lines().filter(|line| !line.is_empty()) {
    let text = git(root, &["show", "-s", "--format=%B", hash])?;
    let subject = git(root, &["show", "-s", "--format=%s", hash])?;
    commits.push(OwnCommit {
      short: hash.chars().take(12).collect(),
      subject,
      text,
    });
  }

  Ok(commits)
}

fn remote_branch(root: &Path) -> String {
  git(root, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"])
    .ok()
    .filter(|branch| !branch.is_empty() && branch != "@{upstream}")
    .unwrap_or_else(|| "origin/main".to_owned())
}

fn is_ancestor(root: &Path, ancestor: &str, descendant: &str) -> bool {
  output_within(
    Command::new("git")
      .arg("-C")
      .arg(root)
      .args(["merge-base", "--is-ancestor", ancestor, descendant]),
    COMMAND_WITHIN,
  )
  .is_ok_and(|output| output.status.success())
}

// The job's own base commit: the last commit HEAD shares with the branch the checkout was made from, so the
// range past it holds this job's commits alone.
fn job_base(root: &Path) -> Option<String> {
  let landing = landing_ref(root)?;
  let base = git(root, &["merge-base", "HEAD", &landing]).ok()?;

  (!base.is_empty()).then_some(base)
}

// The branch the checkout was made from: the one its git configuration writes down, else `main`, else
// `master`, else the remote branch it follows.
fn landing_ref(root: &Path) -> Option<String> {
  let current = git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
    .ok()
    .filter(|branch| !branch.is_empty());
  let remote = remote_branch(root);

  if let Some(branch) = current.as_deref() {
    for key in ["bmo-main", "bmo-from"] {
      if let Ok(from) = git(root, &["config", "--get", &format!("branch.{branch}.{key}")])
        && !from.is_empty()
        && resolves(root, &from)
      {
        return Some(from);
      }
    }

    if branch != "main" && branch != "master" {
      for name in ["main", "master", remote.as_str()] {
        if resolves(root, name) {
          return Some(name.to_owned());
        }
      }
    }
  }

  if resolves(root, &remote) {
    return Some(remote);
  }

  ["main", "master"].into_iter().find(|name| resolves(root, name)).map(str::to_owned)
}

fn resolves(root: &Path, name: &str) -> bool {
  git(root, &["rev-parse", "--verify", "--quiet", &format!("{name}^{{commit}}")]).is_ok()
}

// Why no range was established: the record's commit when it names one, and the repository the close runs in.
fn no_range_refusal(root: &Path, record: Option<&str>) -> String {
  let repository = root
    .file_name()
    .map_or_else(|| "this repository".to_owned(), |name| name.to_string_lossy().into_owned());

  match record {
    Some(commit) => format!(
      "no range can be established in {repository}: the deployment record names {}, which is not an ancestor of HEAD there, and no branch of it serves as the job's base",
      short_of(commit)
    ),
    None => format!(
      "no range can be established in {repository}: no deployment record names the live commit, and no branch of it serves as the job's base"
    ),
  }
}

fn short_of(commit: &str) -> String {
  commit.chars().take(12).collect()
}

// One step of the testbed's, `verb`, run in the checkout and watched. A step the close ended for falling silent
// says, besides its last line and what was running, how the checkout stands against `found`.
fn step(root: &Path, steps: &Steps, verb: &str, what: &str, found: Option<&str>) -> Result<(), String> {
  let mut command = Command::new(steps.testbed);
  command.arg(verb).current_dir(root);

  run_step(command, what, &steps.clocks, &steps.running, steps.say).map_err(|end| match end {
    StepEnd::Failed(why) => why,
    StepEnd::Hung(why) => format!("{why}; {}", checkout_against(root, found)),
  })
}

// What the checkout holds as git reads it: its HEAD and every entry of its status, untracked files each named.
fn checkout_state(root: &Path) -> Option<String> {
  let head = git(root, &["rev-parse", "HEAD"]).ok()?;
  let status = git(root, &["status", "--porcelain=v1", "--untracked-files=all"]).ok()?;

  Some(format!("HEAD {head}\n{status}"))
}

// How the checkout stands against what the close read before its check: as found, or the entries that differ.
fn checkout_against(root: &Path, found: Option<&str>) -> String {
  let (Some(found), Some(now)) = (found, checkout_state(root)) else {
    return "the checkout's state could not be read".to_owned();
  };
  // Each entry is read trimmed, since the first line of what git answers reaches here trimmed already.
  let entries = |state: &str| state.lines().map(|entry| entry.trim().to_owned()).collect::<Vec<_>>();
  let (before, after) = (entries(found), entries(&now));
  let differ: Vec<String> = after
    .iter()
    .filter(|entry| !before.contains(entry))
    .map(|entry| format!("now {entry}"))
    .chain(before.iter().filter(|entry| !after.contains(entry)).map(|entry| format!("was {entry}")))
    .collect();

  if differ.is_empty() {
    "the checkout stands as the close found it".to_owned()
  } else {
    format!("the checkout differs from how the close found it: {}", differ.join("; "))
  }
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
  let output = output_within(Command::new("git").arg("-C").arg(root).args(args), COMMAND_WITHIN)
    .map_err(|why| format!("git {} could not be run: {why}", args.first().copied().unwrap_or_default()))?;

  if !output.status.success() {
    return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
  }

  Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn hangul(text: &str) -> bool {
  text.chars().any(|letter| ('\u{AC00}'..='\u{D7A3}').contains(&letter))
}

fn home() -> PathBuf {
  std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

#[derive(Deserialize)]
struct Declaration {
  #[serde(default)]
  deploy: Option<Deploy>,
}

#[derive(Deserialize)]
struct Deploy {
  #[serde(default)]
  root: Option<String>,
}

#[derive(Deserialize)]
struct Record {
  commit: String,
}
