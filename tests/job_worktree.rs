//! What a checkout holds open (docs/queue.md, "A checkout held"): what was open before is somebody else's and
//! what opened since is the job's, a baseline read back for its own checkout and no other, and a baseline
//! from before untracked files were counted read as tracked alone.

use std::path::{Path, PathBuf};
use std::process::Command;

use bmo_job::job_run::checkout::Checkout;
use bmo_job::job_run::worktree::{
  Work, clear_baseline, new_work, nothing_open, preserved_work, read_worktree_state, record_baseline,
  recorded_baseline, worktree_state,
};
use serde_json::json;

struct Repo {
  dir: PathBuf,
}

impl Drop for Repo {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.dir);
  }
}

fn git(dir: &Path, args: &[&str]) {
  Command::new("git").args(args).current_dir(dir).output().unwrap();
}

fn repo() -> Repo {
  let dir = tempfile::tempdir().unwrap().keep();

  std::fs::create_dir_all(&dir).unwrap();
  git(&dir, &["init", "-q", "-b", "main"]);
  git(&dir, &["config", "user.name", "tester"]);
  git(&dir, &["config", "user.email", "tester@example.com"]);
  std::fs::write(dir.join("shared.ts"), "one\ntwo\nthree\n").unwrap();
  std::fs::write(dir.join("other.ts"), "a\n").unwrap();
  git(&dir, &["add", "."]);
  git(&dir, &["commit", "-qm", "first"]);

  Repo { dir }
}

#[test]
fn what_was_open_before_is_somebody_else_s_and_what_opened_since_is_the_job_s() {
  let repo = repo();
  let here = Checkout::local(repo.dir.to_string_lossy());

  std::fs::write(repo.dir.join("shared.ts"), "one\ntwo changed\nthree\n").unwrap();
  std::fs::write(repo.dir.join("notes.txt"), "theirs\n").unwrap();

  let before = worktree_state(&here).unwrap();

  std::fs::write(repo.dir.join("other.ts"), "b\n").unwrap();
  std::fs::write(repo.dir.join("scratch.txt"), "mine\n").unwrap();

  let after = worktree_state(&here).unwrap();

  assert_eq!(
    new_work(&before, &after),
    Work {
      tracked: vec!["other.ts".into()],
      untracked: vec!["scratch.txt".into()]
    }
  );
  assert_eq!(
    preserved_work(&before, &after),
    Work {
      tracked: vec!["shared.ts".into()],
      untracked: vec!["notes.txt".into()]
    }
  );

  // A line of the job's own added to somebody else's open file makes the file the job's too.
  std::fs::write(repo.dir.join("shared.ts"), "one\ntwo changed\nthree\nfour\n").unwrap();

  let later = worktree_state(&here).unwrap();

  assert!(new_work(&before, &later).tracked.contains(&"shared.ts".to_owned()));
}

#[test]
fn a_baseline_is_read_back_for_its_own_checkout_and_no_other() {
  let repo = repo();
  let data = repo.dir.join("data");
  let cwd = repo.dir.to_string_lossy().into_owned();
  let mut state = nothing_open();

  state.tracked.insert("a.ts".into(), "digest".into());
  state.untracked.push("b.txt".into());
  record_baseline(&data, &cwd, &state, 1);

  assert_eq!(recorded_baseline(&data, &cwd), state);
  assert_eq!(recorded_baseline(&data, "/somewhere/else"), nothing_open());

  clear_baseline(&data, &cwd);

  assert_eq!(recorded_baseline(&data, &cwd), nothing_open());
}

#[test]
fn a_baseline_from_before_untracked_files_were_counted_reads_as_tracked_alone() {
  let state = read_worktree_state(&json!({ "a.ts": "digest" }));

  assert_eq!(state.tracked.get("a.ts").map(String::as_str), Some("digest"));
  assert!(state.untracked.is_empty());
  assert_eq!(read_worktree_state(&json!(null)), nothing_open());
}
