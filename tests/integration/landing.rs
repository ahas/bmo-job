//! A job's branch landed on main (docs/runs.md, "Landing on main"; docs/verbs.md, "Landing a branch"): over a
//! base checkout with a remote of its own and a support worker's worktree beside it, each a scratch
//! repository of the test's own, and the units linked into the checkout main stands in asked of the
//! bmo-testbed stand-in.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;

use bmo_job::job_run::checkout::landing::{LandOptions, LandRefusal, LandSetUp, Landed};
use bmo_job::job_run::checkout::units::{CheckoutUnits, TestbedLinks};
use bmo_job::job_run::checkout::{Checkout, MainRestored, landed_commits};
use bmo_job::queue::workers::Worker;
use bmo_job::verbs::land::{landing_branch_of, pushed_from_nodes};
use bmo_job::verbs::node_land::setup_inputs_moved;
use job_policy::Policy;
use serde_json::json;

use crate::support::queue::policy;
use crate::support::testbed;

fn git(cwd: &Path, args: &[&str]) -> String {
  let done = Command::new("git")
    .args(args)
    .current_dir(cwd)
    .env("GIT_CONFIG_GLOBAL", "/dev/null")
    .env("GIT_AUTHOR_NAME", "tester")
    .env("GIT_AUTHOR_EMAIL", "tester@example.invalid")
    .env("GIT_COMMITTER_NAME", "tester")
    .env("GIT_COMMITTER_EMAIL", "tester@example.invalid")
    .output()
    .unwrap();

  assert!(done.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&done.stderr));
  String::from_utf8_lossy(&done.stdout).trim().to_owned()
}

fn git_fails(cwd: &Path, args: &[&str]) -> bool {
  !Command::new("git").args(args).current_dir(cwd).output().unwrap().status.success()
}

fn commit(cwd: &Path, file: &str, text: &str, message: &str) -> String {
  let path = cwd.join(file);

  std::fs::create_dir_all(path.parent().unwrap()).unwrap();
  std::fs::write(path, text).unwrap();
  git(cwd, &["add", file]);
  git(cwd, &["commit", "-qm", message]);
  git(cwd, &["rev-parse", "HEAD"])
}

fn read(path: PathBuf) -> String {
  std::fs::read_to_string(path).unwrap()
}

// A base checkout on main with a remote to push to, the folder the landing writes down what it landed in,
// and where its first support worker's worktree stands.
struct Repo {
  _dir: tempfile::TempDir,
  base: PathBuf,
  remote: PathBuf,
  data: PathBuf,
  policy: Policy,
}

impl Repo {
  fn support(&self) -> PathBuf {
    PathBuf::from(format!("{}-w2", self.base.display()))
  }

  fn here(&self, cwd: &Path) -> Checkout {
    Checkout::local(cwd.to_string_lossy())
  }

  // The worker rows of the base checkout and its first support worker.
  fn rows(&self) -> Vec<Worker> {
    let worker = |name: &str, cwd: &Path, support_of: Option<&str>| Worker {
      name: name.to_owned(),
      label: name.to_owned(),
      cwd: cwd.to_string_lossy().into_owned(),
      builtin: false,
      preamble: None,
      dev_command: None,
      support_count: i64::from(support_of.is_none()),
      support_keepalive: 0,
      support_of: support_of.map(str::to_owned),
      support_slot: support_of.map(|_| 2),
      node: None,
    };

    vec![worker("bench", &self.base, None), worker("bench-w2", &self.support(), Some("bench"))]
  }

  // The support worktree made from main and put on a job's branch.
  fn support_on(&self, branch: &str, main: &str) -> Checkout {
    let base = self.here(&self.base);

    base.ensure_support_checkout(&self.support().to_string_lossy(), "true").unwrap();

    let support = self.here(&self.support());

    support.prepare_branch(branch, main, "park", "true").unwrap();
    support
  }

  async fn land(&self, at: &Checkout, main: &str, options: LandOptions<'_>) -> Result<Landed, LandRefusal> {
    at.land_rounds(&self.policy, &self.data, main, options).await
  }
}

fn repository() -> Repo {
  let dir = tempfile::tempdir().unwrap();
  let base = dir.path().join("bench");
  let remote = dir.path().join("remote.git");
  let data = dir.path().join("data");

  std::fs::create_dir_all(&base).unwrap();
  git(dir.path(), &["init", "-q", "--bare", "-b", "main", &remote.to_string_lossy()]);
  git(&base, &["init", "-q", "-b", "main"]);
  // The landing's own git rebases and commits in these repositories, so the identity it commits as is the
  // repository's.
  git(&base, &["config", "user.name", "tester"]);
  git(&base, &["config", "user.email", "tester@example.invalid"]);
  std::fs::write(base.join("a.ts"), "one\ntwo\nthree\n").unwrap();
  std::fs::write(base.join("b.ts"), "b\n").unwrap();
  git(&base, &["add", "."]);
  git(&base, &["commit", "-qm", "initial"]);
  git(&base, &["remote", "add", "origin", &remote.to_string_lossy()]);
  git(&base, &["push", "-q", "origin", "main"]);

  Repo {
    _dir: dir,
    base,
    remote,
    data,
    policy: policy(),
  }
}

fn quiet() -> LandOptions<'static> {
  LandOptions {
    push: false,
    ..LandOptions::default()
  }
}

#[tokio::test]
async fn fast_forwards_main_in_the_base_checkout_and_writes_down_what_it_landed() {
  let repo = repository();
  let support = repo.support_on("work/744", "main");

  assert_eq!(landing_branch_of(&support, &repo.rows()), "main");

  // Somebody commits on main in the base meanwhile, and leaves another file open there.
  let other = commit(&repo.base, "b.ts", "b another job\n", "other job");

  std::fs::write(repo.base.join("open.ts"), "base job at work\n").unwrap();

  let own = commit(&repo.support(), "a.ts", "one\ntwo 744\nthree\n", "744");
  let landed = repo.land(&support, "main", LandOptions::default()).await.unwrap();
  let head = git(&repo.support(), &["rev-parse", "HEAD"]);

  assert_eq!(git(&repo.base, &["rev-parse", "HEAD"]), head);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), head);
  assert_eq!(read(repo.base.join("a.ts")), "one\ntwo 744\nthree\n");
  assert_eq!(read(repo.base.join("open.ts")), "base job at work\n");
  git(&repo.base, &["merge-base", "--is-ancestor", &other, "HEAD"]);
  // The rebased commit is the job's own; the other job's commit between is not.
  assert_eq!(support.own_branch_commits(&repo.data, "main"), std::slice::from_ref(&head));
  assert_eq!(landed.landed, std::slice::from_ref(&head));
  assert_ne!(head, own);
}

#[tokio::test]
async fn a_conflict_with_main_aborts_the_rebase_and_names_the_files() {
  let repo = repository();
  let support = repo.support_on("work/744", "main");

  commit(&repo.base, "a.ts", "main's\n", "main");
  commit(&repo.support(), "a.ts", "branch's\n", "744");

  match repo.land(&support, "main", LandOptions::default()).await {
    Err(LandRefusal::Conflict { files, .. }) => assert_eq!(files, ["a.ts"]),
    other => panic!("not refused on the conflict: {other:?}"),
  }
  assert_eq!(git(&repo.support(), &["status", "--porcelain"]), "");
  assert_eq!(read(repo.support().join("a.ts")), "branch's\n");
}

#[tokio::test]
async fn an_open_change_in_the_base_the_landing_would_overwrite_refuses_it() {
  let repo = repository();
  let support = repo.support_on("work/744", "main");

  commit(&repo.support(), "a.ts", "744\n", "744");
  commit(&repo.support(), "new/c.ts", "c\n", "744 more");
  std::fs::write(repo.base.join("a.ts"), "base job's own, uncommitted\n").unwrap();
  // A file git has not seen, standing where the landing puts a folder.
  std::fs::write(repo.base.join("new"), "a scratch file\n").unwrap();
  std::fs::write(repo.base.join("untouched.ts"), "beside it\n").unwrap();

  let before = git(&repo.base, &["rev-parse", "main"]);

  assert_eq!(
    repo.land(&support, "main", LandOptions::default()).await,
    Err(LandRefusal::Overwrite {
      at: repo.base.to_string_lossy().into_owned(),
      files: vec!["a.ts".to_owned(), "new".to_owned()],
    })
  );
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), before);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), before);
  assert_eq!(read(repo.base.join("a.ts")), "base job's own, uncommitted\n");
  assert!(landed_commits(&repo.data, &support.cwd).is_empty());
}

#[tokio::test]
async fn a_push_the_remote_refuses_leaves_main_and_the_record_where_they_were() {
  let repo = repository();
  let support = repo.support_on("work/744", "main");
  let hook = repo.remote.join("hooks").join("pre-receive");

  std::fs::write(&hook, "#!/bin/sh\necho 'main is frozen here' >&2\nexit 1\n").unwrap();
  std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
  commit(&repo.support(), "a.ts", "one\ntwo 744\nthree\n", "744");

  let before = git(&repo.base, &["rev-parse", "main"]);

  match repo.land(&support, "main", LandOptions::default()).await {
    Err(LandRefusal::Push(said)) => assert!(said.contains("main is frozen here"), "{said}"),
    other => panic!("not refused on the push: {other:?}"),
  }
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), before);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), before);
  assert_eq!(read(repo.base.join("a.ts")), "one\ntwo\nthree\n");
  assert!(landed_commits(&repo.data, &support.cwd).is_empty());
  // The job's commit is still the branch's own, past main, for the next close to land.
  assert_eq!(support.own_branch_commits(&repo.data, "main").len(), 1);
}

// The first syllable of a script a commit must not hold, written by its code point so the test file stays
// English.
const SYLLABLE: char = '\u{AC00}';

#[tokio::test]
async fn a_commit_holding_hangul_is_refused_before_the_push_and_main_and_the_remote_stay() {
  let repo = repository();
  let support = repo.support_on("work/744", "main");
  let subject = format!("fix the {SYLLABLE} parser");

  commit(&repo.support(), "a.ts", "one\ntwo 744\nthree\n", "744 in English");
  commit(&repo.support(), "b.ts", "b 744\n", &subject);

  let before = git(&repo.base, &["rev-parse", "main"]);
  let refusal = repo.land(&support, "main", LandOptions::default()).await.unwrap_err();
  let short = git(&repo.support(), &["rev-parse", "--short", "HEAD"]);

  match &refusal {
    LandRefusal::Language(named) => {
      assert_eq!(named.len(), 1, "{named:?}");
      assert_eq!((named[0].short.as_str(), named[0].subject.as_str()), (short.as_str(), subject.as_str()));
    }
    other => panic!("not refused on its language: {other:?}"),
  }

  let said = refusal.text(&repo.policy, "main");

  assert!(said.contains(&format!("{short} {subject}")), "{said}");
  assert!(!said.contains("744 in English"), "{said}");
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), before);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), before);
  assert_eq!(read(repo.base.join("b.ts")), "b\n");
  assert!(landed_commits(&repo.data, &support.cwd).is_empty());
}

#[tokio::test]
async fn an_english_commit_lands_with_a_set_aside_name_in_it() {
  let repo = repository();
  let support = repo.support_on("work/744", "main");
  let name = repo.policy.close.english.set_aside.split('|').next().unwrap().to_owned();

  commit(
    &repo.support(),
    "a.ts",
    "one\ntwo 744\nthree\n",
    &format!("Answer {name} in English\n\nThe body is English too."),
  );

  let landed = repo.land(&support, "main", LandOptions::default()).await.unwrap();
  let head = git(&repo.support(), &["rev-parse", "HEAD"]);

  assert_eq!(landed.landed, std::slice::from_ref(&head));
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), head);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), head);
}

#[test]
fn two_checkouts_closing_at_once_on_the_same_file_land_the_first_and_refuse_the_second_on_its_file() {
  let repo = repository();
  let top = repo.base.parent().unwrap().to_owned();
  let clones: Vec<PathBuf> = ["w7", "w8"]
    .iter()
    .map(|name| {
      let at = top.join(name);

      git(&top, &["clone", "-q", &repo.remote.to_string_lossy(), &at.to_string_lossy()]);
      git(&at, &["config", "user.name", "tester"]);
      git(&at, &["config", "user.email", "tester@example.invalid"]);
      repo.here(&at).prepare_branch(&format!("work/{name}"), "main", "park", "true").unwrap();
      commit(&at, "a.ts", &format!("{name}'s\n"), name);
      at
    })
    .collect();
  // Each its own repository, so no landing lock stands between them: only the remote's main does.
  let start = std::sync::Barrier::new(clones.len());
  let outcomes: Vec<Result<Landed, LandRefusal>> = std::thread::scope(|scope| {
    let landings: Vec<_> = clones
      .iter()
      .map(|at| {
        let (start, repo) = (&start, &repo);

        scope.spawn(move || {
          let here = Checkout::local(at.to_string_lossy());
          let refresh = || here.follow_remote_main("main");
          let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();

          start.wait();
          runtime.block_on(repo.land(
            &here,
            "main",
            LandOptions {
              refresh: Some(&refresh),
              ..LandOptions::default()
            },
          ))
        })
      })
      .collect();

    landings.into_iter().map(|landing| landing.join().unwrap()).collect()
  });

  let landed: Vec<&Landed> = outcomes.iter().filter_map(|outcome| outcome.as_ref().ok()).collect();
  let refused: Vec<&LandRefusal> = outcomes.iter().filter_map(|outcome| outcome.as_ref().err()).collect();

  assert_eq!(landed.len(), 1, "{outcomes:?}");
  assert!(
    matches!(refused.as_slice(), [LandRefusal::Conflict { files, .. }] if files == &["a.ts"]),
    "{outcomes:?}"
  );
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), landed[0].commit);

  let (first, second) =
    if outcomes[0].is_ok() { (&clones[0], &clones[1]) } else { (&clones[1], &clones[0]) };

  assert_eq!(git(first, &["rev-parse", "main"]), landed[0].commit);
  assert_eq!(landed_commits(&repo.data, &first.to_string_lossy()), landed[0].landed);
  assert!(landed_commits(&repo.data, &second.to_string_lossy()).is_empty());
}

#[test]
fn an_uncommitted_change_to_a_tracked_file_is_refused_before_anything_moves() {
  let repo = repository();
  let support = repo.support_on("work/744", "main");

  std::fs::write(repo.support().join("a.ts"), "not committed\n").unwrap();
  std::fs::write(repo.support().join("scratch.txt"), "a scratch file is no change\n").unwrap();

  assert_eq!(support.rebase_on_main("main", &[]), Err(LandRefusal::Open(vec!["a.ts".to_owned()])));
}

#[tokio::test]
async fn lands_on_the_branch_the_base_checkout_stands_on() {
  let repo = repository();
  let main = git(&repo.base, &["rev-parse", "main"]);

  git(&repo.base, &["switch", "-qc", "field-map"]);
  commit(&repo.base, "b.ts", "b field-map\n", "field-map");

  let support = repo.support_on("work/937", "field-map");

  // A branch put on before anything was written beside it reads the remote's main in the worktree; the base
  // checkout still reads its own branch.
  git(&repo.support(), &["config", "--unset", "branch.work/937.bmo-main"]);

  assert_eq!(support.main_branch(), "main");
  assert_eq!(landing_branch_of(&support, &repo.rows()), "field-map");
  assert_eq!(landing_branch_of(&repo.here(&repo.base), &repo.rows()), "field-map");

  commit(&repo.support(), "a.ts", "one\ntwo 937\nthree\n", "937");

  let landed = repo
    .land(&support, &landing_branch_of(&support, &repo.rows()), LandOptions::default())
    .await
    .unwrap();
  let head = git(&repo.support(), &["rev-parse", "HEAD"]);

  assert_eq!(landed.landed, std::slice::from_ref(&head));
  assert_eq!(git(&repo.base, &["rev-parse", "HEAD"]), head);
  assert_eq!(git(&repo.remote, &["rev-parse", "field-map"]), head);
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), main);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), main);
}

#[test]
fn a_worker_of_its_own_in_a_linked_worktree_lands_on_the_main_worktree_s_branch() {
  let repo = repository();
  let cwd = PathBuf::from(format!("{}-solo", repo.base.display()));
  let solo = Worker {
    name: "solo".to_owned(),
    label: "solo".to_owned(),
    cwd: cwd.to_string_lossy().into_owned(),
    builtin: false,
    preamble: None,
    dev_command: None,
    support_count: 0,
    support_keepalive: 0,
    support_of: None,
    support_slot: None,
    node: None,
  };

  git(&repo.base, &["switch", "-qc", "field-map"]);
  git(&repo.base, &["worktree", "add", "-q", "--detach", &cwd.to_string_lossy(), "field-map"]);

  let here = repo.here(&cwd);

  here.prepare_branch("work/1070", "field-map", "park", "true").unwrap();
  git(&cwd, &["config", "--unset", "branch.work/1070.bmo-main"]);

  assert_eq!(landing_branch_of(&here, &[repo.rows(), vec![solo]].concat()), "field-map");
}

#[test]
fn the_landing_reads_the_base_checkout_s_branch_while_its_own_job_has_it_on_a_work_branch() {
  let repo = repository();
  let base = repo.here(&repo.base);

  git(&repo.base, &["switch", "-qc", "field-map"]);
  base.prepare_branch("work/945", "field-map", "park", "true").unwrap();
  base.ensure_support_checkout(&repo.support().to_string_lossy(), "true").unwrap();

  assert_eq!(landing_branch_of(&repo.here(&repo.support()), &repo.rows()), "field-map");
}

// A support branch made from main while the base checkout stood there, and a base checkout since put on a
// branch of its own that parted from main before main's latest commit.
fn made_from_main_base_elsewhere() -> (Repo, Checkout, String, String) {
  let repo = repository();

  git(&repo.base, &["branch", "field-map"]);
  commit(&repo.base, "b.ts", "b main only\n", "main only");

  let support = repo.support_on("work/951", "main");
  let branch_head = commit(&repo.support(), "a.ts", "one\ntwo 951\nthree\n", "951");

  git(&repo.base, &["switch", "-q", "field-map"]);

  let field_map = commit(&repo.base, "c.ts", "field-map\n", "field-map");

  (repo, support, field_map, branch_head)
}

#[tokio::test]
async fn refuses_to_land_a_branch_on_another_branch_than_the_one_it_was_made_from() {
  let (repo, support, field_map, branch_head) = made_from_main_base_elsewhere();
  let main = landing_branch_of(&support, &repo.rows());

  assert_eq!(main, "field-map");
  assert_eq!(
    repo.land(&support, &main, LandOptions::default()).await,
    Err(LandRefusal::Origin {
      branch: "work/951".to_owned(),
      from: "main".to_owned(),
    })
  );
  assert_eq!(git(&repo.base, &["rev-parse", "field-map"]), field_map);
  assert_eq!(git(&repo.support(), &["rev-parse", "HEAD"]), branch_head);
  assert!(git_fails(&repo.remote, &["rev-parse", "--verify", "field-map"]));
}

#[test]
fn reads_a_branch_made_before_its_origin_was_written_down_against_its_candidates() {
  let (repo, support, _, _) = made_from_main_base_elsewhere();

  git(&repo.support(), &["config", "--remove-section", "branch.work/951"]);

  assert!(
    matches!(support.rebase_on_main("field-map", &[]), Err(LandRefusal::Origin { from, .. }) if from == "main")
  );
}

#[tokio::test]
async fn lands_a_branch_whose_own_commits_were_moved_across() {
  let (repo, support, field_map, _) = made_from_main_base_elsewhere();

  git(&repo.support(), &["rebase", "-q", "--onto", "field-map", "main"]);

  let landed = repo.land(&support, "field-map", quiet()).await.unwrap();

  assert_eq!(landed.landed, [git(&repo.support(), &["rev-parse", "HEAD"])]);
  assert_eq!(git(&repo.base, &["rev-parse", "HEAD~1"]), field_map);
}

// A support branch whose job adds a module importing one main already has, closed against main as it then
// stood; and the check again that reads the rebased tree, the imported module there or not.
fn closed_branch() -> (Repo, Checkout, String) {
  let repo = repository();

  commit(&repo.base, "lib.ts", "export const lib = 1;\n", "lib");
  git(&repo.base, &["push", "-q", "origin", "main"]);

  let support = repo.support_on("work/777", "main");

  commit(&repo.support(), "use.ts", "import { lib } from \"./lib\";\n", "777");

  let checked = git(&repo.base, &["rev-parse", "main"]);

  (repo, support, checked)
}

fn loads(cwd: &Path) -> Option<String> {
  let imported = read(cwd.join("use.ts"));
  let module = imported.split('"').nth(1).unwrap_or_default();

  (!cwd.join(format!("{module}.ts")).exists()).then(|| format!("use.ts did not load: {module}"))
}

#[tokio::test]
async fn a_module_main_renamed_meanwhile_is_checked_again_and_refused_and_the_mended_commit_lands() {
  let (repo, support, checked) = closed_branch();

  // Between the close's check and its landing another landing renames the module the job imports.
  git(&repo.base, &["mv", "lib.ts", "moved.ts"]);
  git(&repo.base, &["commit", "-qm", "rename"]);

  let renamed = git(&repo.base, &["rev-parse", "HEAD"]);

  git(&repo.base, &["push", "-q", "origin", "main"]);

  let seen = RefCell::new(Vec::<String>::new());
  let mut recheck = |since: &str| {
    seen.borrow_mut().push(since.to_owned());
    loads(&repo.support())
  };
  let refused = repo
    .land(
      &support,
      "main",
      LandOptions {
        checked: Some(checked.clone()),
        recheck: Some(&mut recheck),
        ..LandOptions::default()
      },
    )
    .await;

  assert!(matches!(&refused, Err(LandRefusal::Check(said)) if said.contains("./lib")));
  assert_eq!(*seen.borrow(), std::slice::from_ref(&checked));
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), renamed);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), renamed);
  // The branch stands rebased onto the main it was refused on, for the job to mend.
  git(&repo.support(), &["merge-base", "--is-ancestor", &renamed, "HEAD"]);

  commit(&repo.support(), "use.ts", "import { lib } from \"./moved\";\n", "777 mended");

  let mut recheck = |since: &str| {
    seen.borrow_mut().push(since.to_owned());
    loads(&repo.support())
  };

  repo
    .land(
      &support,
      "main",
      LandOptions {
        checked: Some(checked.clone()),
        recheck: Some(&mut recheck),
        ..LandOptions::default()
      },
    )
    .await
    .unwrap();

  assert_eq!(*seen.borrow(), [checked.clone(), checked]);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), git(&repo.support(), &["rev-parse", "HEAD"]));
}

#[tokio::test]
async fn a_main_that_has_not_moved_since_the_close_checked_lands_without_checking_again() {
  let (repo, support, checked) = closed_branch();
  let mut seen = 0;
  let mut recheck = |_: &str| {
    seen += 1;
    None
  };

  repo
    .land(
      &support,
      "main",
      LandOptions {
        checked: Some(checked),
        recheck: Some(&mut recheck),
        ..LandOptions::default()
      },
    )
    .await
    .unwrap();

  assert_eq!(seen, 0);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), git(&repo.support(), &["rev-parse", "HEAD"]));
}

#[tokio::test]
async fn the_checkout_main_stands_in_is_set_up_again_where_a_lockfile_moved_and_not_otherwise() {
  let repo = repository();
  let support = repo.support_on("work/777", "main");
  let set_up = RefCell::new(Vec::<String>::new());
  let setup = |at: &Checkout| {
    set_up.borrow_mut().push(at.cwd.clone());
    Ok(())
  };
  let options = || LandOptions {
    push: false,
    setup_after: Some(&|files: &[String]| setup_inputs_moved(files)),
    setup: Some(&setup),
    ..LandOptions::default()
  };

  commit(&repo.support(), "a.ts", "one 777\ntwo\nthree\n", "code only");

  assert_eq!(repo.land(&support, "main", options()).await.unwrap().set_up, None);
  assert!(set_up.borrow().is_empty());

  commit(&repo.support(), "Cargo.lock", "serde 1\n", "lock");

  let locked = repo.land(&support, "main", options()).await.unwrap();

  assert_eq!(
    locked.set_up,
    Some(LandSetUp {
      at: repo.base.to_string_lossy().into_owned(),
      fault: None,
    })
  );
  assert_eq!(*set_up.borrow(), [repo.base.to_string_lossy().into_owned()]);
  assert_eq!(read(repo.base.join("Cargo.lock")), "serde 1\n");
}

#[tokio::test]
async fn a_setup_that_fails_there_leaves_the_landing_standing_and_says_why() {
  let repo = repository();
  let support = repo.support_on("work/777", "main");
  let own = commit(&repo.support(), "crates/a/Cargo.toml", "[package]\n", "a crate");
  let failing = |_: &Checkout| Err("the setup failed".to_owned());
  let landed = repo
    .land(
      &support,
      "main",
      LandOptions {
        setup_after: Some(&|files: &[String]| setup_inputs_moved(files)),
        setup: Some(&failing),
        ..LandOptions::default()
      },
    )
    .await
    .unwrap();

  assert_eq!(landed.landed, std::slice::from_ref(&own));
  assert_eq!(landed.set_up.and_then(|set_up| set_up.fault).as_deref(), Some("the setup failed"));
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), own);
}

#[tokio::test]
async fn its_open_files_never_hold_up_a_support_worker_s_landing_and_whoever_lands_second_rebases() {
  let repo = repository();
  let support = repo.support_on("work/753", "main");
  let base = repo.here(&repo.base);

  base.prepare_branch("work/758", "main", "park", "true").unwrap();
  // The base job holds the very file the support job changed, uncommitted.
  std::fs::write(repo.base.join("a.ts"), "one\ntwo\nthree 758\n").unwrap();

  let theirs = commit(&repo.support(), "a.ts", "one 753\ntwo\nthree\n", "753");

  assert_eq!(
    repo.land(&support, "main", LandOptions::default()).await.unwrap().landed,
    std::slice::from_ref(&theirs)
  );
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), theirs);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), theirs);
  assert_eq!(git(&repo.base, &["symbolic-ref", "--short", "HEAD"]), "work/758");
  assert_eq!(read(repo.base.join("a.ts")), "one\ntwo\nthree 758\n");

  // The base job closes after it: its rebase takes the landed change in, and it lands on top.
  git(&repo.base, &["commit", "-qam", "758"]);
  repo.land(&base, "main", LandOptions::default()).await.unwrap();

  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), git(&repo.base, &["rev-parse", "HEAD"]));
  assert_eq!(read(repo.base.join("a.ts")), "one 753\ntwo\nthree 758\n");

  // As the job ends the base goes back on main, and the branch main now holds is deleted.
  assert_eq!(
    base.restore_main("main", &[], "park"),
    MainRestored {
      parked: None,
      on_main: true,
      said: None,
      deleted: Some("work/758".to_owned()),
    }
  );
  assert_eq!(git(&repo.base, &["symbolic-ref", "--short", "HEAD"]), "main");
}

#[tokio::test]
async fn a_conflict_is_the_later_job_s_to_resolve_and_main_keeps_the_first() {
  let repo = repository();
  let support = repo.support_on("work/753", "main");
  let base = repo.here(&repo.base);

  base.prepare_branch("work/758", "main", "park", "true").unwrap();
  std::fs::write(repo.base.join("a.ts"), "758's\n").unwrap();

  let theirs = commit(&repo.support(), "a.ts", "753's\n", "753");

  repo.land(&support, "main", quiet()).await.unwrap();
  git(&repo.base, &["commit", "-qam", "758"]);

  assert!(
    matches!(repo.land(&base, "main", quiet()).await, Err(LandRefusal::Conflict { files, .. }) if files == ["a.ts"])
  );
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), theirs);
  assert_eq!(read(repo.base.join("a.ts")), "758's\n");
}

#[tokio::test]
async fn lands_with_somebody_else_s_edit_open_beside_it_and_still_refuses_its_own() {
  let repo = repository();
  let support = repo.support_on("work/753", "main");
  let base = repo.here(&repo.base);

  std::fs::write(repo.base.join("b.ts"), "a person's own edit\n").unwrap();
  base.prepare_branch("work/758", "main", "park", "true").unwrap();
  commit(&repo.support(), "a.ts", "one 753\ntwo\nthree\n", "753");
  repo.land(&support, "main", quiet()).await.unwrap();
  commit(&repo.base, "a.ts", "one\ntwo\nthree 758\n", "758");

  assert_eq!(repo.land(&base, "main", quiet()).await, Err(LandRefusal::Open(vec!["b.ts".to_owned()])));

  let others = ["b.ts".to_owned()];

  repo
    .land(
      &base,
      "main",
      LandOptions {
        others_open: &others,
        push: false,
        ..LandOptions::default()
      },
    )
    .await
    .unwrap();

  assert_eq!(read(repo.base.join("a.ts")), "one 753\ntwo\nthree 758\n");
  assert_eq!(read(repo.base.join("b.ts")), "a person's own edit\n");
  assert_eq!(git(&repo.base, &["show", "main:b.ts"]), "b");
}

// A base checkout with a remote, and a checkout of the same repository on a node.
fn with_a_node() -> (Repo, PathBuf) {
  let repo = repository();
  let node = repo.base.parent().unwrap().join("node").join("bench-n1");

  std::fs::create_dir_all(node.parent().unwrap()).unwrap();
  git(
    node.parent().unwrap(),
    &["clone", "-q", &repo.remote.to_string_lossy(), &node.to_string_lossy()],
  );
  git(&node, &["config", "user.name", "tester"]);
  git(&node, &["config", "user.email", "tester@example.invalid"]);

  (repo, node)
}

#[tokio::test]
async fn a_node_s_landing_takes_the_remote_s_main_in_pushes_and_the_base_follows() {
  let (repo, node) = with_a_node();
  let at = repo.here(&node);

  at.prepare_branch("work/77", "main", "park", "true").unwrap();
  // Another job landed here meanwhile and pushed.
  commit(&repo.base, "b.ts", "here\n", "landed here");
  git(&repo.base, &["push", "-q", "origin", "main"]);
  std::fs::write(node.join("a.ts"), "one\nnode 77\n").unwrap();
  git(&node, &["commit", "-qam", "77"]);

  let refresh = || at.follow_remote_main("main");

  repo
    .land(
      &at,
      "main",
      LandOptions {
        refresh: Some(&refresh),
        ..LandOptions::default()
      },
    )
    .await
    .unwrap();

  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), git(&node, &["rev-parse", "HEAD"]));

  repo.here(&repo.base).follow_remote_main("main").unwrap();
  assert_eq!(read(repo.base.join("a.ts")), "one\nnode 77\n");
  assert_eq!(read(repo.base.join("b.ts")), "here\n");
}

#[test]
fn a_landing_here_takes_in_what_a_node_pushed() {
  let (repo, node) = with_a_node();

  std::fs::write(node.join("a.ts"), "from the node\n").unwrap();
  git(&node, &["commit", "-qam", "node"]);
  git(&node, &["push", "-q", "origin", "HEAD:main"]);

  let mut rows = repo.rows();

  rows[1].cwd = node.to_string_lossy().into_owned();
  rows[1].node = Some("n1".to_owned());
  rows[1].support_slot = None;

  let base = repo.here(&repo.base);

  assert!(pushed_from_nodes(&base, &rows));
  assert!(!pushed_from_nodes(&base, &rows[..1]));

  base.follow_remote_main("main").unwrap();
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), git(&repo.remote, &["rev-parse", "main"]));
}

// A submodule `sub` the base checkout pins, its own repository bare beside it and seeded from a clone of the
// test's: that repository, and the seed.
fn with_a_submodule(repo: &Repo) -> (PathBuf, PathBuf) {
  let top = repo.base.parent().unwrap();
  let sub_remote = top.join("sub.git");
  let seed = top.join("sub-seed");

  git(top, &["init", "-q", "--bare", "-b", "main", &sub_remote.to_string_lossy()]);
  std::fs::create_dir_all(&seed).unwrap();
  git(&seed, &["init", "-q", "-b", "main"]);
  commit(&seed, "s.ts", "s\n", "s");
  git(&seed, &["push", "-q", &sub_remote.to_string_lossy(), "main"]);
  git(
    &repo.base,
    &[
      "-c",
      "protocol.file.allow=always",
      "submodule",
      "add",
      "-q",
      &sub_remote.to_string_lossy(),
      "sub",
    ],
  );
  git(&repo.base, &["commit", "-qm", "pin"]);
  git(&repo.base, &["push", "-q", "origin", "main"]);

  (sub_remote, seed)
}

// A job in the base checkout on `branch` that commits inside the submodule and moves the pin to it: the commit
// pinned.
fn move_the_pin(repo: &Repo, branch: &str, text: &str) -> String {
  let sub = repo.base.join("sub");

  git(&repo.base, &["switch", "-q", "-c", branch]);

  let moved = commit(&sub, "s.ts", text, branch);

  git(&repo.base, &["add", "sub"]);
  git(&repo.base, &["commit", "-qm", &format!("move the pin, {branch}")]);
  moved
}

#[tokio::test]
async fn a_moved_pin_is_pushed_to_the_submodule_s_repository_before_main_moves() {
  let repo = repository();
  let (sub_remote, seed) = with_a_submodule(&repo);
  let sub = repo.base.join("sub");
  let base = repo.here(&repo.base);
  let moved = move_the_pin(&repo, "work/9", "s 9\n");

  repo.land(&base, "main", LandOptions::default()).await.unwrap();

  assert_eq!(git(&sub_remote, &["rev-parse", "main"]), moved);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), git(&repo.base, &["rev-parse", "HEAD"]));

  // The submodule's repository moves on elsewhere, and a pin beside it would not fast-forward it.
  git(&seed, &["pull", "-q", &sub_remote.to_string_lossy(), "main"]);

  let elsewhere = commit(&seed, "t.ts", "t\n", "elsewhere");

  git(&seed, &["push", "-q", &sub_remote.to_string_lossy(), "main"]);
  git(&repo.base, &["switch", "-q", "-c", "work/10"]);
  commit(&sub, "s.ts", "s 10\n", "10");
  git(&repo.base, &["add", "sub"]);
  git(&repo.base, &["commit", "-qm", "move the pin again"]);

  let main_before = git(&repo.remote, &["rev-parse", "main"]);

  match repo.land(&base, "main", LandOptions::default()).await {
    Err(LandRefusal::Submodule { path, branch, .. }) => {
      assert_eq!((path.as_str(), branch.as_str()), ("sub", "main"))
    }
    other => panic!("not refused on the submodule: {other:?}"),
  }
  assert_eq!(git(&sub_remote, &["rev-parse", "main"]), elsewhere);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), main_before);
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), main_before);
}

#[tokio::test]
async fn two_closes_moving_the_same_pin_in_turn_push_both_and_the_second_is_live() {
  let repo = repository();
  let (sub_remote, _) = with_a_submodule(&repo);
  let base = repo.here(&repo.base);
  let first = move_the_pin(&repo, "work/11", "s 11\n");
  let landed_first = repo.land(&base, "main", LandOptions::default()).await.unwrap();
  let second = move_the_pin(&repo, "work/12", "s 12\n");
  let landed_second = repo.land(&base, "main", LandOptions::default()).await.unwrap();

  // Both pins stand on the submodule's own main, the second over the first.
  git(&sub_remote, &["merge-base", "--is-ancestor", &first, "main"]);
  assert_eq!(git(&sub_remote, &["rev-parse", "main"]), second);
  // Each landing pushed main pinning its own commit, and main now pins the second.
  assert_eq!(git(&repo.remote, &["rev-parse", &format!("{}:sub", landed_first.commit)]), first);
  assert_eq!(git(&repo.remote, &["rev-parse", "main"]), landed_second.commit);
  assert_eq!(git(&repo.remote, &["rev-parse", "main:sub"]), second);
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), landed_second.commit);
}

// A support worker's branch moving the folder `old/` a unit file stands in to `new/`.
fn unit_moved(repo: &Repo) -> Checkout {
  commit(&repo.base, "old/runner.service", "[Service]\n", "a unit");

  let support = repo.support_on("work/30", "main");

  git(&repo.support(), &["mv", "old", "new"]);
  git(&repo.support(), &["commit", "-qm", "move the folder"]);
  support
}

#[tokio::test]
async fn a_unit_linked_into_the_checkout_main_stands_in_follows_its_file_as_main_moves_there() {
  let repo = repository();
  let support = unit_moved(&repo);
  let testbed = testbed::Stand::new();
  let client = testbed.client();
  let links = TestbedLinks(&client);
  let units = CheckoutUnits {
    links: &links,
    policy: &repo.policy,
  };

  testbed.answer(
    "units-plan",
    &json!({ "relinks": [{ "name": "runner.service", "file": "new/runner.service" }], "running": [], "unmatched": [] })
      .to_string(),
    "",
    0,
  );
  testbed.answer(
    "units-relink",
    r#"{"relinked": ["runner.service"], "dangling": [], "unmatched": []}"#,
    "",
    0,
  );
  repo
    .land(
      &support,
      "main",
      LandOptions {
        push: false,
        units: Some(&units),
        ..LandOptions::default()
      },
    )
    .await
    .unwrap();

  let runs = testbed.runs();

  assert_eq!(runs.len(), 2, "{runs:?}");
  assert_eq!(runs[0].words[..2], ["units", "plan"]);
  assert_eq!(runs[1].words[..2], ["units", "relink"]);
  assert_eq!(runs[1].dir.canonicalize().unwrap(), repo.base.canonicalize().unwrap());
  assert!(repo.base.join("new/runner.service").exists());
}

#[tokio::test]
async fn a_unit_running_in_a_folder_the_landing_takes_away_holds_it_with_nothing_moved() {
  let repo = repository();
  let support = unit_moved(&repo);
  let before = git(&repo.base, &["rev-parse", "main"]);
  let testbed = testbed::Stand::new();
  let client = testbed.client();
  let links = TestbedLinks(&client);
  let units = CheckoutUnits {
    links: &links,
    policy: &repo.policy,
  };

  testbed.answer(
    "units-plan",
    &json!({ "relinks": [], "running": ["runner.service"], "unmatched": [] }).to_string(),
    "",
    0,
  );

  assert_eq!(
    repo
      .land(
        &support,
        "main",
        LandOptions {
          push: false,
          units: Some(&units),
          ..LandOptions::default()
        },
      )
      .await,
    Err(LandRefusal::Running(vec!["runner.service".to_owned()]))
  );
  assert_eq!(git(&repo.base, &["rev-parse", "main"]), before);
  assert_eq!(testbed.runs().len(), 1, "no relink was asked");
}
