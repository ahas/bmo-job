//! A job's close (docs/verbs.md, "A job's close"): the commit it closed on written down as waiting to go live
//! through bmo-testbed, whose record says whether it is urgent; a reason or a commit of the job's own
//! holding Hangul refused before anything is checked or written down; an English one passing with the names of
//! address among its words — on a stand of the test's own and a repository of the test's own.

use std::path::Path;

use bmo_job::job_run::checkout::Checkout;
use bmo_job::verbs::job_close::{JobCloseArgs, commits_refusal, job_close, reasons_refusal};
use bmo_job::verbs::node_land::record_pending;
use bmo_job::verbs::stand::{JobProcess, RunningJob, VerbError};
use serde_json::json;

use crate::support::desk::Desk;

// A syllable of a script a commit must not hold, made from its code point so the test's own words stay
// English, and a name of address written in it.
fn syllable(at: u32) -> char {
  char::from_u32(0xAC00 + at * 29).unwrap()
}

fn owner_name() -> String {
  [syllable(3), syllable(7)].into_iter().collect()
}

fn git(cwd: &Path, args: &[&str]) -> String {
  let done = std::process::Command::new("git")
    .args(args)
    .current_dir(cwd)
    .env("GIT_AUTHOR_NAME", "tester")
    .env("GIT_AUTHOR_EMAIL", "tester@example.invalid")
    .env("GIT_COMMITTER_NAME", "tester")
    .env("GIT_COMMITTER_EMAIL", "tester@example.invalid")
    .output()
    .unwrap();

  assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
  String::from_utf8_lossy(&done.stdout).trim().to_owned()
}

fn commit_saying(cwd: &Path, file: &str, message: &str) -> String {
  std::fs::write(cwd.join(file), message).unwrap();
  git(cwd, &["add", "-A"]);
  git(cwd, &["commit", "-qm", message]);
  git(cwd, &["rev-parse", "--short", "HEAD"])
}

// A job's branch: main with one commit, and `work/1` made from it.
fn branch() -> tempfile::TempDir {
  let repo = tempfile::tempdir().unwrap();

  git(repo.path(), &["init", "-q", "-b", "main"]);
  commit_saying(repo.path(), "README.md", "Start the repository");
  git(repo.path(), &["switch", "-qc", "work/1"]);
  repo
}

// The desk whose owner is named in the script the close sets aside as a name of address.
async fn desk() -> Desk {
  let name = owner_name();

  Desk::edited(move |_, text| text.replace("owner_name = \"Owner\"", &format!("owner_name = \"{name}\"")))
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_commit_is_written_down_through_the_testbed_and_its_record_said() {
  let desk = desk().await;
  let checkout = desk.testbed.folder("checkout");
  let cwd = checkout.display().to_string();
  let job = RunningJob {
    id: 20,
    todo_id: Some(2),
    worker: Some("worker".into()),
  };
  let commit = "0123456789abcdef0123456789abcdef01234567";

  desk.testbed.answer(
    "pending-add",
    &json!({
      "repository": "job", "commit": commit, "item": 2, "worker": "worker", "job": 20,
      "reason": "dropped a column", "urgent": true, "created_at": 1,
    })
    .to_string(),
    "",
    0,
  );

  let said = record_pending(&desk.stand, &cwd, commit, Some(&job), None, "  dropped a column ").await;
  let run = desk.testbed.run();

  assert_eq!(
    run.words,
    [
      "pending",
      "add",
      "--commit",
      commit,
      "--item",
      "2",
      "--worker",
      "worker",
      "--job",
      "20",
      "--reason",
      "dropped a column",
      "--json"
    ]
  );
  assert_eq!(run.dir.canonicalize().unwrap(), checkout.canonicalize().unwrap());
  // The urgency is the record's, read off the commit by bmo-testbed, never the job's word.
  assert_eq!(said, "waiting to go live: 0123456789ab (#2 · worker · urgent) — dropped a column");

  // A record bmo-testbed refused is said as not written down.
  desk.testbed.answer("pending-add", "", "no such repository", 1);
  assert_eq!(
    record_pending(&desk.stand, &cwd, commit, None, Some("worker"), "why").await,
    "the commit could not be written down"
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_reason_holding_hangul_is_refused_before_anything_is_checked_or_written_down() {
  let desk = desk().await;
  let repo = branch();
  let korean = format!("Land the {} fix", syllable(0));
  let close = |then: &str, skip: Option<&str>| JobCloseArgs {
    then: Some(then.to_owned()),
    skip_suite: skip.map(str::to_owned),
    in_line: false,
  };
  let refused = |answer: Result<_, VerbError>| match answer {
    Err(VerbError::Ended { code: 1, said }) => said,
    _ => panic!("the close was not refused"),
  };

  let said =
    refused(job_close(&desk.stand, &JobProcess::default(), close(&korean, None), repo.path()).await);

  assert!(said.contains("--then"), "{said}");

  let said = refused(
    job_close(&desk.stand, &JobProcess::default(), close("Land the fix", Some(&korean)), repo.path()).await,
  );

  assert!(said.contains("--skip-suite"), "{said}");
  assert!(desk.testbed.runs().is_empty(), "nothing was checked or written down");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_commit_holding_hangul_is_refused_by_its_hash_against_main() {
  let desk = desk().await;
  let repo = branch();
  let english = commit_saying(repo.path(), "one.md", "Add the first page");
  let korean =
    commit_saying(repo.path(), "two.md", &format!("Add the second page\n\nAs asked: {}", syllable(0)));
  let checkout = Checkout::local(repo.path().display().to_string());
  let said = commits_refusal(&desk.stand.policy, &checkout, &["main".to_owned()], "main").unwrap();

  assert!(said.contains(&format!("{korean} Add the second page")), "{said}");
  assert!(!said.contains(&english), "{said}");
  assert!(said.contains("main"), "{said}");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_english_commit_and_reason_pass_with_a_name_of_address_among_their_words() {
  let desk = desk().await;
  let repo = branch();
  let name = owner_name();

  commit_saying(repo.path(), "page.md", &format!("Add the page {name} asked for"));

  let then = format!("Put the page {name} asked for live");
  let args = JobCloseArgs {
    then: Some(then.clone()),
    skip_suite: Some(then),
    in_line: false,
  };
  let checkout = Checkout::local(repo.path().display().to_string());

  assert_eq!(reasons_refusal(&desk.stand.policy, &args), None);
  assert_eq!(commits_refusal(&desk.stand.policy, &checkout, &["main".to_owned()], "main"), None);
  assert_eq!(checkout.messages_past(&["main".to_owned()]).len(), 1);
}
