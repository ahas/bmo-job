//! A job's close reading where its repository's deployment records what is live and handing its commit over
//! (docs/deploy.md, "A job closes"; docs/cli.md, "`bmo-job job close`"): the record its own deployment writes
//! under the root its declaration names, the range that record narrows to the job's own commits, and the
//! line the deployment reads.

use std::cell::RefCell;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::support;

use bmo_job::close_watch::{Running, StepEnd, output_within, run_step};
use bmo_job::job_close::{
  Clocks, Deployed, JobCloseArgs, Standing, Steps, close, commits_refusal, deployed_commit_of,
  keep_standing, wait_deployed,
};

use bmo_job_client::{EventQuery, NewItem};

// The clocks a test's close keeps to: its deployment's records read again at once, and a step's silence, the
// grace after a TERM and the lines saying where it stands each a fraction of a second.
fn quick() -> Clocks {
  Clocks {
    silence: Duration::from_millis(300),
    grace: Duration::from_millis(200),
    look: Duration::from_millis(10),
    standing_every: Duration::from_millis(100),
    look_again: Duration::from_millis(10),
  }
}

// A close's wait for its commit's deployment, from a second past the epoch, by the test's clocks.
fn waited(root: &Path, data_root: &Path, handed: &str) -> Deployed {
  wait_deployed(root, data_root, handed, 1_000, &quick(), &|_| {})
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

// A testbed standing in for the real one: it writes the verb it was asked on a line.
fn testbed_at(dir: &Path) -> (PathBuf, PathBuf) {
  let calls = dir.join("testbed-calls");
  let path = dir.join("bmo-testbed");
  fs::write(&path, format!("#!/bin/sh\necho \"$@\" >> \"{}\"\nexit 0\n", calls.display()))
    .expect("the testbed stand-in");
  let mut permissions = fs::metadata(&path).expect("the stand-in").permissions();
  permissions.set_mode(0o755);
  fs::set_permissions(&path, permissions).expect("the stand-in is executable");

  (path, calls)
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
  )
  .map(|(lines, _)| lines);

  (closed, told.into_inner(), said.into_inner())
}

// A testbed stand-in whose every verb fails.
fn failing_testbed_at(dir: &Path) -> PathBuf {
  let path = dir.join("bmo-testbed-failing");
  fs::write(&path, "#!/bin/sh\nexit 1\n").expect("the testbed stand-in");
  let mut permissions = fs::metadata(&path).expect("the stand-in").permissions();
  permissions.set_mode(0o755);
  fs::set_permissions(&path, permissions).expect("the stand-in is executable");

  path
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
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &head);

  let (testbed, calls) = testbed_at(data.path());
  let lines = run_close(repo.path(), data.path(), "Land the first page", None, &testbed).expect("clean");

  assert!(
    lines.iter().any(|line| line.contains("this commit is already what is deployed")),
    "{lines:?}"
  );
  assert!(!calls.exists(), "the close runs nothing on a commit already deployed");
}

#[test]
fn an_archive_that_keeps_no_pending_list_still_hands_the_commit_over() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &base);

  let (testbed, calls) = testbed_at(data.path());
  let lines =
    run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect("handed over");
  let pushed = format!("waiting to go live: {} stands pushed for its deployment", &head[..12]);

  assert!(lines.iter().any(|line| line == &pushed), "{lines:?}");
  let said = fs::read_to_string(calls).expect("the testbed stand-in's calls");
  assert!(said.contains("check"), "{said}");
  assert!(said.contains("suite"), "{said}");
  assert!(!said.contains("pending"), "{said}");
}

#[test]
fn a_close_tells_testing_and_then_the_commit_that_waits_to_go_live() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &base);

  let (testbed, _) = testbed_at(data.path());
  let (closed, told) = told_close(repo.path(), data.path(), "Land the second page", None, &testbed);

  closed.expect("handed over");
  assert_eq!(told, vec![Standing::Testing, Standing::WaitingLive(head)]);
}

#[test]
fn a_close_whose_check_fails_tells_testing_and_then_takes_it_off() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &base);

  let testbed = failing_testbed_at(data.path());
  let (closed, told) = told_close(repo.path(), data.path(), "Land the second page", None, &testbed);

  assert_eq!(closed.expect_err("the check fails"), "the static check did not pass");
  assert_eq!(told, vec![Standing::Testing, Standing::Off]);
}

#[test]
fn a_close_on_the_live_commit_and_a_refused_one_take_the_standing_off() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &base);

  let (testbed, _) = testbed_at(data.path());
  let (closed, told) = told_close(repo.path(), data.path(), "Land the first page", None, &testbed);

  closed.expect("clean");
  assert_eq!(told, vec![Standing::Off]);

  commit(repo.path(), "two.md", "Add the second page");
  let (closed, told) = told_close(repo.path(), data.path(), "Land the second page", None, &testbed);

  closed.expect_err("unpushed");
  assert_eq!(told, vec![Standing::Off]);
}

#[test]
fn an_unpushed_commit_is_refused_with_its_reason() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &base]);
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &base);

  let (testbed, calls) = testbed_at(data.path());
  let refused =
    run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect_err("unpushed");

  assert!(
    refused
      .contains(&format!("{} is not on origin/main; an unpushed commit is not deployable", &head[..12])),
    "{refused}"
  );
  assert!(!calls.exists(), "a refused close runs nothing");
}

#[test]
fn a_close_skipping_the_suite_runs_the_check_alone() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &base);

  let (testbed, calls) = testbed_at(data.path());
  let lines = run_close(
    repo.path(),
    data.path(),
    "Land the second page",
    Some("the change reaches no test"),
    &testbed,
  )
  .expect("handed over");

  assert!(lines.iter().any(|line| line.contains("stands pushed for its deployment")), "{lines:?}");
  let said = fs::read_to_string(calls).expect("the testbed stand-in's calls");
  assert!(said.contains("check"), "{said}");
  assert!(!said.contains("suite"), "{said}");
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
fn a_foreign_record_falls_back_to_the_job_s_own_base_and_hands_the_commit_over() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  platform_record_at(data.path(), FOREIGN);

  let (testbed, calls) = testbed_at(data.path());
  let lines =
    run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect("handed over");
  let pushed = format!("waiting to go live: {} stands pushed for its deployment", &head[..12]);
  let range = format!("the range is {}..{}", &head[..12], &head[..12]);

  assert!(lines.iter().any(|line| line == &pushed), "{lines:?}");
  assert!(
    lines.iter().any(|line| line.contains(&FOREIGN[..12]) && line.contains(&range)),
    "{lines:?}"
  );
  let said = fs::read_to_string(calls).expect("the testbed stand-in's calls");
  assert!(said.contains("check"), "{said}");
  assert!(said.contains("suite"), "{said}");

  // The job's own base is the last commit HEAD shares with the branch it lands on, so the range is
  // established without the foreign commit and the commit it is measured from is this repository's.
  assert_ne!(&base[..12], &head[..12]);
}

#[test]
fn a_foreign_record_does_not_read_an_earlier_hangul_commit_as_the_job_s_own() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  commit(repo.path(), "old.md", &format!("Add the old page\n\nAs asked: {}", '\u{AC00}'));
  commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  platform_record_at(data.path(), FOREIGN);

  let (testbed, _calls) = testbed_at(data.path());
  let lines =
    run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect("handed over");

  assert!(lines.iter().any(|line| line.contains("stands pushed for its deployment")), "{lines:?}");
}

#[test]
fn a_record_commit_on_another_branch_is_not_taken_as_the_base() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  git(repo.path(), &["checkout", "-q", "-b", "other", &base]);
  let elsewhere = commit(repo.path(), "other.md", "Add the other page");
  git(repo.path(), &["checkout", "-q", "main"]);
  platform_record_at(data.path(), &elsewhere);

  let (testbed, _calls) = testbed_at(data.path());
  let lines =
    run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect("handed over");
  let pushed = format!("waiting to go live: {} stands pushed for its deployment", &head[..12]);

  assert!(lines.iter().any(|line| line == &pushed), "{lines:?}");
  assert!(lines.iter().any(|line| line.contains(&elsewhere[..12])), "{lines:?}");
}

#[test]
fn an_unpushed_commit_is_refused_while_the_record_is_foreign() {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &base]);
  platform_record_at(data.path(), FOREIGN);

  let (testbed, calls) = testbed_at(data.path());
  let refused =
    run_close(repo.path(), data.path(), "Land the second page", None, &testbed).expect_err("unpushed");

  assert!(
    refused
      .contains(&format!("{} is not on origin/main; an unpushed commit is not deployable", &head[..12])),
    "{refused}"
  );
  assert!(!calls.exists(), "a refused close runs nothing");
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

// A repository whose declaration names no deployment, a base library, is never put live: the close ends at once.
#[test]
fn a_close_in_a_repository_that_deploys_nothing_ends_live_at_once() {
  let repo = tempfile::tempdir().expect("a checkout");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());

  let handed = commit(repo.path(), "a.txt", "First");

  fs::write(repo.path().join("testbed.toml"), "testbed = 0\nname = \"the-library\"\n")
    .expect("the declaration");

  match waited(repo.path(), data.path(), &handed) {
    Deployed::Live(lines) => {
      assert!(lines.join("\n").contains("this repository deploys nothing"), "{lines:?}")
    }
    other => panic!("{other:?}"),
  }
}

// A close waits on its commit's deployment and ends with what it came to: live where the deployment carrying the
// commit ended live, and not live, with the deployment's own words, where it failed or rolled back.
#[test]
fn a_waiting_close_ends_with_how_the_deployment_carrying_its_commit_ended() {
  let repo = tempfile::tempdir().expect("a checkout");
  let deploy = tempfile::tempdir().expect("a deploy root");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  commit(repo.path(), "a.txt", "First");

  let handed = commit(repo.path(), "b.txt", "Second");
  let later = commit(repo.path(), "c.txt", "Third");

  declare_deploy_root(repo.path(), deploy.path());

  let progress = |commit: &str, started: i64, ending: &str| {
    fs::write(
      deploy.path().join("deploy-progress.json"),
      format!(r#"{{"unit":"bmo-deploy-1","commit":"{commit}","startedAt":{started},"ending":{ending}}}"#),
    )
    .expect("the progress record");
  };

  progress(&later, 2_000, r#"{"outcome":"live","at":3000,"why":null}"#);
  assert!(
    matches!(waited(repo.path(), data.path(), &handed), Deployed::Live(_)),
    "a later commit carrying it went live"
  );

  progress(
    &handed,
    2_000,
    r#"{"outcome":"rolled-back","at":3000,"why":"the step register-tool after the health failed"}"#,
  );

  match waited(repo.path(), data.path(), &handed) {
    Deployed::NotLive(lines) => {
      let said = lines.join("\n");

      assert!(
        said.contains("ended rolled-back: the step register-tool after the health failed"),
        "{said}"
      );
      assert!(said.contains("journalctl --user -u bmo-deploy-1"), "{said}");
    }
    other => panic!("not live expected: {other:?}"),
  }

  // A deployment that began before the close handed its commit over says nothing of it; the record of what is
  // live does, once no deployment is under way.
  progress(&later, 500, r#"{"outcome":"failed","at":900,"why":"an older run"}"#);
  record_at(deploy.path(), &handed);
  assert!(matches!(waited(repo.path(), data.path(), &handed), Deployed::Live(_)));
}

#[test]
fn a_deployment_whose_run_is_gone_with_no_ending_is_cut_off_and_not_live() {
  let repo = tempfile::tempdir().expect("a checkout");
  let deploy = tempfile::tempdir().expect("a deploy root");
  let data = tempfile::tempdir().expect("a data root");

  init(repo.path());
  commit(repo.path(), "a.txt", "First");

  let handed = commit(repo.path(), "b.txt", "Second");

  declare_deploy_root(repo.path(), deploy.path());

  // A process that has ended stands for a deploy run that died before it wrote an ending.
  let mut ended = Command::new("true").spawn().expect("a process");
  let gone = ended.id();

  ended.wait().expect("it ends");
  fs::write(
    deploy.path().join("deploy-progress.json"),
    format!(r#"{{"pid":{gone},"commit":"{handed}","startedAt":2000,"ending":null}}"#),
  )
  .expect("the progress record");

  match waited(repo.path(), data.path(), &handed) {
    Deployed::NotLive(lines) => {
      let said = lines.join("\n");

      assert!(said.contains("was cut off"), "{said}");
      assert!(said.contains("close again"), "{said}");
    }
    other => panic!("not live expected: {other:?}"),
  }
}

// A process standing for a deploy run, taken down when it is dropped.
struct DeployRun(std::process::Child);

impl Drop for DeployRun {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

// A user unit standing for a deploy run's own, stopped when it is dropped.
struct DeployUnit(String);

impl Drop for DeployUnit {
  fn drop(&mut self) {
    let _ = Command::new("systemctl").args(["--user", "stop", &self.0]).output();
  }
}

// A checkout whose commit was handed over, its deploy root declared, and the wait on it running on a thread of
// its own, its ending sent once it comes.
fn waiting_on(
  progress: impl Fn(&str) -> String,
) -> (tempfile::TempDir, tempfile::TempDir, std::sync::mpsc::Receiver<Deployed>) {
  let repo = tempfile::tempdir().expect("a checkout");
  let deploy = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  commit(repo.path(), "a.txt", "First");

  let handed = commit(repo.path(), "b.txt", "Second");

  declare_deploy_root(repo.path(), deploy.path());
  fs::write(deploy.path().join("deploy-progress.json"), progress(&handed)).expect("the progress record");

  let (send, ended) = std::sync::mpsc::channel();
  let root = repo.path().to_path_buf();
  let data = deploy.path().to_path_buf();

  std::thread::spawn(move || {
    let _ = send.send(waited(&root, &data, &handed));
  });

  (repo, deploy, ended)
}

fn cut_off(ended: &std::sync::mpsc::Receiver<Deployed>) {
  match ended.recv_timeout(Duration::from_secs(30)).expect("the wait ends once the run is gone") {
    Deployed::NotLive(lines) => assert!(lines.join("\n").contains("was cut off"), "{lines:?}"),
    other => panic!("not live expected: {other:?}"),
  }
}

#[test]
fn a_deploy_run_stopped_in_the_middle_of_the_wait_ends_it_cut_off() {
  let mut run = DeployRun(Command::new("sleep").arg("600").spawn().expect("a deploy run"));
  let pid = run.0.id();
  let (_repo, _deploy, ended) =
    waiting_on(|handed| format!(r#"{{"pid":{pid},"commit":"{handed}","startedAt":2000,"ending":null}}"#));

  assert!(
    ended.recv_timeout(Duration::from_millis(200)).is_err(),
    "a run still standing is waited on"
  );

  run.0.kill().expect("the run is stopped");
  run.0.wait().expect("and gone");
  cut_off(&ended);
}

#[test]
fn a_deploy_unit_stopped_in_the_middle_of_the_wait_ends_it_cut_off() {
  let name = format!("bmo-job-test-deploy-{}", std::process::id());
  let started = Command::new("systemd-run")
    .args(["--user", "--quiet", "--unit", &name, "sleep", "600"])
    .output();

  if !started.as_ref().is_ok_and(|output| output.status.success()) {
    eprintln!("no user service manager answers here, so no unit can stand for a deploy run: {started:?}");
    return;
  }

  let unit = DeployUnit(name);
  let (_repo, _deploy, ended) = waiting_on(|handed| {
    format!(r#"{{"unit":"{}","commit":"{handed}","startedAt":2000,"ending":null}}"#, unit.0)
  });

  assert!(
    ended.recv_timeout(Duration::from_millis(200)).is_err(),
    "a unit still active is waited on"
  );

  let stopped = Command::new("systemctl")
    .args(["--user", "stop", &unit.0])
    .output()
    .expect("systemctl runs");

  assert!(stopped.status.success(), "{stopped:?}");
  cut_off(&ended);
}

#[tokio::test]
async fn the_close_writes_its_standing_through_a_job_service_restart_and_hears_its_run_cut_off() {
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
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy = tempfile::tempdir().expect("a deploy root");
  let tools = tempfile::tempdir().expect("a directory for the testbed stand-in");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  declare_deploy_root(repo.path(), deploy.path());
  record_at(deploy.path(), &base);

  // The deployment that takes the commit is already under way, and its run died before it wrote an ending.
  let mut gone = Command::new("true").spawn().expect("a process");
  let pid = gone.id();
  gone.wait().expect("it ends");
  let later = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .expect("the time")
    .as_millis()
    + 3_600_000;
  fs::write(
    deploy.path().join("deploy-progress.json"),
    format!(r#"{{"pid":{pid},"commit":"{head}","startedAt":{later},"ending":null}}"#),
  )
  .expect("the progress record");

  let (_, calls) = testbed_at(tools.path());
  let path = format!("{}:{}", tools.path().display(), std::env::var("PATH").unwrap_or_default());

  relay.go_down();

  let child = tokio::process::Command::new(env!("CARGO_BIN_EXE_bmo-job"))
    .args(["job", "close", "--then", "Land the second page", "--todo", &item.to_string()])
    .current_dir(repo.path())
    .env("BMO_JOB_URL", &relay.url)
    .env("BMO_JOB_TOKEN", support::TOKEN)
    .env("BMO_DATA_DIR", data.path())
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
  let said = String::from_utf8_lossy(&output.stderr);

  assert_eq!(output.status.code(), Some(1), "{said}");
  assert!(
    String::from_utf8_lossy(&output.stdout).trim_end().ends_with("job-close: ended, exit 1"),
    "the close's last line says it ended"
  );
  assert!(said.contains("asking again"), "{said}");
  assert!(said.contains("was cut off"), "{said}");
  assert_eq!(fs::read_to_string(&calls).expect("the testbed's calls"), "check\nsuite\n");

  let standing = client.item(item).await.expect("the item").labels;

  assert!(!standing.contains_key("close-phase"), "{standing:?}");

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

  assert_eq!(phases, ["testing", "waiting-live"]);
  assert!(changes.iter().any(|event| {
    event
      .data
      .get("labels")
      .and_then(|labels| labels.get("close-commit"))
      .and_then(|commit| commit.as_str())
      == Some(head.as_str())
  }));
}

// A testbed stand-in whose check passes and whose suite is `suite`, a shell body run with the file the test
// keeps the stand-in's processes in as `$1`.
fn suite_at(dir: &Path, suite: &str) -> (PathBuf, Started) {
  let path = dir.join("bmo-testbed-suite");
  let pids = dir.join("stand-in-pids");

  fs::write(
    &path,
    format!(
      "#!/bin/sh\n[ \"$1\" = suite ] || exit 0\nset -- \"{}\"\necho $$ >> \"$1\"\n{suite}\n",
      pids.display()
    ),
  )
  .expect("the testbed stand-in");
  let mut permissions = fs::metadata(&path).expect("the stand-in").permissions();
  permissions.set_mode(0o755);
  fs::set_permissions(&path, permissions).expect("the stand-in is executable");

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

// A checkout whose second commit is pushed and waits to be closed, its deploy root naming the first as live.
fn closable() -> (tempfile::TempDir, tempfile::TempDir, tempfile::TempDir, String) {
  let repo = tempfile::tempdir().expect("a repository");
  let data = tempfile::tempdir().expect("a data root");
  let deploy_root = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let base = commit(repo.path(), "one.md", "Add the first page");
  let head = commit(repo.path(), "two.md", "Add the second page");
  git(repo.path(), &["update-ref", "refs/remotes/origin/main", &head]);
  declare_deploy_root(repo.path(), deploy_root.path());
  record_at(deploy_root.path(), &base);

  (repo, data, deploy_root, head)
}

#[test]
fn a_suite_that_falls_silent_ends_the_close_with_its_last_line_what_ran_and_the_checkout_as_found() {
  let (repo, data, _deploy_root, _) = closable();
  let (testbed, started) = suite_at(
    data.path(),
    "echo 'unit agency-checks started'\nsetsid sleep 600 &\necho $! >> \"$1\"\nsleep 600 &\necho $! >> \"$1\"\nwait",
  );
  let (closed, told, _) = said_close(repo.path(), data.path(), "Land the second page", None, &testbed);
  let why = closed.expect_err("a silent suite refuses the close");

  assert!(why.starts_with("the suite wrote nothing for 300ms and was ended"), "{why}");
  assert!(why.contains("its last line: unit agency-checks started"), "{why}");
  assert!(why.contains("still running: ") && why.contains("sleep 600"), "{why}");
  assert!(why.ends_with("the checkout stands as the close found it"), "{why}");
  assert_eq!(told, vec![Standing::Testing, Standing::Off]);
  assert_eq!(
    started.pids().len(),
    3,
    "the stand-in, its own process and the one in a session of its own"
  );
  assert!(soon(|| started.all_gone()), "nothing the suite started is left running");
}

#[test]
fn a_suite_whose_output_is_held_back_but_heard_runs_past_the_silence_and_says_where_it_stands() {
  let (repo, data, _deploy_root, head) = closable();
  let (testbed, _started) = suite_at(
    data.path(),
    "for i in 1 2 3 4 5 6; do touch \"$BMO_TESTBED_HEARD\"; sleep 0.1; done\necho 'suite: ok'",
  );
  let (closed, told, said) = said_close(repo.path(), data.path(), "Land the second page", None, &testbed);

  closed.expect("a suite that is heard is not ended");
  assert_eq!(told, vec![Standing::Testing, Standing::WaitingLive(head)]);
  assert!(
    said.iter().any(|line| line.starts_with("job-close: the suite has run ")
      && line.contains("its output last moved")
      && line.ends_with(": nothing written")),
    "{said:?}"
  );
}

#[test]
fn a_stop_ends_the_step_running_with_everything_under_it() {
  let dir = tempfile::tempdir().expect("a directory");
  let (testbed, started) = suite_at(
    dir.path(),
    "setsid sleep 600 &\necho $! >> \"$1\"\nsleep 600 &\necho $! >> \"$1\"\nwait",
  );
  let running = Running::default();
  let held = running.clone();
  let mut command = Command::new(&testbed);
  command.arg("suite");

  let step = std::thread::spawn(move || {
    let clocks = Clocks {
      silence: Duration::from_secs(3_600),
      ..quick()
    };

    run_step(command, "the suite", &clocks, &held, &|_| {})
  });

  assert!(soon(|| started.pids().len() == 3), "the stand-in started what it starts");

  let ended = running.take_down(&quick());

  assert!(ended.iter().any(|named| named.contains("sleep 600")), "{ended:?}");
  assert_eq!(
    step.join().expect("the step ends"),
    Err(StepEnd::Failed("the suite did not pass".to_owned()))
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

#[test]
fn a_close_waiting_for_its_deployment_says_how_it_stands_until_it_goes_live() {
  let repo = tempfile::tempdir().expect("a checkout");
  let deploy = tempfile::tempdir().expect("a deploy root");

  init(repo.path());
  let handed = commit(repo.path(), "a.txt", "First");
  let short: String = handed.chars().take(12).collect();

  declare_deploy_root(repo.path(), deploy.path());

  let said = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
  let (root, data, asked, heard) =
    (repo.path().to_path_buf(), deploy.path().to_path_buf(), handed.clone(), said.clone());
  let wait = std::thread::spawn(move || {
    wait_deployed(&root, &data, &asked, 1_000, &quick(), &|line| {
      heard.lock().expect("the lines").push(line.to_owned())
    })
  });

  assert!(
    soon(|| !said.lock().expect("the lines").is_empty()),
    "a waiting close says how it stands"
  );
  fs::write(
    deploy.path().join("deploy-progress.json"),
    format!(r#"{{"pid":{},"commit":"{handed}","startedAt":2000,"ending":null}}"#, std::process::id()),
  )
  .expect("the progress record");
  assert!(
    soon(|| said
      .lock()
      .expect("the lines")
      .iter()
      .any(|line| line.ends_with("carrying it is under way"))),
    "a deployment under way is named"
  );
  fs::write(
    deploy.path().join("deploy-progress.json"),
    format!(
      r#"{{"pid":{},"commit":"{handed}","startedAt":2000,"ending":{{"outcome":"live"}}}}"#,
      std::process::id()
    ),
  )
  .expect("the progress record");

  assert!(matches!(wait.join().expect("the wait ends"), Deployed::Live(_)));
  assert!(
    said.lock().expect("the lines")[0]
      .starts_with(&format!("job-close: waiting for the deployment of {short} for ")),
    "{said:?}"
  );
  assert!(said.lock().expect("the lines")[0].ends_with(": no deployment carrying it has begun"));
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
