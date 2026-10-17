//! `job close`: the close of a job in the repository its checkout belongs to (docs/verbs.md, "A job's
//! close"), run once at its end. It runs itself again as a command in the repository's close line through
//! bmo-testbed, so one close of a repository checks and lands at a time on the machine; inside the line it
//! rebases a job's branch onto main, runs the change check, the test count and the suite as its gate, lands the
//! branch, and records the commit it closed on as waiting to go live. It never deploys.

pub mod gate;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use clap::Args;
use job_testbed::{Check, Close as CloseLine, CloseTurn, Count, Outcome, PendingOf, Suite, Testbed};

use self::gate::{
  CheckFault, CloseHands, NOT_RUN_EXIT_CODE, Said, SuiteChoice, SuiteRun, run_suite_then_record,
};
use super::checkout_setup::checkout_of;
use super::job_guard::git_language::EnglishTest;
use super::land::{home_said, land_held, landing_of};
use super::node_land::{record_pending, setup_inputs_moved};
use super::stand::{Answer, JobProcess, RunningJob, Stand, VerbError, last_job};
use crate::job_run::checkout::landing::{LandOptions, LandingHold, Recheck};
use crate::job_run::checkout::units::{CheckoutUnits, TestbedLinks};
use crate::job_run::checkout::{Checkout, is_work_branch};
use crate::job_run::worktree::{preserved_work, recorded_baseline, worktree_state};
use crate::order_run::hold::SETUP_RUNTIME;
use crate::queue::units::testbed_failed;
use crate::queue::workers::workers;

/// The close's own words.
#[derive(Args, Debug, Default, Clone)]
pub struct JobCloseArgs {
  /// Why this goes live, in English.
  #[arg(long, allow_hyphen_values = true)]
  pub then: Option<String>,
  /// Why the change reaches nothing the suite runs, in English, where the suite is skipped.
  #[arg(long, num_args = 0..=1, default_missing_value = "", allow_hyphen_values = true)]
  pub skip_suite: Option<String>,
  /// The close standing in its repository's close line already, as the close runs itself there.
  #[arg(long, hide = true)]
  pub in_line: bool,
}

fn say(line: &str) {
  eprintln!("{line}");
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
  Command::new("git")
    .args(args)
    .current_dir(root)
    .stdin(Stdio::null())
    .stderr(Stdio::null())
    .output()
    .ok()
    .filter(|done| done.status.success())
    .map(|done| String::from_utf8_lossy(&done.stdout).trim().to_owned())
}

/// Why a tree is not handed over: an unpushed commit is not deployable.
pub fn tree_fault(stand: &Stand, head: &str, on_remote: bool, remote_branch: &str) -> Option<String> {
  let said = &stand.policy.close.tree;

  if head.is_empty() {
    return Some(said.no_commit.clone());
  }

  (!on_remote).then(|| {
    stand
      .policy
      .fill(&said.not_pushed, &[("head", &head[..head.len().min(12)]), ("remote", remote_branch)])
  })
}

/// The refusal of a reason holding Hangul past the close's set-aside names, the first one where both do; none
/// where every reason given is English.
pub fn reasons_refusal(policy: &job_policy::Policy, args: &JobCloseArgs) -> Option<String> {
  let english = EnglishTest::of(policy);

  [("then", &args.then), ("skip-suite", &args.skip_suite)]
    .into_iter()
    .find(|(_, reason)| reason.as_deref().is_some_and(|reason| !english.passes(reason)))
    .map(|(flag, _)| policy.fill(&policy.close.language.reason, &[("flag", flag)]))
}

/// The refusal of the job's own commits, those on HEAD that none of `past` holds, where any message holds
/// Hangul past the close's set-aside names: each such commit named, and `main` the point a rebase rewords them
/// against. None where every message is English.
pub fn commits_refusal(
  policy: &job_policy::Policy,
  checkout: &Checkout,
  past: &[String],
  main: &str,
) -> Option<String> {
  let english = EnglishTest::of(policy);
  let words = &policy.close.language;
  let named: Vec<String> = checkout
    .messages_past(past)
    .into_iter()
    .filter(|message| !english.passes(&message.text))
    .map(|message| policy.fill(&words.commit, &[("hash", &message.short), ("subject", &message.subject)]))
    .collect();

  (!named.is_empty())
    .then(|| policy.fill(&words.commits, &[("commits", &named.join(&words.joiner)), ("main", main)]))
}

/// The newest of the candidate commits HEAD descends from: the one with the fewest commits between it and
/// HEAD. None where HEAD descends from none of them.
pub fn newest_vetted(root: &Path, candidates: &[String]) -> Option<String> {
  candidates
    .iter()
    .filter(|candidate| git(root, &["merge-base", "--is-ancestor", candidate, "HEAD"]).is_some())
    .filter_map(|candidate| {
      let between = git(root, &["rev-list", "--count", &format!("{candidate}..HEAD")])?
        .parse::<u64>()
        .ok()?;

      Some((between, candidate.clone()))
    })
    .min_by_key(|(between, _)| *between)
    .map(|(_, candidate)| candidate)
}

fn refused(stand: &Stand, why: &str) -> VerbError {
  VerbError::Ended {
    code: 1,
    said: stand.policy.fill(&stand.policy.close.close.refused, &[("why", why)]),
  }
}

// The close as the gate reaches it: the checks, the suite and the landing, each a call on bmo-testbed in the
// checkout, every log under the close's own folder.
struct Close<'a> {
  stand: &'a Stand,
  checkout: Checkout,
  root: PathBuf,
  why: String,
  skip: Option<String>,
  head: String,
  deployed: Option<String>,
  pending: Vec<String>,
  job: Option<RunningJob>,
  others_open: Vec<String>,
  pushed: bool,
  main: Option<String>,
  /// What holds the landing: here, or on a work node the operations machine's, which records what it lands.
  hold: Option<LandingHold>,
  /// What the operations machine said of a landing it held, the record's line among it.
  home_said: Option<Vec<String>>,
  since: Option<String>,
  logs: PathBuf,
  runs: usize,
}

impl Close<'_> {
  // A fresh log file of the close's folder for one call.
  fn log(&mut self, what: &str) -> PathBuf {
    self.runs += 1;
    self.logs.join(format!("{}-{what}.log", self.runs))
  }

  async fn check_from(&mut self, since: Option<String>) -> Option<CheckFault> {
    let testbed = &self.stand.ground.testbed;
    let checked = Check {
      checkout: self.root.clone(),
      since: since.clone(),
      log: self.log("check"),
    };
    let faulted = |what: &str, outcome: Result<Outcome, String>, log: &Path| match outcome {
      Ok(Outcome::Passed) => None,
      Ok(Outcome::NotRun) => Some(CheckFault::NotRun(format!(
        "{what}: not run — its turn did not come (log: {})",
        log.display()
      ))),
      Ok(Outcome::Failed) => Some(CheckFault::Failed(format!("{what}: FAILED (log: {})", log.display()))),
      Err(why) => Some(CheckFault::Failed(format!("{what}: FAILED — {why}"))),
    };

    if let Some(fault) = faulted(
      "change check",
      testbed.check(&checked).await.map_err(|why| why.to_string()),
      &checked.log,
    ) {
      return Some(fault);
    }

    since.as_ref()?;

    let counted = Count {
      checkout: self.root.clone(),
      since,
      log: self.log("count"),
    };

    faulted("test count", testbed.count(&counted).await.map_err(|why| why.to_string()), &counted.log)
  }

  async fn suite_from(&mut self, since: Option<String>, again: bool) -> SuiteRun {
    let testbed = &self.stand.ground.testbed;
    let suite = Suite {
      checkout: self.root.clone(),
      since,
      again,
      log: self.log("suite"),
      ..Suite::default()
    };
    let outcome = match testbed.suite(&suite).await {
      Ok(outcome) => outcome,
      Err(why) => {
        say(&format!("suite: {why}"));
        Outcome::Failed
      }
    };
    let read = testbed.read_log(&suite.log).await.ok();

    SuiteRun {
      outcome,
      verdict: read.as_ref().map(|read| read.verdict.clone()).filter(|verdict| !verdict.is_empty()),
      failures: read.as_ref().map(|read| read.failures.clone()).unwrap_or_default(),
      failed_units: read.as_ref().map(|read| read.failed_units.clone()).unwrap_or_default(),
      unstarted_units: read.map(|read| read.unstarted_units).unwrap_or_default(),
    }
  }

  // The landing's check again, where its rebase took the branch onto a newer main than the one this close
  // checked: the setup the rebased tree needs, the checks, and the suite from the main last checked, unless the
  // close skipped it. Where it passed, the tests that passed only when run once more; otherwise what failed.
  async fn check_again(&mut self, from: &str) -> Result<Vec<String>, String> {
    let policy = &self.stand.policy;
    let main = self.main.clone().unwrap_or_default();

    say(&policy.fill(
      &policy.close.close.check_again,
      &[("main", &main), ("from", &from[..from.len().min(12)])],
    ));
    self.checkout.run_setup(SETUP_RUNTIME)?;

    if let Some(fault) = self.check_from(Some(from.to_owned())).await {
      return Err(match fault {
        CheckFault::NotRun(said) | CheckFault::Failed(said) => said,
      });
    }

    if self.skip.is_some() {
      return Ok(Vec::new());
    }

    let first = self.suite_from(Some(from.to_owned()), false).await;
    let last = if first.passed() || first.again().is_empty() {
      first.clone()
    } else {
      self.suite_from(Some(from.to_owned()), true).await
    };

    if last.passed() {
      return Ok(gate::flaky(&first, &last));
    }

    let said = &policy.close.close;
    let code = if last.outcome == Outcome::NotRun { "75" } else { "1" };

    Err(format!(
      "{}{}",
      last.verdict.clone().unwrap_or_else(|| policy.fill(&said.no_verdict, &[("code", code)])),
      if last.failed_units.is_empty() {
        String::new()
      } else {
        policy.fill(&said.failed_files, &[("files", &last.failed_units.join(", "))])
      },
    ))
  }
}

impl CloseHands for Close<'_> {
  fn suite_choice(&self) -> SuiteChoice {
    self.skip.clone().map_or(SuiteChoice::Run, SuiteChoice::Skip)
  }

  async fn already_handed_over(&mut self) -> Option<String> {
    let said = &self.stand.policy.close.gate;

    if self.deployed.as_deref() == Some(self.head.as_str()) {
      Some(said.deployed.clone())
    } else {
      self.pending.contains(&self.head).then(|| said.waiting.clone())
    }
  }

  async fn static_check(&mut self) -> Option<CheckFault> {
    let since = self.since.clone();

    self.check_from(since).await
  }

  async fn suite(&mut self, again: bool) -> SuiteRun {
    let since = self.since.clone();

    self.suite_from(since, again).await
  }

  async fn land(&mut self) -> Option<Result<Vec<String>, String>> {
    let main = self.main.clone()?;
    let stand = self.stand;
    let policy = &stand.policy;
    let checked = self.since.clone();
    let checkout = self.checkout.clone();
    let others_open = self.others_open.clone();
    let pushed = self.pushed;
    let mut hold = self.hold.take();
    let mut flaky = Vec::new();
    let mut again = |from: &str| -> Option<String> {
      let ran =
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(self.check_again(from)));

      match ran {
        Ok(found) => {
          flaky.extend(found);
          None
        }
        Err(failed) => Some(failed),
      }
    };
    let refresh = || checkout.follow_remote_main(&main);
    let links = TestbedLinks(&stand.ground.testbed);
    let units = CheckoutUnits {
      links: &links,
      policy,
    };
    let options = LandOptions {
      others_open: &others_open,
      checked: checked.clone(),
      recheck: checked.is_some().then_some(&mut again as Recheck<'_>),
      setup_after: Some(&|files: &[String]| setup_inputs_moved(files)),
      refresh: pushed.then_some(&refresh as &dyn Fn() -> Result<(), String>),
      units: Some(&units),
      ..LandOptions::default()
    };
    let landed = land_held(&checkout, policy, &stand.ground.documents, &main, options, &mut hold).await;
    let said = &policy.close.close;

    if let Some(LandingHold::Home(_)) = &hold {
      self.home_said = Some(home_said(&hold));
    }

    for line in home_said(&hold) {
      say(&line);
    }

    match landed {
      Err(refusal) => Some(Err(refusal.text(policy, &main))),
      Ok(landed) => {
        let count = match landed.landed.len() {
          0 => String::new(),
          1 => said.one.clone(),
          many => policy.fill(&said.many, &[("count", &many.to_string())]),
        };

        say(&policy.fill(
          &said.landed,
          &[
            ("commit", &landed.commit[..landed.commit.len().min(12)]),
            ("main", &main),
            ("count", &count),
          ],
        ));

        if let Some(set_up) = landed.set_up {
          say(&match set_up.fault {
            None => policy.fill(&said.set_up, &[("at", &set_up.at), ("main", &main)]),
            Some(fault) => {
              policy.fill(&said.set_up_failed, &[("at", &set_up.at), ("main", &main), ("fault", &fault)])
            }
          });
        }

        self.head = landed.commit;

        Some(Ok(flaky))
      }
    }
  }

  async fn record(&mut self) -> String {
    // A landing the operations machine held was recorded there, and its last word is the record's line.
    if let Some(said) = &self.home_said {
      return said.last().cloned().unwrap_or_else(|| self.stand.policy.close.close.unrecorded.clone());
    }

    let root = self.root.to_string_lossy().into_owned();

    record_pending(self.stand, &root, &self.head, self.job.as_ref(), None, &self.why).await
  }

  fn say(&self, line: &str) {
    say(line);
  }
}

/// The close of the checkout `here` stands in: refused where its words do not hold, run again in its
/// repository's close line, and checked and landed there; the code it ends with.
pub async fn job_close(
  stand: &Stand,
  process: &JobProcess,
  args: JobCloseArgs,
  here: &Path,
) -> Result<Answer, VerbError> {
  let policy = &stand.policy;
  let usage = &policy.close.usage;
  let Some(why) = args.then.clone().filter(|why| !why.trim().is_empty()) else {
    return Err(VerbError::Ended {
      code: 2,
      said: usage.close.clone(),
    });
  };

  if args.skip_suite.as_deref().is_some_and(|reason| reason.trim().is_empty()) {
    return Err(VerbError::Ended {
      code: 2,
      said: usage.skip_reason.clone(),
    });
  }

  if let Some(refusal) = reasons_refusal(policy, &args) {
    return Err(refused(stand, &refusal));
  }

  let root = checkout_of(here);

  if !args.in_line {
    return in_close_line(stand, &root, &args, &why).await;
  }

  let checkout = Checkout::local(root.to_string_lossy());
  let rows = workers(&stand.db, &stand.ground.source).await?;
  let on_branch = is_work_branch(checkout.current_branch().as_deref());
  // A close run on a job's own branch lands the branch itself: rebased onto main before the suite, so the suite
  // runs on what would land, and landed once it has passed. Main is read off the base checkout.
  let (landing_main, pushed, hold) = landing_of(stand, &checkout, &rows, Some(why.trim().to_owned()));
  let main = on_branch.then_some(landing_main);
  let testbed = &stand.ground.testbed;
  let deployed = testbed.deployed(&root).await.map_err(testbed_failed)?.map(|deployed| deployed.commit);
  let pending: Vec<String> = testbed
    .pending(PendingOf::All)
    .await
    .map_err(testbed_failed)?
    .into_iter()
    .map(|pending| pending.commit)
    .collect();
  let candidates: Vec<String> = deployed.iter().cloned().chain(pending.iter().cloned()).collect();
  // The job's own commits are read before anything moves: on its branch, what neither main nor main's upstream
  // holds; on main itself, what is past the newest commit live or waiting to go live.
  let (past, against) = match &main {
    Some(main) => (vec![main.clone(), format!("{main}@{{upstream}}")], main.clone()),
    None => {
      let vetted = newest_vetted(&root, &candidates);

      (vetted.iter().cloned().collect(), vetted.unwrap_or_default())
    }
  };

  if let Some(refusal) = commits_refusal(policy, &checkout, &past, &against) {
    return Err(refused(stand, &refusal));
  }

  // What a base checkout already had open when its job began is somebody else's, set aside by the landing.
  let others_open = match &main {
    Some(_) => {
      let now = worktree_state(&checkout).map_err(|why| refused(stand, &why))?;

      preserved_work(&recorded_baseline(&stand.ground.documents, &checkout.cwd), &now).tracked
    }
    None => Vec::new(),
  };

  if let Some(main) = &main {
    if pushed {
      checkout.follow_remote_main(main).map_err(|why| refused(stand, &why))?;
    }

    checkout
      .rebase_on_main(main, &others_open)
      .map_err(|refusal| refused(stand, &refusal.text(policy, main)))?;

    // The rebase brought in whatever main moved, a lockfile among it, so the setup is brought up to the rebased
    // tree before the suite reads it.
    checkout.run_setup(SETUP_RUNTIME).map_err(|why| refused(stand, &why))?;
  }

  // Only a pushed commit is handed over; a job branch's commit is pushed by its landing.
  let head = git(&root, &["rev-parse", "HEAD"]).filter(|head| head.len() == 40).unwrap_or_default();
  let remote_branch = git(&root, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"])
    .filter(|upstream| !upstream.is_empty())
    .unwrap_or_else(|| "origin/main".to_owned());
  let on_remote = !head.is_empty()
    && (main.is_some() || git(&root, &["merge-base", "--is-ancestor", &head, &remote_branch]).is_some());

  if let Some(fault) = tree_fault(stand, &head, on_remote, &remote_branch) {
    return Err(refused(stand, &fault));
  }

  let job = match process.run_name.as_deref() {
    Some(run) => last_job(&stand.db, run).await?,
    None => None,
  };
  let logs = stand
    .ground
    .testbed_logs
    .clone()
    .unwrap_or_else(|| stand.ground.documents.join("close"))
    .join(format!("close-{}", crate::now_ms()));

  let _ = std::fs::create_dir_all(&logs);

  // Where the change the close judges begins: on a job's branch rebased onto main, main's head, which went
  // through a close of its own; on main, the newest commit HEAD descends from that is live or waiting.
  let since = match &main {
    Some(main) => git(&root, &["rev-parse", "--verify", &format!("{main}^{{commit}}")])
      .filter(|commit| !commit.is_empty()),
    None => newest_vetted(&root, &candidates),
  };
  let shown = logs.to_string_lossy().into_owned();
  let mut close = Close {
    stand,
    checkout,
    root,
    why: why.trim().to_owned(),
    skip: args.skip_suite,
    head,
    deployed,
    pending,
    job,
    others_open,
    pushed,
    main,
    hold,
    home_said: None,
    since,
    logs,
    runs: 0,
  };
  let said = Said {
    policy,
    log: &shown,
  };
  let result = run_suite_then_record(&mut close, &said).await;

  Ok(Answer {
    lines: Vec::new(),
    code: result.code,
  })
}

// The close run again as a command in its repository's close line, its code passed on.
async fn in_close_line(
  stand: &Stand,
  root: &Path,
  args: &JobCloseArgs,
  why: &str,
) -> Result<Answer, VerbError> {
  let program = std::env::current_exe().map_err(|failed| refused(stand, &failed.to_string()))?;
  let mut command =
    vec![program.to_string_lossy().into_owned(), "job".into(), "close".into(), "--in-line".into()];

  command.extend(["--then".to_owned(), why.to_owned()]);

  if let Some(skip) = &args.skip_suite {
    command.extend(["--skip-suite".to_owned(), skip.clone()]);
  }

  let line = CloseLine {
    checkout: root.to_path_buf(),
    command,
  };

  match stand.ground.testbed.close(&line).await.map_err(testbed_failed)? {
    CloseTurn::Ran(code) => Ok(Answer {
      lines: Vec::new(),
      code: u8::try_from(code).unwrap_or(1),
    }),
    CloseTurn::NotRun => Err(VerbError::Ended {
      code: NOT_RUN_EXIT_CODE,
      said: stand.policy.close.close.line_gone.clone(),
    }),
  }
}
