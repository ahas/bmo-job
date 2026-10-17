//! A job's checkout (docs/queue.md, "A checkout held"): a job's branch made from main with main written
//! beside it, what the last job left open parked on its branch, a base checkout put back on main, a job's own
//! commits, submodules brought to their pins, and the words and paths a checkout's commands are given.

use std::path::{Path, PathBuf};
use std::process::Command;

use bmo_job::job_run::checkout::{
  BranchPrepared, Checkout, clear_landed, keyed_path, landed_commits, resolved, shell_word,
};

struct Repo {
  dir: PathBuf,
}

impl Drop for Repo {
  fn drop(&mut self) {
    let _ = std::fs::remove_dir_all(&self.dir);
  }
}

fn git(dir: &Path, args: &[&str]) -> String {
  let out = Command::new("git")
    .args(args)
    .current_dir(dir)
    .env("GIT_AUTHOR_NAME", "tester")
    .env("GIT_AUTHOR_EMAIL", "tester@example.com")
    .env("GIT_COMMITTER_NAME", "tester")
    .env("GIT_COMMITTER_EMAIL", "tester@example.com")
    .output()
    .unwrap();

  String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn repo() -> Repo {
  let dir = tempfile::tempdir().unwrap().keep();

  std::fs::create_dir_all(&dir).unwrap();
  git(&dir, &["init", "-q", "-b", "main"]);
  git(&dir, &["config", "user.name", "tester"]);
  git(&dir, &["config", "user.email", "tester@example.com"]);
  std::fs::write(dir.join("kept.ts"), "one\n").unwrap();
  git(&dir, &["add", "kept.ts"]);
  git(&dir, &["commit", "-qm", "first"]);

  Repo { dir }
}

#[test]
fn a_job_branch_is_made_from_main_and_main_is_written_beside_it() {
  let repo = repo();
  let here = Checkout::local(repo.dir.to_string_lossy());
  let prepared = here.prepare_branch("work/7", "main", "parked", "bun").unwrap();

  assert_eq!(
    prepared,
    BranchPrepared {
      parked: None,
      made: true,
      deleted: None
    }
  );
  assert_eq!(here.current_branch().as_deref(), Some("work/7"));
  assert_eq!(git(&repo.dir, &["config", "--get", "branch.work/7.bmo-main"]), "main");
  assert_eq!(git(&repo.dir, &["config", "--get", "branch.work/7.bmo-from"]), "main");
  assert_eq!(here.main_branch(), "main");
}

#[test]
fn what_the_last_job_left_open_is_parked_on_its_branch_and_a_landed_branch_deleted() {
  let repo = repo();
  let here = Checkout::local(repo.dir.to_string_lossy());

  here.prepare_branch("work/1", "main", "parked", "bun").unwrap();
  std::fs::write(repo.dir.join("open.ts"), "left\n").unwrap();

  let prepared = here.prepare_branch("work/2", "main", "parked by the next job", "bun").unwrap();
  let parked = prepared.parked.unwrap();

  assert_eq!((parked.branch.as_str(), parked.paths.clone()), ("work/1", vec!["open.ts".to_owned()]));
  assert_eq!(git(&repo.dir, &["log", "-1", "--format=%s", "work/1"]), "parked by the next job");
  // The parked branch holds a commit main lacks, so it stays.
  assert_eq!(prepared.deleted, None);

  git(&repo.dir, &["switch", "-q", "main"]);
  git(&repo.dir, &["switch", "-q", "-c", "work/3"]);

  let landed = here.prepare_branch("work/4", "main", "parked", "bun").unwrap();

  assert_eq!(landed.deleted.as_deref(), Some("work/3"));
}

#[test]
fn a_base_checkout_goes_back_on_main_with_its_own_paths_parked_and_others_carried() {
  let repo = repo();
  let here = Checkout::local(repo.dir.to_string_lossy());

  std::fs::write(repo.dir.join("someone.ts"), "theirs\n").unwrap();
  here.prepare_branch("work/9", "main", "parked", "bun").unwrap();
  std::fs::write(repo.dir.join("mine.ts"), "mine\n").unwrap();

  let back = here.restore_main("main", &["mine.ts".to_owned()], "left by the job");

  assert!(back.on_main, "{back:?}");
  assert_eq!(
    back.parked.as_ref().map(|parked| parked.paths.clone()),
    Some(vec!["mine.ts".to_owned()])
  );
  assert_eq!(here.current_branch().as_deref(), Some("main"));
  assert!(repo.dir.join("someone.ts").exists());
  assert!(!repo.dir.join("mine.ts").exists());
  assert_eq!(back.deleted, None);
}

#[test]
fn a_job_s_own_commits_are_what_its_branch_holds_past_main_and_what_it_landed() {
  let repo = repo();
  let here = Checkout::local(repo.dir.to_string_lossy());
  let data = repo.dir.join("data");

  here.prepare_branch("work/5", "main", "parked", "bun").unwrap();
  std::fs::write(repo.dir.join("a.ts"), "a\n").unwrap();
  git(&repo.dir, &["add", "a.ts"]);
  git(&repo.dir, &["commit", "-qm", "a"]);
  std::fs::create_dir_all(data.join("worktree")).unwrap();
  std::fs::write(keyed_path(&data, &here.cwd, "-landed"), r#"["landed-one"]"#).unwrap();

  let own = here.own_branch_commits(&data, "main");

  assert_eq!(own.len(), 2);
  assert_eq!(own[0], "landed-one");
  assert_eq!(here.commit_files(&own[1..]), vec!["a.ts".to_owned()]);
  clear_landed(&data, &here.cwd);
  assert!(landed_commits(&data, &here.cwd).is_empty());
}

#[test]
fn a_submodule_is_brought_to_its_pin_over_what_an_older_commit_left_in_its_folder() {
  let source = repo();
  let top = repo();
  let here = Checkout::local(top.dir.to_string_lossy());
  let sub = top.dir.join("sub");

  std::fs::create_dir_all(&sub).unwrap();
  std::fs::write(sub.join("old.ts"), "old\n").unwrap();
  std::fs::write(top.dir.join(".gitignore"), "node_modules/\n").unwrap();
  git(&top.dir, &["add", "-A"]);
  git(&top.dir, &["commit", "-qm", "a folder"]);

  let folder = git(&top.dir, &["rev-parse", "HEAD"]);

  git(&top.dir, &["rm", "-rq", "sub"]);
  // The submodule's repository is absorbed into this one's as it is added, so bringing it later clones
  // nothing.
  git(
    &top.dir,
    &[
      "-c",
      "protocol.file.allow=always",
      "submodule",
      "add",
      "-q",
      &source.dir.to_string_lossy(),
      "sub",
    ],
  );
  git(&top.dir, &["commit", "-qm", "a submodule"]);

  let pin = git(&top.dir, &["rev-parse", "HEAD"]);

  // A tree on the older commit, its folder holding an install, that never held the submodule.
  git(&top.dir, &["checkout", "-q", "--force", "--detach", &folder]);
  std::fs::remove_file(sub.join(".git")).unwrap();
  std::fs::create_dir_all(sub.join("node_modules")).unwrap();
  std::fs::write(sub.join("node_modules/x.js"), "x\n").unwrap();
  git(&top.dir, &["checkout", "-q", "--force", "--detach", &pin]);
  assert!(sub.join("node_modules/x.js").exists());

  here.bring_submodules(true).unwrap();

  assert!(sub.join(".git").exists());
  assert!(sub.join("kept.ts").exists());
  assert!(!sub.join("node_modules").exists());

  // Back over the folder and forward again: the forced move took the submodule's files, and the pin is
  // checked out again although the submodule's own HEAD never moved.
  git(&top.dir, &["checkout", "-q", "--force", "--detach", &folder]);
  git(&top.dir, &["checkout", "-q", "--force", "--detach", &pin]);
  here.bring_submodules(true).unwrap();

  assert!(sub.join("kept.ts").exists());
  assert_eq!(git(&top.dir, &["status", "--porcelain"]), "");
}

#[test]
fn a_word_is_quoted_only_where_a_shell_would_read_it_otherwise() {
  assert_eq!(shell_word("plain/path-1.ts"), "plain/path-1.ts");
  assert_eq!(shell_word("it's here"), r#"'it'"'"'s here'"#);
  assert_eq!(shell_word(""), "''");
}

#[test]
fn a_path_resolves_as_the_runtime_resolves_it() {
  assert_eq!(resolved("/a/b/../c/./d/"), "/a/c/d");
}
