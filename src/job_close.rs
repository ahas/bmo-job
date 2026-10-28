//! `bmo-job job close` from a shell (docs/cli.md, docs/deploy.md): the job's commit handed over to its
//! repository's deployment, with the range narrowed to the commit that deployment last wrote down as live.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

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
/// the commit over to the deployment that records what goes live.
pub fn close(
  root: &Path,
  data_root: &Path,
  args: &JobCloseArgs,
  testbed: &Path,
) -> Result<Vec<String>, String> {
  if let Some(refusal) = reasons_refusal(args) {
    return Err(refusal);
  }

  let head =
    git(root, &["rev-parse", "HEAD"]).map_err(|why| format!("the checkout is on no commit: {why}"))?;
  let short = head.chars().take(12).collect::<String>();
  let mut lines = vec![format!("job-close: {}", args.then_reason.trim())];
  let deployed = deployed_commit_of(root, data_root);

  // A commit its deployment already records as live runs nothing and ends clean.
  if deployed.as_deref() == Some(head.as_str()) {
    lines.push("this commit is already what is deployed".to_owned());
    return Ok(lines);
  }

  let remote = remote_branch(root);
  let main = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|_| "main".to_owned());
  let base = match deployed {
    Some(commit) => commit,
    None => git(root, &["rev-parse", "--verify", &remote]).map_err(|_| {
      format!("no deployment record names the live commit, and the remote {remote} is not known")
    })?,
  };

  if let Some(refusal) = commits_refusal(root, &base, &main)? {
    return Err(refusal);
  }

  let known = git(root, &["rev-parse", "--verify", &remote]).is_ok();
  if !known || !is_ancestor(root, &head, &remote) {
    return Err(format!("{short} is not on {remote}; an unpushed commit is not deployable"));
  }

  run(root, testbed, "check", "the static check")?;
  if args.skip_suite.is_none() {
    run(root, testbed, "suite", "the suite")?;
  }

  lines.push(format!("waiting to go live: {short} stands pushed for its deployment"));
  Ok(lines)
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
      "usage: bmo job close --then '<why this goes live, in English>' [--skip-suite '<why, in English>']"
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
  Command::new("git")
    .arg("-C")
    .arg(root)
    .args(["merge-base", "--is-ancestor", ancestor, descendant])
    .status()
    .map(|status| status.success())
    .unwrap_or(false)
}

fn run(root: &Path, testbed: &Path, verb: &str, what: &str) -> Result<(), String> {
  let status = Command::new(testbed)
    .arg(verb)
    .current_dir(root)
    .status()
    .map_err(|why| format!("{what} could not be started: {why}"))?;

  if !status.success() {
    return Err(format!("{what} did not pass"));
  }

  Ok(())
}

fn git(root: &Path, args: &[&str]) -> Result<String, String> {
  let output = Command::new("git")
    .arg("-C")
    .arg(root)
    .args(args)
    .output()
    .map_err(|why| format!("git could not be started: {why}"))?;

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
