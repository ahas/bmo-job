//! `bmo-job job close` from a shell (docs/cli.md, docs/deploy.md): what the job changed checked, its commits
//! pushed onto the remote's main, and the item left waiting for its repository's batch deployment. The close runs
//! no suite and waits for no deployment. Where the close stands is written on the item it was told, as a pair of
//! labels for its own repository beside the pair summing up every repository's.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use bmo_job_client::{ClientFailure, Item, ItemChange, JobClient, Labels};
use serde::Deserialize;

use crate::again::{Halt, until_answered};
use crate::close_watch::{Running, StepEnd, output_within, run_step, spoken};

/// How long the check may write nothing and touch nothing before it is hung, in a checkout whose declaration
/// sets no testbed silence: the testbed's own ten-minute default and the margin above it.
pub const SILENCE: Duration = Duration::from_secs(15 * 60);
/// How far the close's silence stands above the testbed's own bound on a silent unit, so the testbed ends a hung
/// unit first and names it.
pub const SILENCE_MARGIN: Duration = Duration::from_secs(5 * 60);
/// How long a process sent a TERM is given before a KILL.
pub const GRACE: Duration = Duration::from_secs(10);
/// How often the running check is looked at.
pub const LOOK: Duration = Duration::from_secs(1);
/// How often a running close says where it stands.
pub const STANDING_EVERY: Duration = Duration::from_secs(60);
/// How long a local `git` call may run before it is ended and read as failed.
pub const COMMAND_WITHIN: Duration = Duration::from_secs(60);
/// How long a `git` call that reaches the remote, the fetch and the push, may run before it is ended and read as
/// failed.
pub const REMOTE_WITHIN: Duration = Duration::from_secs(5 * 60);
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
}

impl Default for Clocks {
  fn default() -> Self {
    Self {
      silence: SILENCE,
      grace: GRACE,
      look: LOOK,
      standing_every: STANDING_EVERY,
    }
  }
}

impl Clocks {
  /// The clocks a close keeps to in the checkout at `root`: the defaults, its silence read from the checkout's
  /// declaration (`silence_for`).
  pub fn for_checkout(root: &Path) -> Self {
    Self {
      silence: silence_for(root),
      ..Self::default()
    }
  }
}

/// The close's silence in the checkout at `root`: the margin above the testbed silence its `testbed.toml` declares
/// (`[test] silence_seconds`), else `SILENCE` where it declares none or its declaration does not read.
pub fn silence_for(root: &Path) -> Duration {
  declaration(root)
    .and_then(|declaration| declaration.test?.silence_seconds)
    .map_or(SILENCE, |seconds| Duration::from_secs(seconds).saturating_add(SILENCE_MARGIN))
}

/// What a close runs its check with: the testbed's command, the clocks it keeps to, the step it is running for a
/// stop to end, and where its lines saying where it stands go.
pub struct Steps<'a> {
  pub testbed: &'a Path,
  pub clocks: Clocks,
  pub running: Running,
  pub say: &'a dyn Fn(&str),
}

/// The label summing up where an item's closes stand, and the one naming a commit a close left waiting on main
/// (docs/cli.md, 「`bmo-job job close`」). Each repository's own pair is these keys, `:` and its name.
pub const PHASE_LABEL: &str = "close-phase";
pub const COMMIT_LABEL: &str = "close-commit";

const TESTING: &str = "testing";
const WAITING_BATCH: &str = "waiting-batch";

/// The label saying where the close of `repository` stands on an item.
pub fn phase_label_of(repository: &str) -> String {
  format!("{PHASE_LABEL}:{repository}")
}

/// The label naming the commit the close of `repository` left waiting on main.
pub fn commit_label_of(repository: &str) -> String {
  format!("{COMMIT_LABEL}:{repository}")
}

// The repository a repository's own phase label is for; none for any other label.
fn phase_label_for(key: &str) -> Option<&str> {
  key.strip_prefix(PHASE_LABEL)?.strip_prefix(':')
}

/// The commits an item waits on to be deployed: each repository whose close left its commit waiting on main, with
/// that commit, by repository name.
pub fn waiting_commits(labels: &Labels) -> BTreeMap<String, String> {
  labels
    .iter()
    .filter(|(_, phase)| phase.as_str() == WAITING_BATCH)
    .filter_map(|(key, _)| phase_label_for(key))
    .map(|repository| {
      let commit = labels.get(&commit_label_of(repository)).cloned().unwrap_or_default();

      (repository.to_owned(), commit)
    })
    .collect()
}

/// The labels an item carries once the close of `repository` stands as `standing`: that repository's pair written,
/// every other repository's pair kept, and the summing pair written again from them all.
pub fn standing_labels(labels: &Labels, repository: &str, standing: &Standing) -> Labels {
  let mut written = labels.clone();
  let named_before = written.remove(COMMIT_LABEL);

  written.remove(PHASE_LABEL);
  written.remove(&phase_label_of(repository));
  written.remove(&commit_label_of(repository));
  match standing {
    Standing::Testing => {
      written.insert(phase_label_of(repository), TESTING.to_owned());
    }
    Standing::WaitingBatch(commit) => {
      written.insert(phase_label_of(repository), WAITING_BATCH.to_owned());
      written.insert(commit_label_of(repository), commit.clone());
    }
    Standing::Off => {}
  }

  let testing = written
    .iter()
    .any(|(key, phase)| phase.as_str() == TESTING && phase_label_for(key).is_some());
  let waiting = waiting_commits(&written);

  if testing {
    written.insert(PHASE_LABEL.to_owned(), TESTING.to_owned());
  } else if let Some(first) = waiting.values().next() {
    let named = match standing {
      Standing::WaitingBatch(commit) => commit,
      _ => named_before
        .as_ref()
        .filter(|named| waiting.values().any(|commit| commit == *named))
        .unwrap_or(first),
    };

    written.insert(COMMIT_LABEL.to_owned(), named.clone());
    written.insert(PHASE_LABEL.to_owned(), WAITING_BATCH.to_owned());
  }

  written
}

/// The repository a close in the checkout at `root` writes its labels for: the name its testbed declaration
/// gives, else the last part of the address of the remote its main is fetched from, without `.git`, else the
/// checkout's directory name.
pub fn repository_of(root: &Path) -> String {
  declaration(root)
    .and_then(|declaration| declaration.name)
    .filter(|name| !name.trim().is_empty())
    .or_else(|| {
      let address = git(root, &["remote", "get-url", &followed_remote(root)]).ok()?;
      let last = Path::new(address.trim_end_matches('/')).file_name()?.to_string_lossy().into_owned();
      let name = last.strip_suffix(".git").unwrap_or(&last);

      (!name.is_empty()).then(|| name.to_owned())
    })
    .unwrap_or_else(|| {
      root
        .file_name()
        .map_or_else(|| "checkout".to_owned(), |name| name.to_string_lossy().into_owned())
    })
}

/// Where a close stands, as its item says it: its check under way, its commit waiting on main for its
/// repository's batch deployment, or neither.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Standing {
  Testing,
  WaitingBatch(String),
  Off,
}

/// The commit an item's summing labels say it waits on to be deployed, where they say it waits.
pub fn waiting_commit(item: &Item) -> Option<&str> {
  (item.labels.get(PHASE_LABEL).map(String::as_str) == Some(WAITING_BATCH))
    .then(|| item.labels.get(COMMIT_LABEL).map_or("", String::as_str))
}

/// Where the close of `repository` stands written on its item (`standing_labels`), the item's other labels
/// kept; nothing is written where the item already says it. One read and one write, either of which may fail.
pub async fn write_standing(
  client: &JobClient,
  item: i64,
  repository: &str,
  standing: &Standing,
) -> Result<(), ClientFailure> {
  let read = client.item(item).await?;
  let labels = standing_labels(&read.labels, repository, standing);

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
  repository: &str,
  standing: &Standing,
  within: Duration,
  stop: impl Future<Output = ()>,
) -> Result<(), String> {
  let stop = std::pin::pin!(stop);
  let write = || async {
    tokio::time::timeout(ANSWER_WITHIN, write_standing(client, item, repository, standing))
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

/// What a close is asked: why the commit goes live, and a reason for skipping the suite, which the close reads
/// for its language alone since it runs no suite.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct JobCloseArgs {
  pub then_reason: String,
  pub skip_suite: Option<String>,
}

/// A commit of the job's own: its short key and subject for a refusal and for what the close says it merged, and
/// its whole message for the language the repository keeps.
struct OwnCommit {
  short: String,
  subject: String,
  text: String,
}

/// Closes a job's change: checks what the job changed, pushes its commits onto the remote's main, and leaves the
/// commit waiting there for its repository's batch deployment. The check runs as `steps` says, watched. `tell` is
/// handed where the close stands as it moves: testing once the check starts, waiting for the batch once the commit
/// stands on main in a repository that deploys, and off where it ends any other way. What comes back is what the
/// close says of itself: the commits it merged and the check that passed.
pub fn close(
  root: &Path,
  data_root: &Path,
  args: &JobCloseArgs,
  steps: &Steps,
  tell: &dyn Fn(Standing),
) -> Result<Vec<String>, String> {
  if let Some(refusal) = reasons_refusal(args) {
    return Err(refusal);
  }

  let closed = merged(root, data_root, args, steps, tell);

  match &closed {
    Ok(Closed::Waiting(_, commit)) => tell(Standing::WaitingBatch(commit.clone())),
    Ok(Closed::Ended(_)) | Err(_) => tell(Standing::Off),
  }

  closed.map(|closed| match closed {
    Closed::Waiting(lines, _) | Closed::Ended(lines) => lines,
  })
}

// How a close ended where it was not refused: its commit on main waiting for the batch deployment, or ended
// with nothing to wait for, the commit already deployed or its repository deploying nothing.
enum Closed {
  Waiting(Vec<String>, String),
  Ended(Vec<String>),
}

// The remote's main a close lands on: the remote the checkout's branch follows, else `origin`, and its `main`,
// else its `master`, with the commit it stands at once fetched.
struct Main {
  remote: String,
  branch: String,
  tip: String,
}

impl Main {
  fn name(&self) -> String {
    format!("{}/{}", self.remote, self.branch)
  }

  // The command that brings a branch main has moved past up onto it.
  fn bring_up(&self) -> String {
    format!("git pull --rebase {} {}", self.remote, self.branch)
  }
}

fn merged(
  root: &Path,
  data_root: &Path,
  args: &JobCloseArgs,
  steps: &Steps,
  tell: &dyn Fn(Standing),
) -> Result<Closed, String> {
  let head =
    git(root, &["rev-parse", "HEAD"]).map_err(|why| format!("the checkout is on no commit: {why}"))?;
  let short = short_of(&head);
  let mut lines = vec![format!("job-close: {}", args.then_reason.trim())];
  let deployed = deployed_commit_of(root, data_root);

  if args.skip_suite.is_some() {
    lines.push("the suite is the deployment's: --skip-suite changes nothing".to_owned());
  }

  // A commit its deployment already records as live runs nothing and ends clean.
  if deployed.as_deref() == Some(head.as_str()) {
    lines.push("this commit is already what is deployed".to_owned());
    return Ok(Closed::Ended(lines));
  }

  let main = fetched_main(root)?;
  let to_push = !is_ancestor(root, &head, &main.tip);
  let base = if to_push {
    // The close merges by fast-forward alone: main must stand under HEAD.
    if !is_ancestor(root, &main.tip, &head) {
      return Err(format!(
        "{} has moved past this branch; bring it up with {} and close again",
        main.name(),
        main.bring_up()
      ));
    }

    main.tip.clone()
  } else {
    range_on_main(root, deployed.as_deref(), &head, &mut lines)?
  };

  let own = own_commits(root, &base)?;

  if let Some(refusal) = language_refusal(&own, &main.name()) {
    return Err(refusal);
  }

  let found = checkout_state(root);

  tell(Standing::Testing);
  check(root, steps, &base, found.as_deref())?;

  if to_push {
    push(root, &main)?;
    lines.push(format!("merged: {} onto {}", merged_list(&own), main.name()));
  } else {
    lines.push(format!("merged: {short} already stands on {}", main.name()));
  }

  lines.push(format!("checked: the static check passed over what changed since {}", short_of(&base)));

  // A repository whose declaration reads and names no deployment is never put live: its commit on main is the end.
  if deploys_nothing(root) {
    lines.push(format!(
      "on main: {short} stands on {}, and this repository deploys nothing",
      main.name()
    ));
    return Ok(Closed::Ended(lines));
  }

  lines.push(format!("waiting for the batch deployment: {short} stands on {}", main.name()));
  Ok(Closed::Waiting(lines, head))
}

// The range of a job whose commits already stand on main: past the commit the deployment records as live, where
// this repository holds it under HEAD, else past the job's own base, said in one line.
fn range_on_main(
  root: &Path,
  deployed: Option<&str>,
  head: &str,
  lines: &mut Vec<String>,
) -> Result<String, String> {
  match deployed {
    Some(commit) if is_ancestor(root, commit, head) => Ok(commit.to_owned()),
    record => {
      let base = job_base(root).ok_or_else(|| no_range_refusal(root, record))?;
      let short = short_of(head);

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

      Ok(base)
    }
  }
}

// The remote's main, fetched, so whether HEAD stands on it, or it under HEAD, is read against what the remote holds
// now: its `main`, else its `master`.
fn fetched_main(root: &Path) -> Result<Main, String> {
  let remote = followed_remote(root);
  let mut failed = Vec::new();

  for branch in ["main", "master"] {
    match remote_git(root, &["fetch", "--quiet", &remote, &format!("refs/heads/{branch}")]) {
      Ok(_) => {
        let tip = git(root, &["rev-parse", "FETCH_HEAD"])
          .map_err(|why| format!("{remote}/{branch} was fetched but could not be read: {why}"))?;

        return Ok(Main {
          remote,
          branch: branch.to_owned(),
          tip,
        });
      }
      Err(why) => failed.push(why),
    }
  }

  Err(format!(
    "the main of the remote {remote} could not be fetched: {}",
    failed.first().map_or("", String::as_str)
  ))
}

// The remote the checkout's branch follows, else `origin`.
fn followed_remote(root: &Path) -> String {
  git(root, &["symbolic-ref", "--quiet", "--short", "HEAD"])
    .ok()
    .and_then(|branch| git(root, &["config", "--get", &format!("branch.{branch}.remote")]).ok())
    .filter(|remote| !remote.is_empty() && remote != ".")
    .unwrap_or_else(|| "origin".to_owned())
}

// HEAD pushed onto the remote's main. A push the remote refuses, its main having moved on, says how to close again.
fn push(root: &Path, main: &Main) -> Result<(), String> {
  remote_git(root, &["push", "--quiet", &main.remote, &format!("HEAD:refs/heads/{}", main.branch)])
    .map(drop)
    .map_err(|why| {
      format!(
        "the push onto {} was refused: {why}; bring the branch up with {} and close again",
        main.name(),
        main.bring_up()
      )
    })
}

// The commits a close merged, each as its short commit and subject, oldest first.
fn merged_list(own: &[OwnCommit]) -> String {
  own
    .iter()
    .rev()
    .map(|commit| format!("{} {}", commit.short, commit.subject))
    .collect::<Vec<_>>()
    .join("; ")
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

/// The refusal of the job's own commits, those past `base`, where any message holds Hangul: each such commit
/// named. None where every message is English.
pub fn commits_refusal(root: &Path, base: &str, main: &str) -> Result<Option<String>, String> {
  Ok(language_refusal(&own_commits(root, base)?, main))
}

fn language_refusal(own: &[OwnCommit], main: &str) -> Option<String> {
  let named: Vec<String> = own
    .iter()
    .filter(|commit| hangul(&commit.text))
    .map(|commit| format!("{} {}", commit.short, commit.subject))
    .collect();

  (!named.is_empty()).then(|| {
    format!(
      "these commits hold Hangul, and what goes into git is English only: {}. Reword each in English (git commit --amend for the last, git rebase -i {main} for an earlier one) and close again",
      named.join("; ")
    )
  })
}

// The reasons a close is given: both English.
fn reasons_refusal(args: &JobCloseArgs) -> Option<String> {
  if args.then_reason.trim().is_empty() {
    return Some(
      "usage: bmo job close --then '<why this goes live, in English>' [--todo <item>] [--skip-suite '<why, in English>']"
        .to_owned(),
    );
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
  declaration(root).is_some_and(|declaration| declaration.deploy.is_none())
}

// The checkout's declaration, `testbed.toml` at its root; none where it stands absent or does not read.
fn declaration(root: &Path) -> Option<Declaration> {
  let text = std::fs::read_to_string(root.join("testbed.toml")).ok()?;

  toml::from_str(&text).ok()
}

// The deploy root the checkout's declaration names, a bare `~` or `~/` standing for the home and a relative
// path taken from the checkout; none where it declares none or its declaration does not read.
fn declared_deploy_root(root: &Path) -> Option<PathBuf> {
  let named = declaration(root)?.deploy?.root?;
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

// The testbed's check of what changed since `base`, run in the checkout and watched. A check the close ended for
// falling silent says, besides its last line and what was running, how the checkout stands against `found`.
fn check(root: &Path, steps: &Steps, base: &str, found: Option<&str>) -> Result<(), String> {
  let mut command = Command::new(steps.testbed);
  command.args(["check", "--since", base]).current_dir(root);

  run_step(command, "the static check", &steps.clocks, &steps.running, steps.say).map_err(|end| match end {
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
  git_within(root, args, COMMAND_WITHIN)
}

// A `git` call that reaches the remote, on the longer clock, never stopping to ask for a password.
fn remote_git(root: &Path, args: &[&str]) -> Result<String, String> {
  git_within(root, args, REMOTE_WITHIN)
}

fn git_within(root: &Path, args: &[&str], within: Duration) -> Result<String, String> {
  let output = output_within(
    Command::new("git").arg("-C").arg(root).args(args).env("GIT_TERMINAL_PROMPT", "0"),
    within,
  )
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
  name: Option<String>,
  #[serde(default)]
  deploy: Option<Deploy>,
  #[serde(default)]
  test: Option<TestTable>,
}

#[derive(Deserialize)]
struct TestTable {
  #[serde(default)]
  silence_seconds: Option<u64>,
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
