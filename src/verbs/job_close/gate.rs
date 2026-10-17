//! A job's close as its gate (docs/verbs.md, "A job's close"): the change check and the test count, the
//! suite, and the commit recorded as waiting to go live only through a suite that passed. The one rule of it —
//! nothing the suite did not pass ever waits to go live — is one function over hands it is given, so a test
//! can hand it a suite that failed and read back that nothing was recorded at all. What failed or never
//! started runs once more before the suite is judged.

use std::future::Future;

use job_policy::Policy;
use job_testbed::{Failure, Outcome};

/// The exit code a close ends with where its turn never came: the caller runs it again and waits on from its
/// place.
pub const NOT_RUN_EXIT_CODE: u8 = 75;

/// Whether the suite runs before the commit is handed over, and why not where it does not.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuiteChoice {
  Run,
  Skip(String),
}

/// One run of the suite as its log reads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuiteRun {
  pub outcome: Outcome,
  /// The suite's own last line; none where the run wrote none.
  pub verdict: Option<String>,
  pub failures: Vec<Failure>,
  pub failed_units: Vec<String>,
  pub unstarted_units: Vec<String>,
}

impl SuiteRun {
  /// A run that wrote no verdict is read as a failure whatever its outcome says.
  pub fn passed(&self) -> bool {
    self.outcome == Outcome::Passed && self.verdict.is_some()
  }

  /// What a run once more runs: the units that failed and those that never started.
  pub fn again(&self) -> Vec<String> {
    self.failed_units.iter().chain(&self.unstarted_units).cloned().collect()
  }
}

/// Why a check did not pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckFault {
  /// Its turn did not come within its wait.
  NotRun(String),
  Failed(String),
}

/// What the gate reaches outside itself, so a test can stand in for each of it.
pub trait CloseHands {
  fn suite_choice(&self) -> SuiteChoice;
  /// Whether this commit is already live or already waiting, so there is nothing to hand over.
  fn already_handed_over(&mut self) -> impl Future<Output = Option<String>>;
  /// The change check and the test count, run before the suite: none where they passed.
  fn static_check(&mut self) -> impl Future<Output = Option<CheckFault>>;
  /// The suite over what the change reaches; `again` runs once more what the last run failed or never
  /// started.
  fn suite(&mut self, again: bool) -> impl Future<Output = SuiteRun>;
  /// Lands the commit on main where the close runs on a job's branch: none where it has nothing to land, the
  /// refusal where it could not. A landing that went through answers the tests its check again on a newer
  /// main passed only when run once more.
  fn land(&mut self) -> impl Future<Output = Option<Result<Vec<String>, String>>>;
  /// Records the commit as waiting to go live; the line that says so.
  fn record(&mut self) -> impl Future<Output = String>;
  fn say(&self, line: &str);
}

/// What a job's close came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordOutcome {
  Recorded,
  CheckFailed,
  SuiteFailed,
  LandFailed,
  NothingNew,
}

/// A close's end: the code it exits with, what it came to, and what the suite said.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecordResult {
  /// Nought only where its commit is handed over or was already.
  pub code: u8,
  pub outcome: RecordOutcome,
  /// The suite's own last line, or what stands in for it where the run wrote none.
  pub verdict: Option<String>,
  /// The tests the run named as failing.
  pub failed: Vec<String>,
  /// The units those failures stand in, which are where the mending starts.
  pub failed_units: Vec<String>,
}

fn ended(code: u8, outcome: RecordOutcome, verdict: Option<String>) -> RecordResult {
  RecordResult {
    code,
    outcome,
    verdict,
    failed: Vec::new(),
    failed_units: Vec::new(),
  }
}

/// The tests that failed the first run and not the last, each as `test (unit)`.
pub fn flaky(first: &SuiteRun, last: &SuiteRun) -> Vec<String> {
  first
    .failures
    .iter()
    .filter(|failure| !last.failures.contains(failure))
    .map(|failure| format!("{} ({})", failure.test, failure.unit))
    .collect()
}

/// The gate's words, filled with the close's name, what was not done and the log.
pub struct Said<'a> {
  pub policy: &'a Policy,
  pub log: &'a str,
}

impl Said<'_> {
  fn gate(&self, template: &str, more: &[(&str, &str)]) -> String {
    let words = &self.policy.close;
    let mut all = vec![
      ("name", words.close.name.as_str()),
      ("stopped", words.gate.stopped.as_str()),
      ("log", self.log),
    ];

    all.extend_from_slice(more);
    self.policy.fill(template, &all)
  }

  fn verdict_of(&self, run: &SuiteRun) -> String {
    let code = match run.outcome {
      Outcome::Passed => "0",
      Outcome::Failed => "1",
      Outcome::NotRun => "75",
    };

    run
      .verdict
      .clone()
      .unwrap_or_else(|| self.policy.fill(&self.policy.close.close.no_verdict, &[("code", code)]))
  }
}

// The checks, then the suite: a check that failed with its line, a suite skipped with its reason, passed with
// its verdict and the tests that passed only when run once more, or failed with what failed. The checks run
// whether or not the suite does.
async fn suite_gate(
  hands: &mut impl CloseHands,
  said: &Said<'_>,
) -> Result<Option<(String, Vec<String>)>, RecordResult> {
  let words = &said.policy.close.gate;

  hands.say(&said.gate(&words.check, &[]));

  match hands.static_check().await {
    None => {}
    Some(CheckFault::NotRun(check)) => {
      hands.say(&check);
      hands.say(&said.gate(&words.check_not_run, &[]));
      return Err(ended(NOT_RUN_EXIT_CODE, RecordOutcome::CheckFailed, Some(check)));
    }
    Some(CheckFault::Failed(check)) => {
      hands.say(&check);
      hands.say(&said.gate(&words.check_failed, &[]));
      return Err(ended(1, RecordOutcome::CheckFailed, Some(check)));
    }
  }

  if let SuiteChoice::Skip(why) = hands.suite_choice() {
    hands.say(&said.gate(&words.skipped, &[("why", &why)]));
    return Ok(None);
  }

  hands.say(&said.gate(&words.suite, &[]));

  let first = hands.suite(false).await;

  hands.say(&said.verdict_of(&first));

  // What failed or never started runs once more, alone, before the close is judged: a test that fails only now
  // and then lets the change through once the next run passes.
  let last = if first.passed() || first.outcome == Outcome::NotRun || first.again().is_empty() {
    first.clone()
  } else {
    hands.say(&said.gate(&words.once_more, &[("files", &first.again().join(", "))]));

    let again = hands.suite(true).await;

    hands.say(&said.verdict_of(&again));
    again
  };
  let verdict = said.verdict_of(&last);

  // A suite whose turn never came tested nothing and failed nothing, and one that left units unstarted within
  // its guard and failed none has no result for them: the caller runs again at once.
  if last.outcome == Outcome::NotRun {
    hands.say(&if last.unstarted_units.is_empty() {
      said.gate(&words.suite_not_run, &[])
    } else {
      said.gate(&words.unfinished, &[("count", &last.unstarted_units.len().to_string())])
    });

    return Err(ended(NOT_RUN_EXIT_CODE, RecordOutcome::SuiteFailed, Some(verdict)));
  }

  if !last.passed() {
    if !last.failed_units.is_empty() {
      hands.say(&said.policy.fill(&words.failed_files, &[("files", &last.failed_units.join(", "))]));
    }

    hands.say(&said.gate(&words.suite_failed, &[]));

    return Err(RecordResult {
      code: 1,
      outcome: RecordOutcome::SuiteFailed,
      verdict: Some(verdict),
      failed: last.failures.iter().map(|failure| failure.test.clone()).collect(),
      failed_units: last.failed_units.clone(),
    });
  }

  let names = flaky(&first, &last);

  if !names.is_empty() {
    hands.say(&said.gate(&words.flaky, &[("tests", &names.join(", ")), ("record", said.log)]));
  }

  Ok(Some((verdict, names)))
}

/// A job's close: the suite as its own gate, then the branch landed where it is a job's, then the commit
/// recorded as waiting to go live. The deployment is not the job's; a suite that failed records nothing, so
/// nothing it did not pass is ever waiting to go live. Its last line names each test that passed only when run
/// once more, for the job to report as flaky.
pub async fn run_suite_then_record(hands: &mut impl CloseHands, said: &Said<'_>) -> RecordResult {
  let words = &said.policy.close.gate;

  if let Some(handed) = hands.already_handed_over().await {
    hands.say(&said.policy.fill(&words.handed, &[("handed", &handed)]));
    return ended(0, RecordOutcome::NothingNew, None);
  }

  let (verdict, mut flaky) = match suite_gate(hands, said).await {
    Ok(Some((verdict, flaky))) => (Some(verdict), flaky),
    Ok(None) => (None, Vec::new()),
    Err(result) => return result,
  };
  let what = if verdict.is_none() { &words.was_skipped } else { &words.passed };

  // A commit that could not land is not on main, so nothing is recorded for it.
  match hands.land().await {
    Some(Err(refused)) => {
      hands.say(&said.policy.fill(&words.not_landed, &[("what", what), ("refused", &refused)]));
      return ended(1, RecordOutcome::LandFailed, verdict);
    }
    // What the landing's check again found flaky is named beside what the gate's own suite found.
    Some(Ok(again)) => {
      for name in again {
        if !flaky.contains(&name) {
          flaky.push(name);
        }
      }
    }
    None => {}
  }

  let record = hands.record().await;
  let named = if flaky.is_empty() {
    String::new()
  } else {
    said.policy.fill(&words.flaky_named, &[("tests", &flaky.join(", "))])
  };

  hands.say(&format!(
    "{}{named}",
    said.policy.fill(&words.done, &[("what", what), ("record", &record)])
  ));
  ended(0, RecordOutcome::Recorded, verdict)
}
