//! A job's close ending at the merge (docs/cli.md, "`bmo-job job close`"; docs/deploy.md, "A job closes"): the
//! check over what changed since the remote's main, the job's commits pushed onto it, the item left waiting for
//! its repository's batch deployment, and the record its own deployment writes read for a commit already on main.

use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::support;

use bmo_job::close_watch::{Running, StepEnd, output_within, run_step};
use bmo_job::job_close::{
  Clocks, JobCloseArgs, Standing, Steps, close, commits_refusal, deployed_commit_of, keep_standing,
};

use bmo_job_client::{EventQuery, NewItem};

// The clocks a test's close keeps to: a step's silence, the grace after a TERM and the lines saying where it
// stands each a fraction of a second.
fn quick() -> Clocks {
  Clocks {
    silence: Duration::from_millis(300),
    grace: Duration::from_millis(200),
    look: Duration::from_millis(10),
    standing_every: Duration::from_millis(100),
  }
}

// A record as the testbed's deployment writes it, under the deploy root it names.
const RECORD: &str = r#"{
  "commit": "{commit}",
  "subject": "A deployment made through the testbed",
  "message": "A deployment made through the testbed",
  "version": "0.1.0-rev.1",
  "at": "2026-10-28T03:02:36.133Z",
  "backup": ""
}"#;

fn git(dir: &Path, args: &[&str]) -> String {
  let output = Command::new("git").arg("-C").arg(dir).args(args).output().expect("git runs");

  assert!(output.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&output.stderr));
  String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

fn init(dir: &Path) {
  git(dir, &["init", "-q", "-b", "main"]);
}

fn commit(dir: &Path, file: &str, message: &str) -> String {
  fs::write(dir.join(file), format!("{file}\n")).expect("the file");
  git(dir, &["add", file]);
  git(
    dir,
    &["-c", "user.name=tester", "-c", "user.email=tester", "commit", "-q", "-m", message],
  );

  git(dir, &["rev-parse", "HEAD"])
}

// A bare repository standing as the checkout's `origin`, its main pushed from the checkout's HEAD as it stands.
fn remote_for(repo: &Path) -> tempfile::TempDir {
  let remote = tempfile::tempdir().expect("a remote");

  git(remote.path(), &["init", "-q", "--bare", "-b", "main"]);
  git(repo, &["remote", "add", "origin", &remote.path().display().to_string()]);
  git(repo, &["push", "-q", "origin", "HEAD:refs/heads/main"]);

  remote
}

// The commit the remote's main stands at.
fn main_of(remote: &Path) -> String {
  git(remote, &["rev-parse", "refs/heads/main"])
}

// A second clone of the remote with a commit of its own, not yet pushed.
fn other_clone(remote: &Path) -> tempfile::TempDir {
  let other = tempfile::tempdir().expect("another clone");

  git(other.path(), &["clone", "-q", &remote.display().to_string(), "."]);
  commit(other.path(), "elsewhere.md", "Add a page from elsewhere");

  other
}

fn declare_deploy_root(repo: &Path, deploy_root: &Path) {
  fs::write(
    repo.join("testbed.toml"),
    format!(
      "testbed = 0\nname = \"the-test\"\n\n[deploy]\nroot = \"{}\"\nlink = \"deploy\"\n",
      deploy_root.display()
    ),
  )
  .expect("the declaration");
}

fn record_at(deploy_root: &Path, commit: &str) {
  fs::write(deploy_root.join("deployed.json"), RECORD.replace("{commit}", commit)).expect("the record");
}

// A record the platform's data root holds, which a checkout with no declaration reads.
fn platform_record_at(data_root: &Path, commit: &str) {
  fs::write(
    data_root.join("deployed.json"),
    format!(
      "{{\"commit\":\"{commit}\",\"subject\":\"A deployment\",\"at\":\"2026-10-21T18:36:32.828Z\",\"backup\":\"\"}}"
    ),
  )
  .expect("the platform record");
}

// A commit no checkout of the test's repository holds.
const FOREIGN: &str = "b0dd02ec3a4edabffb3c794b3d4e54eeffbe561b";

fn executable(path: &Path, body: &str) {
  fs::write(path, body).expect("the testbed stand-in");
  let mut permissions = fs::metadata(path).expect("the stand-in").permissions();
  permissions.set_mode(0o755);
  fs::set_permissions(path, permissions).expect("the stand-in is executable");
}

// A testbed standing in for the real one: it writes the words it was asked with on a line.
fn testbed_at(dir: &Path) -> (PathBuf, PathBuf) {
  let calls = dir.join("testbed-calls");
  let path = dir.join("bmo-testbed");

  executable(&path, &format!("#!/bin/sh\necho \"$@\" >> \"{}\"\nexit 0\n", calls.display()));

  (path, calls)
}

// A testbed stand-in whose every verb fails.
fn failing_testbed_at(dir: &Path) -> PathBuf {
  let path = dir.join("bmo-testbed-failing");

  executable(&path, "#!/bin/sh\nexit 1\n");

  path
}

fn run_close(
  root: &Path,
  data_root: &Path,
  then_reason: &str,
  skip_suite: Option<&str>,
  testbed: &Path,
) -> Result<Vec<String>, String> {
  told_close(root, data_root, then_reason, skip_suite, testbed).0
}

// A close, with every standing it told in order.
fn told_close(
  root: &Path,
  data_root: &Path,
  then_reason: &str,
  skip_suite: Option<&str>,
  testbed: &Path,
) -> (Result<Vec<String>, String>, Vec<Standing>) {
  let (closed, told, _) = said_close(root, data_root, then_reason, skip_suite, testbed);

  (closed, told)
}

// A close by the test's clocks, with every standing it told and every line it said of where it stands.
fn said_close(
  root: &Path,
  data_root: &Path,
  then_reason: &str,
  skip_suite: Option<&str>,
  testbed: &Path,
) -> (Result<Vec<String>, String>, Vec<Standing>, Vec<String>) {
  let told = RefCell::new(Vec::new());
  let said = RefCell::new(Vec::new());
  let steps = Steps {
    testbed,
    clocks: quick(),
    running: Running::default(),
    say: &|line| said.borrow_mut().push(line.to_owned()),
  };
  let closed = close(
    root,
    data_root,
    &JobCloseArgs {
      then_reason: then_reason.to_owned(),
      skip_suite: skip_suite.map(str::to_owned),
    },
    &steps,
    &|standing| told.borrow_mut().push(standing),
  );

  (closed, told.into_inner(), said.into_inner())
}

// A checkout whose first commit stands on its remote's main and is recorded live, and whose second is the job's
// own, not yet pushed.
struct Closable {
  repo: tempfile::TempDir,
  data: tempfile::TempDir,
  _deploy_root: tempfile::TempDir,
  remote: tempfile::TempDir,
  base: String,
  head: String,
}

fn closable() -> Closable {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let remote = remote_for(repo.path());
  let head = commit(repo.path(), "two.md", "Add the second page");
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &base);

  Closable {
    repo,
    data,
    _deploy_root: deploy_root,
    remote,
    base,
    head,
  }
}

#[test]
fn the_deploy_record_is_read_under_the_root_the_checkout_declares() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");
  let deployed = "d83a99f8da4953d32bb39541ac8658e9cdfb1753";

  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), deployed);

  assert_eq!(deployed_commit_of(repo.path(), data.path()), Some(deployed.to_owned()));
}

#[test]
fn the_deploy_log_names_the_live_commit_where_no_record_file_stands() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");
  let (older, newest) =
    ("665efe7d02cb60f6a3215cc98b15bd20a1a81134", "c9c5dfed79cf545265c6f5f140b019f1ff4cd381");

  declare_deploy_root(repo.path(), deploy_root.path());
  fs::write(
    deploy_root.path().join("deployed.log"),
    format!(
      "2026-10-27T01:30:54.590Z {older}  Hold a row to its kind\n2026-10-28T03:05:16.778Z {newest}  Run the migrations\n"
    ),
  )
  .expect("the log");

  assert_eq!(deployed_commit_of(repo.path(), data.path()), Some(newest.to_owned()));
}

#[test]
fn a_checkout_with_no_declaration_reads_the_platform_s_record() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deployed = "b0dd02ec3a4edabffb3c794b3d4e54eeffbe561b";

  fs::write(
    data.path().join("deployed.json"),
    format!(
      "{{\"commit\":\"{deployed}\",\"subject\":\"A deployment\",\"at\":\"2026-10-21T18:36:32.828Z\",\"backup\":\"\"}}"
    ),
  )
  .expect("the platform record");

  assert_eq!(deployed_commit_of(repo.path(), data.path()), Some(deployed.to_owned()));
}

#[test]
fn an_old_commit_before_the_deployed_one_is_not_read_as_the_job_s_own() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let start = commit(repo.path(), "README.md", "Start the repository");
  commit(repo.path(), "old.md", &format!("Add the old page\n\nAs asked: {}", '\u{AC00}'));
  let deployed = commit(repo.path(), "one.md", "Add the first page");
  commit(repo.path(), "two.md", "Add the second page");
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &deployed);

  let base = deployed_commit_of(repo.path(), data.path()).expect("the record names the deployed commit");
  assert_eq!(base, deployed);
  assert_eq!(commits_refusal(repo.path(), &base, "main").expect("reads"), None);

  let earlier = commits_refusal(repo.path(), &start, "main")
    .expect("reads")
    .expect("the old commit is read past that base");
  assert!(earlier.contains("Add the old page"), "{earlier}");
}

#[test]
fn a_close_reads_the_deploy_record_and_ends_clean_on_the_commit_it_names() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let head = commit(repo.path(), "one.md", "Add the first page");
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &head);

  let (testbed, calls) = testbed_at(data.path());
  let (closed, told) = told_close(repo.path(), data.path(), "Land the first page", None, &testbed);
  let lines = closed.expect("clean");

  assert!(
    lines.iter().any(|line| line.contains("this commit is already what is deployed")),
    "{lines:?}"
  );
  assert!(!calls.exists(), "the close runs nothing on a commit already deployed");
  assert_eq!(told, vec![Standing::Off]);
}

// The close's whole ending: the check over what changed since main, the job's commits pushed onto main, and the
// item told the commit waits there for the batch; no suite runs and no deployment is waited for.
#[test]
fn a_close_checks_since_main_merges_and_leaves_the_commit_waiting_for_the_batch() {
  let closable = closable();
  let (testbed, calls) = testbed_at(closable.data.path());
  let (closed, told) =
    told_close(closable.repo.path(), closable.data.path(), "Land the second page", None, &testbed);
  let lines = closed.expect("merged");
  let (base, head) = (&closable.base[..12], &closable.head[..12]);

  assert_eq!(
    lines[1..],
    [
      format!("merged: {head} Add the second page onto origin/main"),
      format!("checked: the static check passed over what changed since {base}"),
      format!("waiting for the batch deployment: {head} stands on origin/main"),
    ]
  );
  assert_eq!(main_of(closable.remote.path()), closable.head);
  assert_eq!(
    fs::read_to_string(calls).expect("the testbed stand-in's calls"),
    format!("check --since {}\n", closable.base),
    "the check alone runs, over what changed since main"
  );
  assert_eq!(told, vec![Standing::Testing, Standing::WaitingBatch(closable.head.clone())]);
}

#[test]
fn a_close_runs_no_suite_and_reads_a_skip_reason_for_its_language_alone() {
  let closable = closable();
  let (testbed, calls) = testbed_at(closable.data.path());
  let lines = run_close(
    closable.repo.path(),
    closable.data.path(),
    "Land the second page",
    Some("the change reaches no test"),
    &testbed,
  )
  .expect("merged");

  assert!(
    lines
      .iter()
      .any(|line| line == "the suite is the deployment's: --skip-suite changes nothing"),
    "{lines:?}"
  );
  assert_eq!(
    fs::read_to_string(calls).expect("the testbed stand-in's calls"),
    format!("check --since {}\n", closable.base)
  );

  let refused = run_close(
    closable.repo.path(),
    closable.data.path(),
    "Land the second page",
    Some(&format!("no test {}", '\u{AC00}')),
    &testbed,
  )
  .expect_err("a skip reason with Hangul");

  assert!(refused.contains("--skip-suite holds Hangul"), "{refused}");
}

#[test]
fn a_close_whose_check_fails_tells_testing_takes_it_off_and_pushes_nothing() {
  let closable = closable();
  let testbed = failing_testbed_at(closable.data.path());
  let (closed, told) =
    told_close(closable.repo.path(), closable.data.path(), "Land the second page", None, &testbed);

  assert_eq!(closed.expect_err("the check fails"), "the static check did not pass");
  assert_eq!(told, vec![Standing::Testing, Standing::Off]);
  assert_eq!(main_of(closable.remote.path()), closable.base);
}

#[test]
fn a_branch_main_has_moved_past_is_refused_with_the_command_that_brings_it_up() {
  let closable = closable();
  let other = other_clone(closable.remote.path());
  git(other.path(), &["push", "-q", "origin", "HEAD:refs/heads/main"]);
  let moved = main_of(closable.remote.path());

  let (testbed, calls) = testbed_at(closable.data.path());
  let (closed, told) =
    told_close(closable.repo.path(), closable.data.path(), "Land the second page", None, &testbed);
  let refused = closed.expect_err("main moved past the branch");

  assert_eq!(
    refused,
    "origin/main has moved past this branch; bring it up with git pull --rebase origin main and close again"
  );
  assert!(!calls.exists(), "a refused close runs nothing");
  assert_eq!(told, vec![Standing::Off]);
  assert_eq!(main_of(closable.remote.path()), moved);
}

// Main moving while the check runs refuses the push, and the item is not told the commit waits.
#[test]
fn a_push_main_refuses_after_the_check_ends_the_close_with_the_way_to_close_again() {
  let closable = closable();
  let other = other_clone(closable.remote.path());
  let testbed = closable.data.path().join("bmo-testbed-racing");

  executable(
    &testbed,
    &format!("#!/bin/sh\ngit -C \"{}\" push -q origin HEAD:refs/heads/main\n", other.path().display()),
  );

  let (closed, told) =
    told_close(closable.repo.path(), closable.data.path(), "Land the second page", None, &testbed);
  let refused = closed.expect_err("the push is refused");

  assert!(refused.starts_with("the push onto origin/main was refused: "), "{refused}");
  assert!(
    refused.ends_with("bring the branch up with git pull --rebase origin main and close again"),
    "{refused}"
  );
  assert_eq!(told, vec![Standing::Testing, Standing::Off]);
  assert_ne!(main_of(closable.remote.path()), closable.head);
}

#[test]
fn an_unpushed_commit_holding_hangul_is_refused_and_nothing_is_checked_or_pushed() {
  let closable = closable();
  let worded = commit(
    closable.repo.path(),
    "three.md",
    &format!("Add the third page\n\nAs asked: {}", '\u{AC00}'),
  );
  let (testbed, calls) = testbed_at(closable.data.path());
  let (closed, told) =
    told_close(closable.repo.path(), closable.data.path(), "Land the third page", None, &testbed);
  let refused = closed.expect_err("a message with Hangul");

  assert!(refused.contains(&format!("{} Add the third page", &worded[..12])), "{refused}");
  assert!(refused.contains("git rebase -i origin/main"), "{refused}");
  assert!(!calls.exists(), "a refused close runs nothing");
  assert_eq!(told, vec![Standing::Off]);
  assert_eq!(main_of(closable.remote.path()), closable.base);
}

#[test]
fn a_reason_holding_hangul_is_refused() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let (testbed, calls) = testbed_at(data.path());
  let refused = run_close(repo.path(), data.path(), &format!("Land {}", '\u{AC00}'), None, &testbed)
    .expect_err("a reason with Hangul");

  assert!(refused.contains("--then holds Hangul"), "{refused}");
  assert!(!calls.exists(), "a refused close runs nothing");
}

#[test]
fn a_remote_that_cannot_be_read_refuses_the_close() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  commit(repo.path(), "one.md", "Add the first page");

  let (testbed, calls) = testbed_at(data.path());
  let refused =
    run_close(repo.path(), data.path(), "Land the first page", None, &testbed).expect_err("no remote");

  assert!(refused.starts_with("the main of the remote origin could not be fetched: "), "{refused}");
  assert!(!calls.exists(), "a refused close runs nothing");
}

// A job that pushed its commit itself: nothing is pushed, and the range past the record's commit is checked.
#[test]
fn a_commit_already_on_main_is_checked_past_the_record_and_waits_for_the_batch() {
  let closable = closable();
  git(closable.repo.path(), &["push", "-q", "origin", "HEAD:refs/heads/main"]);

  let (testbed, calls) = testbed_at(closable.data.path());
  let (closed, told) =
    told_close(closable.repo.path(), closable.data.path(), "Land the second page", None, &testbed);
  let lines = closed.expect("waiting");
  let head = &closable.head[..12];

  assert!(lines.contains(&format!("merged: {head} already stands on origin/main")), "{lines:?}");
  assert!(
    lines.contains(&format!("waiting for the batch deployment: {head} stands on origin/main")),
    "{lines:?}"
  );
  assert_eq!(
    fs::read_to_string(calls).expect("the testbed stand-in's calls"),
    format!("check --since {}\n", closable.base)
  );
  assert_eq!(told, vec![Standing::Testing, Standing::WaitingBatch(closable.head.clone())]);
}

#[test]
fn a_foreign_record_falls_back_to_the_job_s_own_base_for_a_commit_already_on_main() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  let _remote = remote_for(repo.path());
  platform_record_at(data.path(), FOREIGN);

  let (testbed, calls) = testbed_at(data.path());
  let lines = run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect("waiting");
  let range = format!("the range is {}..{}", &head[..12], &head[..12]);

  assert!(
    lines.iter().any(|line| line.contains(&FOREIGN[..12]) && line.contains(&range)),
    "{lines:?}"
  );
  assert!(
    lines.iter().any(|line| line.starts_with("waiting for the batch deployment: ")),
    "{lines:?}"
  );
  assert_eq!(
    fs::read_to_string(calls).expect("the testbed stand-in's calls"),
    format!("check --since {head}\n")
  );
}

#[test]
fn a_foreign_record_does_not_read_an_earlier_hangul_commit_as_the_job_s_own() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  commit(repo.path(), "old.md", &format!("Add the old page\n\nAs asked: {}", '\u{AC00}'));
  commit(repo.path(), "one.md", "Add the first page");
  commit(repo.path(), "two.md", "Add the second page");
  let _remote = remote_for(repo.path());
  platform_record_at(data.path(), FOREIGN);

  let (testbed, _calls) = testbed_at(data.path());
  let lines = run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect("waiting");

  assert!(
    lines.iter().any(|line| line.starts_with("waiting for the batch deployment: ")),
    "{lines:?}"
  );
}

#[test]
fn a_record_commit_on_another_branch_is_not_taken_as_the_base() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  commit(repo.path(), "two.md", "Add the second page");
  let _remote = remote_for(repo.path());
  git(repo.path(), &["checkout", "-q", "-b", "other", &base]);
  let elsewhere = commit(repo.path(), "other.md", "Add the other page");
  git(repo.path(), &["checkout", "-q", "main"]);
  platform_record_at(data.path(), &elsewhere);

  let (testbed, _calls) = testbed_at(data.path());
  let lines = run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect("waiting");

  assert!(
    lines.iter().any(|line| line.starts_with("waiting for the batch deployment: ")),
    "{lines:?}"
  );
  assert!(lines.iter().any(|line| line.contains(&elsewhere[..12])), "{lines:?}");
}

// The record is read only for a commit already on main: a job's unpushed commits are measured from main.
#[test]
fn an_unpushed_commit_is_merged_from_main_while_the_record_is_foreign() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let remote = remote_for(repo.path());
  let head = commit(repo.path(), "two.md", "Add the second page");
  platform_record_at(data.path(), FOREIGN);

  let (testbed, _calls) = testbed_at(data.path());
  let lines = run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect("merged");

  assert!(!lines.iter().any(|line| line.contains(&FOREIGN[..12])), "{lines:?}");
  assert!(
    lines.contains(&format!("checked: the static check passed over what changed since {}", &base[..12])),
    "{lines:?}"
  );
  assert_eq!(main_of(remote.path()), head);
}

// A repository whose declaration names no deployment, a base library, is never put live: the close ends on main
// with nothing to wait for.
#[test]
fn a_close_in_a_repository_that_deploys_nothing_ends_on_main() {
  let closable = closable();

  fs::write(closable.repo.path().join("testbed.toml"), "testbed = 0\nname = \"the-library\"\n")
    .expect("the declaration");

  let (testbed, _calls) = testbed_at(closable.data.path());
  let (closed, told) =
    told_close(closable.repo.path(), closable.data.path(), "Land the second page", None, &testbed);
  let lines = closed.expect("on main");

  assert_eq!(
    lines.last().map(String::as_str),
    Some(
      format!(
        "on main: {} stands on origin/main, and this repository deploys nothing",
        &closable.head[..12]
      )
      .as_str()
    )
  );
  assert_eq!(told, vec![Standing::Testing, Standing::Off]);
  assert_eq!(main_of(closable.remote.path()), closable.head);
}

#[test]
fn the_command_carries_the_job_and_todo_nouns() {
  let output = Command::new(env!("CARGO_BIN_EXE_bmo-job"))
    .arg("nouns")
    .output()
    .expect("the binary runs");

  assert!(output.status.success());
  let nouns: Vec<&str> = std::str::from_utf8(&output.stdout).expect("utf8").lines().collect();
  assert_eq!(nouns, ["job", "job ask\thome", "todo"]);
}

// The binary end to end: the item's standing written through a job service restart, the check run, the commit
// pushed onto main, and the close ending at once with the item waiting for the batch.
#[tokio::test]
async fn the_close_writes_its_standing_through_a_job_service_restart_and_ends_at_the_merge() {
  let service = support::Service::start().await;
  let client = service.client();
  let relay = support::Relay::before(&service.url).await;
  let item = client
    .create_item(&NewItem {
      title: "closed through a restart".to_owned(),
      brief: "close".to_owned(),
      queue_name: "todo".to_owned(),
      ..NewItem::default()
    })
    .await
    .expect("the item")
    .id;
  let closable = closable();
  let tools = tempfile::tempdir().expect("a directory for the testbed stand-in");
  let (_, calls) = testbed_at(tools.path());
  let path = format!("{}:{}", tools.path().display(), std::env::var("PATH").unwrap_or_default());

  relay.go_down();

  let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_bmo-job"))
    .args(["job", "close", "--then", "Land the second page", "--todo", &item.to_string()])
    .current_dir(closable.repo.path())
    .env("BMO_JOB_URL", &relay.url)
    .env("BMO_JOB_TOKEN", support::TOKEN)
    .env("BMO_DATA_DIR", closable.data.path())
    .env("PATH", path)
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn()
    .expect("the close runs");

  relay.until_cut(3).await;
  assert!(!calls.exists(), "nothing is checked before the close's standing is written");
  relay.come_up();

  let output = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
    .await
    .expect("the close ends")
    .expect("its output");
  let (said, out) = (String::from_utf8_lossy(&output.stderr), String::from_utf8_lossy(&output.stdout));
  let head = &closable.head[..12];

  assert_eq!(output.status.code(), Some(0), "{said}");
  assert!(said.contains("asking again"), "{said}");
  assert!(
    out.trim_end().ends_with(&format!(
      "waiting for the batch deployment: {head} stands on origin/main\njob-close: ended, exit 0"
    )),
    "{out}"
  );
  assert_eq!(
    fs::read_to_string(&calls).expect("the testbed's calls"),
    format!("check --since {}\n", closable.base)
  );
  assert_eq!(main_of(closable.remote.path()), closable.head);

  let standing = client.item(item).await.expect("the item").labels;

  assert_eq!(standing.get("close-phase").map(String::as_str), Some("waiting-batch"), "{standing:?}");
  assert_eq!(standing.get("close-commit"), Some(&closable.head), "{standing:?}");

  let changes = client
    .events_page(&EventQuery {
      kinds: Some("item.changed".to_owned()),
      ..EventQuery::default()
    })
    .await
    .expect("the item's changes")
    .items;
  let phases: Vec<&str> = changes
    .iter()
    .filter(|event| event.item_id() == Some(item))
    .filter_map(|event| event.data.get("labels")?.get("close-phase")?.as_str())
    .collect();

  assert_eq!(phases, ["testing", "waiting-batch"]);
}

// A testbed stand-in whose check is `check`, a shell body run with the file the test keeps the stand-in's
// processes in as `$1`.
fn check_at(dir: &Path, check: &str) -> (PathBuf, Started) {
  let path = dir.join("bmo-testbed-check");
  let pids = dir.join("stand-in-pids");

  executable(
    &path,
    &format!(
      "#!/bin/sh\n[ \"$1\" = check ] || exit 0\nset -- \"{}\"\necho $$ >> \"$1\"\n{check}\n",
      pids.display()
    ),
  );

  (path, Started(pids))
}

// The processes a stand-in started, each written to a file of its own: killed when the test ends, whatever it
// came to.
struct Started(PathBuf);

impl Started {
  fn pids(&self) -> Vec<i32> {
    fs::read_to_string(&self.0)
      .unwrap_or_default()
      .split_whitespace()
      .filter_map(|pid| pid.parse().ok())
      .collect()
  }

  // Whether every one of them is gone: no longer listed, or ended and waiting only to be reaped.
  fn all_gone(&self) -> bool {
    self.pids().iter().all(|pid| {
      fs::read_to_string(format!("/proc/{pid}/stat")).map_or(true, |stat| {
        stat.rsplit(')').next().and_then(|rest| rest.split_whitespace().next()) == Some("Z")
      })
    })
  }
}

impl Drop for Started {
  fn drop(&mut self) {
    for pid in self.pids() {
      // SAFETY: a KILL to a process this test's stand-in started.
      unsafe { libc::kill(pid, libc::SIGKILL) };
    }
  }
}

// Waits for `condition` a few seconds at most, looking every few milliseconds.
fn soon(condition: impl Fn() -> bool) -> bool {
  (0..500).any(|_| {
    let met = condition();
    if !met {
      std::thread::sleep(Duration::from_millis(10));
    }
    met
  })
}

#[test]
fn a_check_that_falls_silent_ends_the_close_with_its_last_line_what_ran_and_the_checkout_as_found() {
  let closable = closable();
  let (testbed, started) = check_at(
    closable.data.path(),
    "echo 'unit agency-checks started'\nsetsid sleep 600 &\necho $! >> \"$1\"\nsleep 600 &\necho $! >> \"$1\"\nwait",
  );
  let (closed, told, _) =
    said_close(closable.repo.path(), closable.data.path(), "Land the second page", None, &testbed);
  let why = closed.expect_err("a silent check refuses the close");

  assert!(why.starts_with("the static check wrote nothing for 300ms and was ended"), "{why}");
  assert!(why.contains("its last line: unit agency-checks started"), "{why}");
  assert!(why.contains("still running: ") && why.contains("sleep 600"), "{why}");
  assert!(why.ends_with("the checkout stands as the close found it"), "{why}");
  assert_eq!(told, vec![Standing::Testing, Standing::Off]);
  assert_eq!(main_of(closable.remote.path()), closable.base);
  assert_eq!(
    started.pids().len(),
    3,
    "the stand-in, its own process and the one in a session of its own"
  );
  assert!(soon(|| started.all_gone()), "nothing the check started is left running");
}

#[test]
fn a_check_whose_output_is_held_back_but_heard_runs_past_the_silence_and_says_where_it_stands() {
  let closable = closable();
  let (testbed, _started) = check_at(
    closable.data.path(),
    "for i in 1 2 3 4 5 6; do touch \"$BMO_TESTBED_HEARD\"; sleep 0.1; done\necho 'check: ok'",
  );
  let (closed, told, said) =
    said_close(closable.repo.path(), closable.data.path(), "Land the second page", None, &testbed);

  closed.expect("a check that is heard is not ended");
  assert_eq!(told, vec![Standing::Testing, Standing::WaitingBatch(closable.head.clone())]);
  assert!(
    said.iter().any(|line| line.starts_with("job-close: the static check has run ")
      && line.contains("its output last moved")
      && line.ends_with(": nothing written")),
    "{said:?}"
  );
}

#[test]
fn a_stop_ends_the_step_running_with_everything_under_it() {
  let dir = tempfile::tempdir().expect("a directory");
  let (testbed, started) = check_at(
    dir.path(),
    "setsid sleep 600 &\necho $! >> \"$1\"\nsleep 600 &\necho $! >> \"$1\"\nwait",
  );
  let running = Running::default();
  let held = running.clone();
  let mut command = Command::new(&testbed);
  command.arg("check");

  let step = std::thread::spawn(move || {
    let clocks = Clocks {
      silence: Duration::from_secs(3_600),
      ..quick()
    };

    run_step(command, "the static check", &clocks, &held, &|_| {})
  });

  assert!(soon(|| started.pids().len() == 3), "the stand-in started what it starts");

  let ended = running.take_down(&quick());

  assert!(ended.iter().any(|named| named.contains("sleep 600")), "{ended:?}");
  assert_eq!(
    step.join().expect("the step ends"),
    Err(StepEnd::Failed("the static check did not pass".to_owned()))
  );
  assert!(soon(|| started.all_gone()), "nothing the step started is left running");
}

#[test]
fn a_short_command_past_its_clock_is_ended_and_read_as_failed() {
  let mut command = Command::new("sleep");
  command.arg("600");

  let failed = output_within(&mut command, Duration::from_millis(100)).expect_err("it runs past its clock");

  assert_eq!(failed.kind(), std::io::ErrorKind::TimedOut);
  assert_eq!(failed.to_string(), "it ran past 100ms and was ended");
}

#[tokio::test]
async fn a_standing_the_service_never_answers_is_given_up_once_its_clock_has_passed() {
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a listener");
  let address = listener.local_addr().expect("its address");
  let silent = tokio::spawn(async move {
    let mut held = Vec::new();
    while let Ok((socket, _)) = listener.accept().await {
      held.push(socket);
    }
  });
  let client = bmo_job_client::JobClient::new(&format!("http://{address}"), "token").expect("a client");

  let written =
    keep_standing(&client, 7, &Standing::Testing, Duration::from_millis(300), std::future::pending()).await;

  silent.abort();
  assert_eq!(
    written,
    Err("item 7 was not written on: the job service gave no answer within 300ms".to_owned())
  );
}
