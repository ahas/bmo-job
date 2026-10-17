//! A job's branch landed on main (docs/runs.md, "Landing on main"): rebased onto main where main has moved,
//! checked again by the caller where the rebase took in commits its own check did not cover, pushed to the
//! remote's main as a fast-forward, and only then main fast-forwarded to it here — in the checkout main stands
//! in, which moves that checkout's files with it — under the repository's landing lock: held here, or on the
//! operations machine through `node-land` for a checkout on a work node.

use std::io::{BufRead as _, BufReader, Lines, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::time::Duration;

use job_policy::Policy;
use job_testbed::Relink;
use queue_rows::closing::reads_as_english;
use regex::Regex;

use super::units::{CheckoutUnits, UnitsHeld};
use super::{
  Checkout, Message, branch_from_key, branch_main_key, is_work_branch, landed_commits, landed_path, must,
  resolved,
};
use crate::order_run::hold::SETUP_RUNTIME;
use crate::queue::job_start::{hold_within, lock_file_name};

/// How long a landing waits for another to finish: long enough for that landing to check its commit again,
/// setup, static check and selected tests, when main moved under it.
pub const LAND_LOCK_WAIT: Duration = Duration::from_secs(600);

// How many rebases a landing makes for a main that keeps moving in between.
const ROUNDS: usize = 3;

/// Why a landing did not go through, in the reader's terms.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LandRefusal {
  /// Changed tracked files that are the job's own, which the branch would land without.
  Open(Vec<String>),
  /// The branch was made from another branch whose commits main lacks.
  Origin { branch: String, from: String },
  /// The rebase stopped on conflicts, and was aborted.
  Conflict { files: Vec<String>, said: String },
  /// Main could not be moved where it stands checked out, or brought up to the remote's.
  Base(String),
  /// Moving main where it stands checked out would overwrite files open there.
  Overwrite { at: String, files: Vec<String> },
  /// The rebased commit did not pass the caller's check again.
  Check(String),
  /// Main kept moving under every round.
  Moving,
  /// Another landing held the lock past the wait.
  Locked,
  /// The remote did not take the commit as its main, on every round; main here did not move.
  Push(String),
  /// A submodule pins a commit its own repository's branch lacks, and that repository did not take it.
  Submodule {
    path: String,
    branch: String,
    said: String,
  },
  /// Commits on the branch past main whose message is not English by the close's reading.
  Language(Vec<Message>),
  /// Installed units linked into the checkout main stands in run in a folder the commit takes away.
  Running(Vec<String>),
  /// Installed units linked into the checkout main stands in would be left with no one file of their name.
  Unlinked(Vec<String>),
}

impl LandRefusal {
  /// The refusal as the close says it.
  pub fn text(&self, policy: &Policy, main: &str) -> String {
    let said = &policy.landing.refusal;
    let (line, marks): (&str, Vec<(&str, String)>) = match self {
      Self::Open(paths) => (
        &said.open,
        vec![("paths", paths.iter().take(10).cloned().collect::<Vec<_>>().join(", "))],
      ),
      Self::Origin { branch, from } => {
        (&said.origin, vec![("branch", branch.clone()), ("from", from.clone())])
      }
      Self::Conflict { files, .. } => (
        &said.conflict,
        vec![("files", if files.is_empty() { said.no_file.clone() } else { files.join(", ") })],
      ),
      Self::Base(why) => (&said.base, vec![("said", why.clone())]),
      Self::Overwrite { at, files } => {
        (&said.overwrite, vec![("at", at.clone()), ("files", files.join(", "))])
      }
      Self::Check(why) => (&said.check, vec![("said", why.clone())]),
      Self::Moving => (&said.moving, vec![]),
      Self::Locked => (&said.locked, vec![]),
      Self::Push(why) => (&said.push, vec![("said", why.clone())]),
      Self::Submodule {
        path,
        branch,
        said: why,
      } => (
        &said.submodule,
        vec![("path", path.clone()), ("branch", branch.clone()), ("said", why.clone())],
      ),
      Self::Language(messages) => (
        &said.language,
        vec![(
          "commits",
          messages
            .iter()
            .map(|one| policy.fill(&said.commit, &[("hash", &one.short), ("subject", &one.subject)]))
            .collect::<Vec<_>>()
            .join(&said.joiner),
        )],
      ),
      Self::Running(units) => (&said.running, vec![("units", units.join(", "))]),
      Self::Unlinked(faults) => (&said.unlinked, vec![("faults", faults.join("; "))]),
    };
    let mut values: Vec<(&str, &str)> = marks.iter().map(|(name, value)| (*name, value.as_str())).collect();

    values.push(("main", main));
    policy.fill(line, &values)
  }
}

/// Where a landing ran the repository's setup in the checkout main stands in, and why it failed if it did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LandSetUp {
  pub at: String,
  pub fault: Option<String>,
}

/// A landing that went through: the commit main stands at, the commits it put there, and the setup it ran.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Landed {
  pub commit: String,
  pub landed: Vec<String>,
  pub set_up: Option<LandSetUp>,
}

/// The caller's check of a rebased commit, handed the main commit its last check covered: what failed, or none.
pub type Recheck<'a> = &'a mut dyn FnMut(&str) -> Option<String>;
/// Whether the files main moved over call for the repository's setup.
pub type SetupAfter<'a> = &'a dyn Fn(&[String]) -> bool;
/// A setup run in a checkout.
pub type Setup<'a> = &'a dyn Fn(&Checkout) -> Result<(), String>;

/// How a landing is carried out beyond the rebase and the fast-forward, each part the caller's.
pub struct LandOptions<'a> {
  /// Whether the commit is pushed to the remote's main, which main here moves only after.
  pub push: bool,
  pub remote: &'a str,
  /// Changes open in the checkout that are not the job's, set aside around the rebase.
  pub others_open: &'a [String],
  /// The main commit the caller's own check ran from. Where a round's rebase takes the branch onto a newer
  /// main, `recheck` runs on the rebased commit before main moves, handed the main commit the last check
  /// covered; its answer refuses the landing, and none lets it go on.
  pub checked: Option<String>,
  pub recheck: Option<Recheck<'a>>,
  /// Whether the files main moved over call for the repository's setup in the checkout it stands in.
  pub setup_after: Option<SetupAfter<'a>>,
  /// The setup run there; the repository's own where none is named.
  pub setup: Option<Setup<'a>>,
  /// What brings main up before each round's rebase: the remote's main, where checkouts on nodes push it.
  pub refresh: Option<&'a dyn Fn() -> Result<(), String>>,
  /// The installed units whose links into the checkout main stands in follow its files as it moves; none
  /// leaves them alone.
  pub units: Option<&'a CheckoutUnits<'a>>,
}

impl Default for LandOptions<'_> {
  fn default() -> Self {
    Self {
      push: true,
      remote: "origin",
      others_open: &[],
      checked: None,
      recheck: None,
      setup_after: None,
      setup: None,
      refresh: None,
      units: None,
    }
  }
}

impl Checkout {
  /// The branch a job's branch was made from, as written down when it was made; none where nothing was.
  pub fn branch_made_from(&self) -> Option<String> {
    let branch = self.current_branch()?;

    self.config_value(&branch_from_key(&branch))
  }

  fn config_value(&self, key: &str) -> Option<String> {
    let read = self.git(&["config", "--get", key]);

    (read.ok && !read.out.is_empty()).then_some(read.out)
  }

  /// The branch whose commits landing the job's branch here on `main` would carry across, or none where it
  /// carries only its own. The branch it was made from is the one written down as it was made; a branch made
  /// before that was written is read against each branch it could then have been made from. One of them is
  /// foreign where the commit it shares with the job's branch is not on `main`.
  pub fn foreign_origin(&self, main: &str) -> Option<String> {
    let branch = self.current_branch().filter(|branch| is_work_branch(Some(branch)))?;

    // A main that is not there is the rebase's to refuse, in its own words.
    if !self.branch_exists(main) {
      return None;
    }

    let candidates: Vec<Option<String>> = match self.config_value(&branch_from_key(&branch)) {
      Some(recorded) => vec![Some(recorded)],
      None => {
        let remote_head = self.git(&["symbolic-ref", "--quiet", "--short", "refs/remotes/origin/HEAD"]);

        vec![
          self.config_value(&branch_main_key(&branch)),
          remote_head.ok.then(|| {
            remote_head
              .out
              .find('/')
              .map_or(remote_head.out.clone(), |at| remote_head.out[at + 1..].to_owned())
          }),
          Some("main".to_owned()),
          Some("master".to_owned()),
        ]
      }
    };
    let mut seen: Vec<String> = Vec::new();

    for from in candidates.into_iter().flatten() {
      if seen.contains(&from) {
        continue;
      }
      seen.push(from.clone());

      if from == main || is_work_branch(Some(&from)) || !self.branch_exists(&from) {
        continue;
      }

      let shared = self.git(&["merge-base", "HEAD", &from]);

      if shared.ok && !self.is_ancestor(&shared.out, main) {
        return Some(from);
      }
    }

    None
  }

  /// Rebases the branch it stands on onto main. A branch made from another branch whose commits main lacks is
  /// refused before anything moves. A conflict is aborted and refused with its files. `others_open` names
  /// changes open here that are not the job's — what was open in a base checkout before its job began — which
  /// are set aside around the rebase and put back rather than refused. Answers the commit HEAD stands at.
  pub fn rebase_on_main(&self, main: &str, others_open: &[String]) -> Result<String, LandRefusal> {
    if let Some(from) = self.foreign_origin(main) {
      return Err(LandRefusal::Origin {
        branch: self.current_branch().unwrap_or_else(|| "HEAD".to_owned()),
        from,
      });
    }

    // A file git has never seen is a scratch file as often as it is work, and a rebase leaves it where it
    // stands; a changed tracked file would land without its change.
    let open = self.status_paths(false);
    let own: Vec<String> = open.iter().filter(|path| !others_open.contains(path)).cloned().collect();

    if !own.is_empty() {
      return Err(LandRefusal::Open(own));
    }

    if !self.is_ancestor(main, "HEAD") {
      let mut args = vec!["rebase", "-q"];

      if !open.is_empty() {
        args.push("--autostash");
      }
      args.push(main);

      let rebased = self.git(&args);

      if !rebased.ok {
        let files = self.git(&["diff", "--name-only", "--diff-filter=U"]);

        self.git(&["rebase", "--abort"]);

        return Err(LandRefusal::Conflict {
          files: files.out.split('\n').filter(|file| !file.is_empty()).map(str::to_owned).collect(),
          said: if rebased.err.is_empty() { rebased.out } else { rebased.err },
        });
      }
    }

    must(self.git(&["rev-parse", "HEAD"]), "could not read HEAD").map_err(LandRefusal::Base)
  }

  /// The name of the lock one repository's landings take, so two of them never fast-forward main at once.
  pub fn landing_lock_name(&self) -> String {
    let common = self.git(&["rev-parse", "--path-format=absolute", "--git-common-dir"]);

    format!("land:{}", if common.out.is_empty() { resolved(&self.cwd) } else { common.out })
  }

  // Each submodule commit the landed range pins anew, pushed to its own repository's branch where that branch
  // does not hold it yet, before this repository's main moves, so no pin it publishes names a commit its
  // submodule's repository lacks. A submodule not checked out here is brought first, which fetches a pin its
  // repository already holds and fails on one it does not.
  fn publish_submodule_pins(&self, before: &str, head: &str) -> Result<(), LandRefusal> {
    let raw = self.git(&["diff", "--raw", "--no-renames", "--no-abbrev", before, head]);

    for line in raw.out.lines() {
      let Some((meta, path)) = line.split_once('\t') else {
        continue;
      };
      let fields: Vec<&str> = meta.split_whitespace().collect();
      let [_, "160000", _, pinned, ..] = fields.as_slice() else {
        continue;
      };
      let pinned: &str = pinned;
      let inside = self.at(&format!("{}/{path}", self.cwd.trim_end_matches('/')));
      let refused = |branch: &str, said: String| LandRefusal::Submodule {
        path: path.to_owned(),
        branch: branch.to_owned(),
        said,
      };
      let checked_out = || inside.run(&["test", "-e", ".git"], &[], None).code == Some(0);

      // Git asked in a folder that is no checkout of its own answers for this repository instead, so nothing
      // below runs until the submodule stands checked out.
      if !checked_out() {
        self.bring_submodules(false).map_err(|said| refused("main", said))?;

        if !checked_out() {
          return Err(refused("main", format!("{path} is not checked out here")));
        }
      }

      let remote_head = inside.git(&["rev-parse", "--abbrev-ref", "origin/HEAD"]);
      let branch = remote_head
        .out
        .strip_prefix("origin/")
        .filter(|_| remote_head.ok)
        .unwrap_or("main")
        .to_owned();

      inside.git(&["fetch", "-q", "origin", &branch]);

      if inside.git(&["merge-base", "--is-ancestor", pinned, &format!("origin/{branch}")]).ok {
        continue;
      }

      let pushed = inside.git(&["push", "-q", "origin", &format!("{pinned}:refs/heads/{branch}")]);

      if !pushed.ok {
        return Err(refused(&branch, if pushed.err.is_empty() { pushed.out } else { pushed.err }));
      }
    }

    Ok(())
  }

  // Every file that differs between two commits, each path once: a rename names both of its paths.
  fn changed_between(&self, from: &str, to: &str) -> Vec<String> {
    let names = self.git(&["diff", "--name-only", "--no-renames", from, to]);

    if names.ok {
      names.out.split('\n').filter(|name| !name.is_empty()).map(str::to_owned).collect()
    } else {
      Vec::new()
    }
  }

  // The files open in the checkout at `at` that fast-forwarding it from `before` to `head` would overwrite: a
  // file the commits change, or one standing where they put a folder or inside where they put a file. A
  // submodule is left out, as git leaves one standing open when the pin it stands under moves.
  fn overwritten_at(&self, at: &str, before: &str, head: &str) -> Vec<String> {
    let raw = self.git(&["diff", "--raw", "--no-renames", "--no-abbrev", before, head]);
    let changed: Vec<&str> = raw
      .out
      .lines()
      .filter_map(|line| line.split_once('\t'))
      .filter(|(meta, _)| !meta.split_whitespace().take(2).any(|mode| mode.ends_with("160000")))
      .map(|(_, path)| path)
      .collect();

    self
      .at(at)
      .open_paths()
      .into_iter()
      .filter(|open| {
        changed.iter().any(|path| {
          open == path || open.starts_with(&format!("{path}/")) || path.starts_with(&format!("{open}/"))
        })
      })
      .collect()
  }

  /// The rounds of a landing, run inside whatever holds the repository's landing: up to three rebases, for a
  /// main that moves in between or a remote that refuses the push. The commit goes to the remote's main first;
  /// main here moves, the commits landed are written down under `records` for the job at work here and the
  /// checkout main stands in is set up again only once the remote took it. Commit messages are read for
  /// English by the close's own reading.
  pub async fn land_rounds(
    &self,
    policy: &Policy,
    records: &Path,
    main: &str,
    mut options: LandOptions<'_>,
  ) -> Result<Landed, LandRefusal> {
    let english = &policy.close.english;
    let set_aside = Regex::new(&english.set_aside).map_err(|why| LandRefusal::Base(why.to_string()))?;
    let mut checked = options.checked.take();
    // Why the latest round did not land, said once no round is left.
    let mut last = LandRefusal::Moving;

    for _ in 0..ROUNDS {
      if let Some(refresh) = options.refresh {
        refresh().map_err(LandRefusal::Base)?;
      }

      let head = self.rebase_on_main(main, options.others_open)?;
      let before = must(self.git(&["rev-parse", main]), &format!("could not read {main}"))
        .map_err(LandRefusal::Base)?;

      // Main moved between the rebase and reading it: another round takes it in.
      if !self.is_ancestor(&before, &head) {
        last = LandRefusal::Moving;
        continue;
      }

      // What goes into git is English only, in every repository: a commit that does not read as English is
      // refused here, the last word before anything is pushed or moved.
      let foreign: Vec<Message> = self
        .messages_past(std::slice::from_ref(&before))
        .into_iter()
        .filter(|one| !reads_as_english(&set_aside, english.latin_per_hangul, &one.text))
        .collect();

      if !foreign.is_empty() {
        return Err(LandRefusal::Language(foreign));
      }

      if let (Some(recheck), Some(since)) = (options.recheck.as_mut(), checked.as_deref())
        && since != before
      {
        if let Some(failed) = recheck(since) {
          return Err(LandRefusal::Check(failed));
        }

        checked = Some(before.clone());
      }

      let listed = self.git(&["rev-list", "--reverse", &format!("{before}..HEAD")]);
      let commits: Vec<String> =
        listed.out.split('\n').filter(|one| !one.is_empty()).map(str::to_owned).collect();
      let at = self.checked_out_at(main);
      let mut relinks: Vec<Relink> = Vec::new();

      if !commits.is_empty() {
        if options.push {
          self.publish_submodule_pins(&before, &head)?;
        }

        if let Some(at) = &at {
          let files = self.overwritten_at(at, &before, &head);

          if !files.is_empty() {
            return Err(LandRefusal::Overwrite {
              at: at.clone(),
              files,
            });
          }

          if let Some(units) = options.units {
            relinks = units.plan(&self.at(at), &head, true).await.map_err(|held| match held {
              UnitsHeld::Running(units) => LandRefusal::Running(units),
              UnitsHeld::Unmatched(faults) => LandRefusal::Unlinked(faults),
            })?;
          }
        }
      }

      // A plain push takes only a fast-forward of the remote's main, so whichever landing reaches it first wins;
      // a refused one has moved nothing here and rebases again.
      if options.push {
        let pushed = self.git(&["push", "-q", options.remote, &format!("{head}:refs/heads/{main}")]);

        if !pushed.ok {
          last = LandRefusal::Push(if pushed.err.is_empty() { pushed.out } else { pushed.err });
          continue;
        }
      }

      if commits.is_empty() {
        return Ok(Landed {
          commit: head,
          landed: commits,
          set_up: None,
        });
      }

      let moved = match &at {
        None => self.git(&["update-ref", &format!("refs/heads/{main}"), &head, &before]),
        Some(path) => self.at(path).git(&["merge", "-q", "--ff-only", &head]),
      };

      // Main moved here between the rebase and the landing, or its checkout would not move: the remote may hold
      // the commit already, and the next round follows it and moves main again.
      if !moved.ok {
        last = LandRefusal::Base(if moved.err.is_empty() { moved.out } else { moved.err });
        continue;
      }

      record_landed(policy, records, &self.cwd, &commits);

      if let (Some(at), Some(units)) = (&at, options.units) {
        units.follow(&self.at(at), &relinks).await;
      }

      let set_up = at.and_then(|at| self.set_up_after_landing(at, &before, &head, &options));

      return Ok(Landed {
        commit: head,
        landed: commits,
        set_up,
      });
    }

    Err(last)
  }

  // The checkout main was fast-forwarded in, set up again where the files it moved over call for it, and its
  // submodules brought to the pins main now holds otherwise; none where nothing was run or all went well without
  // a setup.
  fn set_up_after_landing(
    &self,
    at: String,
    before: &str,
    head: &str,
    options: &LandOptions<'_>,
  ) -> Option<LandSetUp> {
    let there = self.at(&at);

    if let Some(setup_after) = options.setup_after
      && setup_after(&self.changed_between(before, head))
    {
      let ran = match options.setup {
        Some(setup) => setup(&there),
        None => there.run_setup(SETUP_RUNTIME),
      };

      return Some(LandSetUp {
        at,
        fault: ran.err(),
      });
    }

    // The files moved without an install to redo, but a pin may have moved with them.
    there.bring_submodules(false).err().map(|why| LandSetUp {
      at,
      fault: Some(why),
    })
  }
}

// The commits a landing put on main, added to what this worktree already landed.
fn record_landed(policy: &Policy, records: &Path, cwd: &str, commits: &[String]) {
  let path = landed_path(records, cwd);
  let mut all = landed_commits(records, cwd);

  for commit in commits {
    if !all.contains(commit) {
      all.push(commit.clone());
    }
  }

  let written = path
    .parent()
    .map_or(Ok(()), std::fs::create_dir_all)
    .and_then(|()| std::fs::write(&path, serde_json::to_string(&all).unwrap_or_default()));

  if let Err(why) = written {
    eprintln!(
      "{}",
      policy.fill(&policy.landing.home.unrecorded, &[("cwd", cwd), ("why", &why.to_string())])
    );
  }
}

/// What holds the repository's landing while it runs.
pub enum LandingHold {
  /// The landing lock in this lock folder, the one every landing of the repository on this machine takes.
  Here(PathBuf),
  /// The operations machine's, through `node-land`.
  Home(HomeLanding),
}

/// A job's branch landed on main under its hold, its landed commits written down under `records`.
pub async fn land_on_main(
  checkout: &Checkout,
  policy: &Policy,
  records: &Path,
  main: &str,
  options: LandOptions<'_>,
  hold: &mut LandingHold,
) -> Result<Landed, LandRefusal> {
  match hold {
    LandingHold::Here(locks) => {
      let lock = locks.join(lock_file_name(&checkout.landing_lock_name()));
      let Some(_held) = hold_within(&lock, LAND_LOCK_WAIT).await else {
        return Err(LandRefusal::Locked);
      };

      checkout.land_rounds(policy, records, main, options).await
    }
    LandingHold::Home(home) => {
      if !home.take() {
        return Err(LandRefusal::Locked);
      }

      let outcome = checkout.land_rounds(policy, records, main, options).await;

      home.let_go(policy, &outcome);
      outcome
    }
  }
}

/// A landing from a checkout on a work node: the repository's landing lock is the operations machine's, held
/// there by `node-land` for as long as the node's landing runs; once the node has pushed, the same verb has the
/// base checkout there follow the remote's main and, where the close asks, writes the commit down as waiting to
/// go live.
pub struct HomeLanding {
  record: Option<String>,
  asked: Option<Child>,
  lines: Option<Lines<BufReader<ChildStdout>>>,
  /// What the operations machine said of the landing, line by line, for the caller to pass on.
  pub said: Vec<String>,
}

impl HomeLanding {
  /// The hold of a node's landing, the commit written down as waiting to go live for this reason where one is
  /// given.
  pub fn new(record: Option<String>) -> Self {
    Self {
      record,
      asked: None,
      lines: None,
      said: Vec::new(),
    }
  }

  // The operations machine asked to take the repository's landing lock, answering once it says it holds it;
  // false where it never did.
  fn take(&mut self) -> bool {
    let Ok(program) = std::env::current_exe() else { return false };
    let mut command = Command::new(program);

    command.arg("node-land");

    if let Some(record) = &self.record {
      command.args(["--record", record]);
    }

    let Ok(mut asked) =
      command.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn()
    else {
      return false;
    };
    let Some(stdout) = asked.stdout.take() else { return false };
    let mut lines = BufReader::new(stdout).lines();
    let mut held = false;

    for line in lines.by_ref() {
      let Ok(line) = line else { break };

      if line == "held" {
        held = true;
        break;
      }

      self.said.push(line);
    }

    if !held {
      drop(asked.stdin.take());
      let _ = asked.wait();
      return false;
    }

    self.asked = Some(asked);
    self.lines = Some(lines);
    true
  }

  // What came of the landing handed back — the commit landed, or nothing — so the operations machine follows and
  // writes down only what was pushed, and what it said then kept.
  fn let_go(&mut self, policy: &Policy, outcome: &Result<Landed, LandRefusal>) {
    let Some(mut asked) = self.asked.take() else { return };

    if let Some(mut input) = asked.stdin.take() {
      let word = match outcome {
        Ok(landed) => format!("landed {}\n", landed.commit),
        Err(_) => "nothing\n".to_owned(),
      };
      let _ = input.write_all(word.as_bytes());
    }

    for line in self.lines.take().into_iter().flatten() {
      let Ok(line) = line else { break };

      self.said.push(line);
    }

    let ended = asked.wait().is_ok_and(|status| status.success());

    if !ended && outcome.is_ok() {
      self.said.push(policy.landing.home.not_taken_in.clone());
    }
  }
}
