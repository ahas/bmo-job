//! A job's checkout before and after its work (docs/queue.md, "A checkout held"): the job's branch taken up
//! or made from main with what the last job left open parked on its own branch, a support worker's worktree
//! made where it is not there yet, a base checkout put back on main as the job ends, and what the job's own
//! commits are. A checkout on a work node answers on its node, every git question going there over one
//! shared login.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// What every job's own branch is named under, which is how a checkout on one is told from a checkout
/// on main.
pub const WORK_BRANCH_PREFIX: &str = "work/";

/// The command a repository sets its fresh worktrees up with: the script of this name in its own
/// `package.json`. The repository knows what it needs installed and linked; the runner only runs it.
pub const WORKTREE_SETUP_SCRIPT: &str = "worktree:setup";

// How long the repository's setup of a fresh worktree may take: installs from a warm cache take
// seconds, and a cold one a few minutes.
const SETUP_WAIT: Duration = Duration::from_secs(10 * 60);

/// A checkout, wherever it stands: here, or on a work node through the login that reaches it, the
/// OpenSSH arguments up to and including the destination.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Checkout {
  pub cwd: String,
  login: Option<Vec<String>>,
}

/// What a command run in a checkout came to: its exit code, none where it never ended in time, and
/// what it printed.
#[derive(Debug)]
pub struct Ran {
  pub code: Option<i32>,
  pub stdout: Vec<u8>,
  pub stderr: Vec<u8>,
}

// Git, as the checkouts need it: whether it worked and what it said, both trimmed.
struct Git {
  ok: bool,
  out: String,
  err: String,
}

/// What a job left open, committed onto the branch it was on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parked {
  pub branch: String,
  pub commit: String,
  pub paths: Vec<String>,
}

/// What putting a checkout on a job's branch did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchPrepared {
  pub parked: Option<Parked>,
  /// Whether the branch was made from main rather than taken up where it stood.
  pub made: bool,
  /// A fully landed branch deleted on the way off it.
  pub deleted: Option<String>,
}

/// What putting a base checkout back on main did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainRestored {
  pub parked: Option<Parked>,
  /// Whether the checkout stands on main now; false with why where the switch could not be made.
  pub on_main: bool,
  pub said: Option<String>,
  /// The job's branch deleted, where main already holds all of it.
  pub deleted: Option<String>,
}

/// One commit with the files it touched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Commit {
  pub hash: String,
  pub at: i64,
  pub files: Vec<String>,
}

/// One commit's message: its short hash, its subject, and the message whole.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
  pub short: String,
  pub subject: String,
  pub text: String,
}

/// A word as a POSIX shell reads it back as itself.
pub fn shell_word(word: &str) -> String {
  let plain =
    !word.is_empty() && word.chars().all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));

  if plain { word.to_owned() } else { format!("'{}'", word.replace('\'', r#"'"'"'"#)) }
}

/// A path as the runtime's own resolution writes it: absolute, with `.` and `..` read off and no slash
/// at its end, so every file keyed by a checkout's path agrees on it.
pub fn resolved(path: &str) -> String {
  let given = Path::new(path);
  let whole = if given.is_absolute() {
    given.to_path_buf()
  } else {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")).join(given)
  };
  let mut parts: Vec<String> = Vec::new();

  for part in whole.components() {
    match part {
      Component::Normal(name) => parts.push(name.to_string_lossy().into_owned()),
      Component::ParentDir => {
        parts.pop();
      }
      _ => {}
    }
  }

  format!("/{}", parts.join("/"))
}

/// The file under the data root's worktree directory a checkout's record of one kind is kept in: a
/// digest of its path rather than the path itself, so a name that is a path is never turned into a
/// file name that cannot be one.
pub fn keyed_path(data_root: &Path, cwd: &str, suffix: &str) -> PathBuf {
  let digest = hex(&Sha256::digest(resolved(cwd).as_bytes()));

  data_root.join("worktree").join(format!("{}{suffix}.json", &digest[..16]))
}

fn hex(bytes: &[u8]) -> String {
  bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

impl Checkout {
  /// A checkout on this machine.
  pub fn local(cwd: impl Into<String>) -> Self {
    Self {
      cwd: cwd.into(),
      login: None,
    }
  }

  /// A checkout on a work node, reached through this login.
  pub fn on_node(cwd: impl Into<String>, login: Vec<String>) -> Self {
    Self {
      cwd: cwd.into(),
      login: Some(login),
    }
  }

  /// Whether it stands on a work node.
  pub fn is_remote(&self) -> bool {
    self.login.is_some()
  }

  /// Another directory reached the way this one is.
  pub fn at(&self, cwd: &str) -> Self {
    Self {
      cwd: cwd.to_owned(),
      login: self.login.clone(),
    }
  }

  /// A command run in the checkout: here as it is, or on its node through its login, in the same
  /// directory with the variables given laid over the node's own. Nothing reads standard input.
  pub fn run(&self, argv: &[&str], env: &[(&str, &str)], wait: Option<Duration>) -> Ran {
    let mut command = match &self.login {
      None => {
        let mut command = Command::new(argv[0]);

        command.args(&argv[1..]).current_dir(&self.cwd).envs(env.iter().copied());
        command
      }
      Some(login) => {
        let variables: Vec<String> =
          env.iter().map(|(name, value)| format!("{name}={}", shell_word(value))).collect();
        let words: Vec<String> = argv.iter().map(|word| shell_word(word)).collect();
        let line = format!(
          "cd {} && {}",
          shell_word(&self.cwd),
          variables
            .into_iter()
            .chain(["exec".to_owned()])
            .chain(words)
            .collect::<Vec<_>>()
            .join(" ")
        );
        let mut command = Command::new("ssh");

        command.args(login).arg(line);
        command
      }
    };

    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());

    let child = match command.spawn() {
      Ok(child) => child,
      Err(why) => {
        return Ran {
          code: Some(127),
          stdout: Vec::new(),
          stderr: why.to_string().into_bytes(),
        };
      }
    };

    match wait {
      None => match child.wait_with_output() {
        Ok(output) => Ran {
          code: output.status.code(),
          stdout: output.stdout,
          stderr: output.stderr,
        },
        Err(why) => Ran {
          code: Some(1),
          stdout: Vec::new(),
          stderr: why.to_string().into_bytes(),
        },
      },
      Some(wait) => waited(child, wait),
    }
  }

  fn git(&self, args: &[&str]) -> Git {
    let argv: Vec<&str> = std::iter::once("git").chain(args.iter().copied()).collect();
    let ran = self.run(&argv, &[("GIT_EDITOR", "true"), ("GIT_TERMINAL_PROMPT", "0")], None);

    Git {
      ok: ran.code == Some(0),
      out: String::from_utf8_lossy(&ran.stdout).trim().to_owned(),
      err: String::from_utf8_lossy(&ran.stderr).trim().to_owned(),
    }
  }

  // Git as the graph's questions ask it: the output whole where it worked, nothing where it did not,
  // and no variables of the checkouts' own.
  fn git_raw(&self, args: &[&str]) -> Option<String> {
    let argv: Vec<&str> = std::iter::once("git").chain(args.iter().copied()).collect();
    let ran = self.run(&argv, &[], None);

    (ran.code == Some(0)).then(|| String::from_utf8_lossy(&ran.stdout).into_owned())
  }
}

// A run given a wait: read on threads of its own so a full pipe never holds it, and killed where it
// is still going when the wait is over.
fn waited(mut child: std::process::Child, wait: Duration) -> Ran {
  use std::io::Read;

  let mut out = child.stdout.take();
  let mut err = child.stderr.take();
  let reading_out = std::thread::spawn(move || {
    let mut bytes = Vec::new();

    if let Some(out) = out.as_mut() {
      let _ = out.read_to_end(&mut bytes);
    }
    bytes
  });
  let reading_err = std::thread::spawn(move || {
    let mut bytes = Vec::new();

    if let Some(err) = err.as_mut() {
      let _ = err.read_to_end(&mut bytes);
    }
    bytes
  });
  let until = std::time::Instant::now() + wait;
  let code = loop {
    match child.try_wait() {
      Ok(Some(status)) => break status.code(),
      Ok(None) if std::time::Instant::now() < until => std::thread::sleep(Duration::from_millis(100)),
      _ => {
        let _ = child.kill();
        let _ = child.wait();
        break None;
      }
    }
  };

  Ran {
    code,
    stdout: reading_out.join().unwrap_or_default(),
    stderr: reading_err.join().unwrap_or_default(),
  }
}

fn must(result: Git, what: &str) -> Result<String, String> {
  if result.ok {
    Ok(result.out)
  } else {
    Err(format!("{what}: {}", if result.err.is_empty() { result.out } else { result.err }))
  }
}

/// Whether a branch is a job's own rather than the repository's main.
pub fn is_work_branch(branch: Option<&str>) -> bool {
  branch.is_some_and(|branch| branch.starts_with(WORK_BRANCH_PREFIX))
}

/// The branch a job works on: its item's, or its own where it has none.
pub fn item_branch(todo_id: Option<i64>, job_id: i64) -> String {
  match todo_id {
    Some(todo) => format!("{WORK_BRANCH_PREFIX}{todo}"),
    None => format!("{WORK_BRANCH_PREFIX}job-{job_id}"),
  }
}

/// Whether a worker's ordinary jobs work on their item's branch rather than on main: every support
/// worker, and a base that has support workers, since a landing beside it moves main under its checkout.
pub fn jobs_work_on_branches(support_of: Option<&str>, support_count: i64) -> bool {
  support_of.is_some_and(|base| !base.is_empty()) || support_count > 0
}

// The git config key that holds the main a job's branch was put on from.
fn branch_main_key(branch: &str) -> String {
  format!("branch.{branch}.bmo-main")
}

// The git config key that holds the branch a job's branch was made from, written once as it is made.
fn branch_from_key(branch: &str) -> String {
  format!("branch.{branch}.bmo-from")
}

impl Checkout {
  /// Whether the path is a checkout: one on a node, which its setup made, or a git directory here.
  pub fn is_checkout(&self) -> bool {
    self.is_remote() || Path::new(&self.cwd).join(".git").exists()
  }

  /// The branch it stands on, or none where its HEAD is detached.
  pub fn current_branch(&self) -> Option<String> {
    let named = self.git(&["symbolic-ref", "--quiet", "--short", "HEAD"]);

    (named.ok && !named.out.is_empty()).then_some(named.out)
  }

  fn branch_exists(&self, branch: &str) -> bool {
    self.git(&["rev-parse", "--verify", "--quiet", &format!("refs/heads/{branch}")]).ok
  }

  fn is_ancestor(&self, ancestor: &str, of: &str) -> bool {
    self.git(&["merge-base", "--is-ancestor", ancestor, of]).ok
  }

  /// The repository's main branch, which every job's branch is made from and lands on: the branch it
  /// stands on where that is not a job's; on a job's branch, the one it was put on from where that was
  /// written down and still exists; and otherwise the one the remote's head names, then `main`, then
  /// `master`.
  pub fn main_branch(&self) -> String {
    let standing = self.current_branch();

    if let Some(standing) = &standing
      && !is_work_branch(Some(standing))
    {
      return standing.clone();
    }

    if let Some(standing) = &standing {
      let put_on_from = self.git(&["config", "--get", &branch_main_key(standing)]);

      if put_on_from.ok && !is_work_branch(Some(&put_on_from.out)) && self.branch_exists(&put_on_from.out) {
        return put_on_from.out;
      }
    }

    let remote_head = self.git(&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]);

    if remote_head.ok
      && let Some(at) = remote_head.out.find('/')
    {
      return remote_head.out[at + 1..].to_owned();
    }

    ["main", "master"]
      .into_iter()
      .find(|name| self.branch_exists(name))
      .unwrap_or("main")
      .to_owned()
  }

  /// The repository's main worktree, where this is a linked worktree of it, or none where it is that
  /// main worktree itself, no checkout, or one on a node.
  pub fn main_checkout(&self) -> Option<String> {
    if self.is_remote() || !Path::new(&self.cwd).exists() {
      return None;
    }

    let own = self.git(&["rev-parse", "--path-format=absolute", "--git-dir"]);
    let common = self.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]);

    if !own.ok
      || !common.ok
      || resolved(&own.out) == resolved(&common.out)
      || !common.out.ends_with("/.git")
    {
      return None;
    }

    Path::new(&common.out).parent().map(|parent| parent.to_string_lossy().into_owned())
  }

  // The paths it has open, read in the form that keeps each entry whole: a status line opens with a
  // space where a file is changed but not staged, and a rename carries its old path after it.
  fn status_paths(&self, untracked: bool) -> Vec<String> {
    let mode = format!("--untracked-files={}", if untracked { "all" } else { "no" });
    let ran = self.run(&["git", "status", "--porcelain=v1", "-z", &mode], &[], None);

    if ran.code != Some(0) {
      return Vec::new();
    }

    status_entries(&String::from_utf8_lossy(&ran.stdout))
      .into_iter()
      .map(|(_, path)| path)
      .collect()
  }

  /// What it has open: a changed tracked file, or a file git has not seen.
  pub fn open_paths(&self) -> Vec<String> {
    self.status_paths(true)
  }

  /// What a job left open, committed onto the branch it was on, which is how a support worker parks its
  /// work: nothing else writes in its worktree, so all of it is that job's. Answers the commit, or none
  /// where nothing was open.
  pub fn park_open(&self, message: &str) -> Result<Option<Parked>, String> {
    let paths = self.open_paths();

    if paths.is_empty() {
      return Ok(None);
    }

    let Some(branch) = self.current_branch() else {
      return Err(format!(
        "{} has open changes and no branch to gather them onto: {}",
        self.cwd,
        paths.iter().take(8).cloned().collect::<Vec<_>>().join(", ")
      ));
    };

    must(self.git(&["add", "-A"]), "could not stage the open changes")?;
    must(
      self.git(&["commit", "-q", "--no-verify", "-m", message]),
      "could not commit the open changes",
    )?;

    Ok(Some(Parked {
      branch,
      commit: must(self.git(&["rev-parse", "HEAD"]), "could not read the commit")?,
      paths,
    }))
  }

  // Whether the repository names a setup script for its fresh worktrees.
  fn setup_script(&self) -> bool {
    let manifest = if self.is_remote() {
      let ran = self.run(&["cat", "package.json"], &[], None);

      if ran.code != Some(0) {
        return false;
      }
      String::from_utf8_lossy(&ran.stdout).into_owned()
    } else {
      match std::fs::read_to_string(Path::new(&self.cwd).join("package.json")) {
        Ok(text) => text,
        Err(_) => return false,
      }
    };

    serde_json::from_str::<serde_json::Value>(&manifest)
      .ok()
      .is_some_and(|manifest| manifest["scripts"][WORKTREE_SETUP_SCRIPT].is_string())
  }

  /// Runs the repository's own setup in it, where the repository names one; `bun` is the runtime this
  /// machine runs the bot's scripts on, and a node's own where the checkout stands there.
  pub fn run_setup(&self, bun: &str) -> Result<(), String> {
    self.bring_submodules(false)?;

    if !self.setup_script() {
      return Ok(());
    }

    let bun = if self.is_remote() { "bun" } else { bun };
    let ran = self.run(&[bun, "run", WORKTREE_SETUP_SCRIPT], &[], Some(SETUP_WAIT));

    if ran.code == Some(0) {
      return Ok(());
    }

    let whole = format!("{}{}", String::from_utf8_lossy(&ran.stdout), String::from_utf8_lossy(&ran.stderr));
    let lines: Vec<&str> = whole.trim().split('\n').collect();
    let said = lines[lines.len().saturating_sub(8)..].join("\n");

    Err(format!(
      "{WORKTREE_SETUP_SCRIPT} failed in {} ({}):\n{said}",
      self.cwd,
      ran.code.map_or_else(|| "timed out".to_owned(), |code| code.to_string())
    ))
  }

  /// The submodules the commit it stands on pins, each cloned where it is missing and put on the commit
  /// pinned. A folder an older commit tracked where a
  /// submodule now stands keeps only what git ignored in it, installs and builds, which would refuse the
  /// clone, so it is emptied first. `discard` throws away what is open inside a submodule, for a tree no
  /// one works in; without it git refuses to move a submodule over open work, and that refusal is the
  /// answer.
  pub fn bring_submodules(&self, discard: bool) -> Result<(), String> {
    let declared = self.git(&["config", "--file", ".gitmodules", "--get-regexp", r"^submodule\..*\.path$"]);

    // No `.gitmodules`, or one declaring nothing: git answers no, and there is nothing to bring.
    if !declared.ok {
      return Ok(());
    }

    for path in declared.out.lines().filter_map(|line| line.split_once(' ').map(|(_, path)| path)) {
      let pinned = self.git(&["ls-files", "--stage", "--", path]).out.starts_with("160000 ");
      let at = format!("{}/{path}", self.cwd.trim_end_matches('/'));
      let checked_out = self.run(&["test", "-e", &format!("{at}/.git")], &[], None).code == Some(0);

      if pinned && !checked_out {
        self.run(&["find", &at, "-mindepth", "1", "-delete"], &[], None);
      }
    }

    must(
      self.git(&["submodule", "sync", "--quiet", "--recursive"]),
      "could not read the submodules' addresses",
    )?;

    let mut update = vec!["submodule", "update", "--quiet", "--init", "--recursive"];

    if discard {
      update.push("--force");
    }

    must(
      self.git(&update),
      &format!("could not bring the submodules of {} to their pins", self.cwd),
    )
    .map(drop)
  }

  /// A support worker's worktree at `cwd`, made where it is not there yet from this base checkout's main
  /// branch and set up by the repository's own script. A directory standing there that is not a checkout
  /// is somebody's, and is refused rather than taken over. Answers whether it was made.
  pub fn ensure_support_checkout(&self, cwd: &str, bun: &str) -> Result<bool, String> {
    let at = Path::new(cwd);

    if at.join(".git").exists() {
      return Ok(false);
    }

    if at.exists()
      && (!at.is_dir() || std::fs::read_dir(at).map_or(true, |mut entries| entries.next().is_some()))
    {
      return Err(format!(
        "{cwd} holds something that is not a worktree, so it cannot be a support worker's working path."
      ));
    }

    self.git(&["worktree", "prune"]);
    must(
      self.git(&["worktree", "add", "--detach", cwd, &self.main_branch()]),
      &format!("could not make a worktree at {cwd}"),
    )?;

    if let Err(why) = self.at(cwd).run_setup(bun) {
      // A worktree whose setup failed is taken away again, so the next launch makes it afresh rather
      // than working in a half-installed one.
      self.git(&["worktree", "remove", "--force", cwd]);
      return Err(why);
    }

    Ok(true)
  }

  // Where the branch named is checked out, if anywhere: a worktree's path.
  fn checked_out_at(&self, branch: &str) -> Option<String> {
    let list = self.git(&["worktree", "list", "--porcelain"]);

    if !list.ok {
      return None;
    }

    let mut at: Option<String> = None;
    let wanted = format!("branch refs/heads/{branch}");

    for line in list.out.split('\n') {
      if let Some(path) = line.strip_prefix("worktree ") {
        at = Some(path.to_owned());
      }
      if line == wanted {
        return at;
      }
    }

    None
  }

  /// A checkout on a node brought up to its remote's main before a job's branch is made from it: main
  /// is the remote's there, which every landing pushes.
  pub fn follow_remote_main(&self, main: &str) -> Result<(), String> {
    must(self.git(&["fetch", "--quiet", "origin"]), &format!("fetching origin in {}", self.cwd))?;

    // Main moves where it stands checked out, files and all, and as a ref alone where it stands nowhere.
    let at = self.checked_out_at(main);
    let moved = match &at {
      None => self.git(&["fetch", "--quiet", "origin", &format!("{main}:{main}")]),
      Some(path) => self.at(path).git(&["merge", "--ff-only", "--quiet", &format!("origin/{main}")]),
    };

    must(
      moved,
      &format!("bringing {main} in {} up to origin/{main}", at.as_deref().unwrap_or(&self.cwd)),
    )
    .map(drop)
  }

  /// Puts it on a job's branch: parks what was left open, takes the branch up where it exists or makes
  /// it from main where it does not, deletes a branch it moved off that main already has in full, and
  /// runs the repository's setup on what the branch holds. What is open on main — which only a base
  /// checkout stands on — is never committed there: the switch carries it across as it stands.
  pub fn prepare_branch(
    &self,
    branch: &str,
    main: &str,
    park_message: &str,
    bun: &str,
  ) -> Result<BranchPrepared, String> {
    let prepared = self.switch_to_branch(branch, main, park_message)?;

    // Written beside the branch, which `git branch -d` takes away with it, so a base checkout on its own
    // job's branch still reads the main it came from, and the landing reads the branch it was made from.
    if !is_work_branch(Some(main)) {
      must(
        self.git(&["config", &branch_main_key(branch), main]),
        &format!("could not record {branch}'s main"),
      )?;

      if prepared.made {
        must(
          self.git(&["config", &branch_from_key(branch), main]),
          &format!("could not record the branch {branch} came from"),
        )?;
      }
    }

    self.run_setup(bun)?;

    Ok(prepared)
  }

  fn switch_to_branch(
    &self,
    branch: &str,
    main: &str,
    park_message: &str,
  ) -> Result<BranchPrepared, String> {
    let was = self.current_branch();

    // What is open on the job's own branch is its own item's work and is left open for it.
    if was.as_deref() == Some(branch) {
      return Ok(BranchPrepared {
        parked: None,
        made: false,
        deleted: None,
      });
    }

    let parked = if was.as_deref() == Some(main) { None } else { self.park_open(park_message)? };
    let made = !self.branch_exists(branch);
    let switched = if made {
      self.git(&["switch", "-q", "-c", branch, main])
    } else {
      self.git(&["switch", "-q", branch])
    };

    must(switched, &format!("could not put {} on {branch}", self.cwd))?;

    let deleted = was.filter(|was| {
      was != main
        && was.starts_with(WORK_BRANCH_PREFIX)
        && self.is_ancestor(was, main)
        && self.git(&["branch", "-q", "-d", was]).ok
    });

    Ok(BranchPrepared {
      parked,
      made,
      deleted,
    })
  }

  // Commits the named paths — the ones a job left open — onto the branch it stands on, and nothing else
  // that is open there. Answers the commit, or none where none of them had anything to commit.
  fn park_paths(&self, paths: &[String], message: &str) -> Result<Option<String>, String> {
    if paths.is_empty() {
      return Ok(None);
    }

    // One path at a time: a path the job opened and removed again names nothing to git, and one refused
    // path must not take the others with it.
    for path in paths {
      self.git(&["add", "-A", "--", &format!(":(literal){path}")]);
    }

    if self.git(&["diff", "--cached", "--quiet"]).ok {
      return Ok(None);
    }

    must(
      self.git(&["commit", "-q", "--no-verify", "-m", message]),
      "could not commit the open changes",
    )?;

    must(self.git(&["rev-parse", "HEAD"]), "could not read the commit").map(Some)
  }

  /// Puts a base checkout back on main as its job ends: what that job left open is committed onto its
  /// branch, main is checked out as the landings left it, carrying across what was open before the job
  /// began, and the branch is deleted where main holds all of it. A checkout not on a job's branch is left
  /// as it stands. The installed units linked into it are the caller's to relink, through bmo-testbed,
  /// once it stands on main.
  pub fn restore_main(&self, main: &str, own: &[String], park_message: &str) -> MainRestored {
    let standing = self.current_branch();
    let Some(branch) = standing.clone().filter(|branch| is_work_branch(Some(branch))) else {
      return MainRestored {
        parked: None,
        on_main: standing.as_deref() == Some(main),
        said: None,
        deleted: None,
      };
    };

    let parked = match self.park_paths(own, park_message) {
      Ok(commit) => commit.map(|commit| Parked {
        branch: branch.clone(),
        commit,
        paths: own.to_vec(),
      }),
      Err(said) => {
        return MainRestored {
          parked: None,
          on_main: false,
          said: Some(said),
          deleted: None,
        };
      }
    };
    let switched = self.git(&["switch", "-q", main]);

    if !switched.ok {
      return MainRestored {
        parked,
        on_main: false,
        said: Some(if switched.err.is_empty() { switched.out } else { switched.err }),
        deleted: None,
      };
    }

    let deleted =
      (self.is_ancestor(&branch, main) && self.git(&["branch", "-q", "-d", &branch]).ok).then_some(branch);

    MainRestored {
      parked,
      on_main: true,
      said: None,
      deleted,
    }
  }

  /// The commit it stands at, or none where that cannot be read.
  pub fn head_commit(&self) -> Option<String> {
    self.git_raw(&["rev-parse", "HEAD"]).map(|out| out.trim().to_owned())
  }

  /// The commits between a point and HEAD, each with the files it touched, newest first; none where git
  /// could not say.
  pub fn commits_between(&self, from: &str) -> Option<Vec<Commit>> {
    let log = self.git_raw(&["log", "--name-only", "--format=%x00%H %ct", &format!("{from}..HEAD")])?;

    Some(
      log
        .split('\0')
        .filter(|block| !block.trim().is_empty())
        .map(|block| {
          let lines: Vec<&str> = block.split('\n').filter(|line| !line.trim().is_empty()).collect();
          let mut head = lines.first().copied().unwrap_or_default().split(' ');
          let hash = head.next().unwrap_or_default().to_owned();
          let at = head.next().and_then(|at| at.parse().ok()).unwrap_or(0);

          Commit {
            hash,
            at,
            files: lines.iter().skip(1).map(|line| (*line).to_owned()).collect(),
          }
        })
        .collect(),
    )
  }

  /// The messages of the commits on HEAD that none of the points named holds, oldest first; a point that
  /// names no commit here is passed over, and none at all reads nothing, nor does a git that could not say.
  pub fn messages_past(&self, points: &[String]) -> Vec<Message> {
    let known: Vec<&str> = points
      .iter()
      .map(String::as_str)
      .filter(|point| self.git(&["rev-parse", "--verify", "--quiet", &format!("{point}^{{commit}}")]).ok)
      .collect();

    if known.is_empty() {
      return Vec::new();
    }

    let mut args = vec!["log", "--reverse", "--format=%h%x00%s%x00%B%x1e", "HEAD", "--not"];

    args.extend(known);

    self
      .git_raw(&args)
      .unwrap_or_default()
      .split('\u{1e}')
      .filter_map(|record| {
        let mut fields = record.trim_start_matches('\n').splitn(3, '\0');

        Some(Message {
          short: fields.next().filter(|short| !short.is_empty())?.to_owned(),
          subject: fields.next()?.to_owned(),
          text: fields.next()?.trim().to_owned(),
        })
      })
      .collect()
  }

  /// A job's own commits on its branch: what it landed, and what is still on its branch past main.
  pub fn own_branch_commits(&self, data_root: &Path, main: &str) -> Vec<String> {
    let unlanded = self.git(&["rev-list", "--reverse", &format!("{main}..HEAD")]);
    let mut seen = HashSet::new();

    landed_commits(data_root, &self.cwd)
      .into_iter()
      .chain(if unlanded.ok {
        unlanded.out.split('\n').filter(|one| !one.is_empty()).map(str::to_owned).collect()
      } else {
        Vec::new()
      })
      .filter(|commit| seen.insert(commit.clone()))
      .collect()
  }

  /// Every file a set of commits touched, each path once.
  pub fn commit_files(&self, commits: &[String]) -> Vec<String> {
    if commits.is_empty() {
      return Vec::new();
    }

    let mut args = vec!["log", "--no-walk=unsorted", "--name-only", "--format="];

    args.extend(commits.iter().map(String::as_str));

    let files = self.git(&args);
    let mut seen = HashSet::new();

    if files.ok {
      files
        .out
        .split('\n')
        .filter(|one| !one.is_empty() && seen.insert(*one))
        .map(str::to_owned)
        .collect()
    } else {
      Vec::new()
    }
  }
}

/// The entries of `git status --porcelain=v1 -z`, each its two-letter code and its path; a rename or a
/// copy is followed by the path it came from, which is no entry of its own.
pub fn status_entries(output: &str) -> Vec<(String, String)> {
  let fields: Vec<&str> = output.split('\0').collect();
  let mut entries = Vec::new();
  let mut at = 0;

  while at < fields.len() {
    let entry = fields[at];

    if !entry.is_empty() {
      let code: String = entry.chars().take(2).collect();

      if code.contains(['R', 'C']) {
        at += 1;
      }
      entries.push((code, entry.get(3..).unwrap_or_default().to_owned()));
    }
    at += 1;
  }

  entries
}

// Where a checkout's landings are written down for the job at work in it: the commits the landing put
// on main, which is what that job's own commits are once other jobs' landings stand between where its
// branch began and where main is now.
fn landed_path(data_root: &Path, cwd: &str) -> PathBuf {
  keyed_path(data_root, cwd, "-landed")
}

/// The commits landed from a worktree since the record was last cleared.
pub fn landed_commits(data_root: &Path, cwd: &str) -> Vec<String> {
  std::fs::read_to_string(landed_path(data_root, cwd))
    .ok()
    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    .and_then(|held| {
      held
        .as_array()
        .map(|all| all.iter().filter_map(|one| one.as_str().map(str::to_owned)).collect())
    })
    .unwrap_or_default()
}

/// Clears the record, which a job does as it starts.
pub fn clear_landed(data_root: &Path, cwd: &str) {
  let _ = std::fs::remove_file(landed_path(data_root, cwd));
}
