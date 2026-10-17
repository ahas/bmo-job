//! A job's close as its gate (docs/verbs.md, "A job's close"): nothing the suite did not pass is ever recorded
//! as waiting to go live, what failed runs once more before the suite is judged, and a test that passed only
//! then is named in the last line. The hands the gate reaches are stood in for.

mod support;

use std::cell::RefCell;
use std::future::Future;

use bmo_job::verbs::job_close::gate::{
  CheckFault, CloseHands, NOT_RUN_EXIT_CODE, RecordOutcome, Said, SuiteChoice, SuiteRun, flaky,
  run_suite_then_record,
};
use job_policy::Policy;
use job_testbed::{Failure, Outcome};
use support::queue::policy;

const LOG: &str = "/srv/logs/close";
const FLAKE: &str = "server/flake";

fn run(outcome: Outcome, verdict: Option<&str>, failures: &[(&str, &str)]) -> SuiteRun {
  SuiteRun {
    outcome,
    verdict: verdict.map(str::to_owned),
    failures: failures
      .iter()
      .map(|(test, unit)| Failure {
        test: (*test).into(),
        unit: (*unit).into(),
      })
      .collect(),
    failed_units: failures.iter().map(|(_, unit)| (*unit).to_owned()).collect(),
    unstarted_units: vec![],
  }
}

fn passed() -> SuiteRun {
  run(Outcome::Passed, Some("suite: ok — 7 of 7 units passed"), &[])
}

// A close whose every hand answers as told, counting the suites it ran and the records it wrote.
struct Told {
  choice: SuiteChoice,
  handed: Option<String>,
  check: Option<CheckFault>,
  first: SuiteRun,
  again: Option<SuiteRun>,
  land: Option<Result<Vec<String>, String>>,
  asked: Vec<bool>,
  records: usize,
  lands: usize,
  said: RefCell<Vec<String>>,
}

fn told(first: SuiteRun) -> Told {
  Told {
    choice: SuiteChoice::Run,
    handed: None,
    check: None,
    first,
    again: None,
    land: None,
    asked: Vec::new(),
    records: 0,
    lands: 0,
    said: RefCell::new(Vec::new()),
  }
}

impl Told {
  fn last(&self) -> String {
    self.said.borrow().last().cloned().unwrap_or_default()
  }
}

impl CloseHands for Told {
  fn suite_choice(&self) -> SuiteChoice {
    self.choice.clone()
  }

  fn already_handed_over(&mut self) -> impl Future<Output = Option<String>> {
    std::future::ready(self.handed.clone())
  }

  fn static_check(&mut self) -> impl Future<Output = Option<CheckFault>> {
    std::future::ready(self.check.clone())
  }

  fn suite(&mut self, again: bool) -> impl Future<Output = SuiteRun> {
    self.asked.push(again);

    let answered = if again {
      self.again.clone().unwrap_or_else(|| self.first.clone())
    } else {
      self.first.clone()
    };

    std::future::ready(answered)
  }

  fn land(&mut self) -> impl Future<Output = Option<Result<Vec<String>, String>>> {
    self.lands += usize::from(self.land.is_some());
    std::future::ready(self.land.clone())
  }

  fn record(&mut self) -> impl Future<Output = String> {
    self.records += 1;
    std::future::ready("waiting to go live: abc".to_owned())
  }

  fn say(&self, line: &str) {
    self.said.borrow_mut().push(line.to_owned());
  }
}

fn gate(policy: &Policy, template: &str, more: &[(&str, &str)]) -> String {
  let mut all = vec![
    ("name", policy.close.close.name.as_str()),
    ("stopped", policy.close.gate.stopped.as_str()),
    ("log", LOG),
  ];

  all.extend_from_slice(more);
  policy.fill(template, &all)
}

#[tokio::test]
async fn a_check_that_failed_runs_no_suite_and_records_nothing() {
  let policy = policy();
  let mut close = told(passed());

  close.check = Some(CheckFault::Failed("change check: FAILED".into()));

  let result = run_suite_then_record(
    &mut close,
    &Said {
      policy: &policy,
      log: LOG,
    },
  )
  .await;

  assert_eq!((result.outcome, result.code), (RecordOutcome::CheckFailed, 1));
  assert_eq!((close.asked.len(), close.records), (0, 0));
  assert_eq!(close.last(), gate(&policy, &policy.close.gate.check_failed, &[]));
}

#[tokio::test]
async fn a_check_or_a_suite_whose_turn_never_came_ends_as_not_run() {
  let policy = policy();
  let said = Said {
    policy: &policy,
    log: LOG,
  };
  let mut check = told(passed());

  check.check = Some(CheckFault::NotRun("change check: not run".into()));

  let mut suite = told(run(Outcome::NotRun, None, &[]));
  let checked = run_suite_then_record(&mut check, &said).await;
  let suited = run_suite_then_record(&mut suite, &said).await;

  assert_eq!([checked.code, suited.code], [NOT_RUN_EXIT_CODE, NOT_RUN_EXIT_CODE]);
  assert_eq!(check.asked.len(), 0);
  assert_eq!([check.records, suite.records], [0, 0]);
  assert_eq!(check.last(), gate(&policy, &policy.close.gate.check_not_run, &[]));
  assert_eq!(suite.last(), gate(&policy, &policy.close.gate.suite_not_run, &[]));
}

#[tokio::test]
async fn a_suite_that_only_left_units_unstarted_ends_as_not_finished() {
  let policy = policy();
  let mut left = run(Outcome::NotRun, Some("suite: 9 of 10 units passed"), &[]);

  left.unstarted_units = vec!["client".into()];

  let mut close = told(left);
  let result = run_suite_then_record(
    &mut close,
    &Said {
      policy: &policy,
      log: LOG,
    },
  )
  .await;

  assert_eq!((result.code, result.failed_units.len(), close.records), (NOT_RUN_EXIT_CODE, 0, 0));
  assert_eq!(close.last(), gate(&policy, &policy.close.gate.unfinished, &[("count", "1")]));
}

#[tokio::test]
async fn a_suite_that_passed_records_the_commit_and_deploys_nothing() {
  let policy = policy();
  let mut close = told(passed());
  let result = run_suite_then_record(
    &mut close,
    &Said {
      policy: &policy,
      log: LOG,
    },
  )
  .await;

  assert_eq!(result.outcome, RecordOutcome::Recorded);
  assert_eq!(close.records, 1);
  assert_eq!(
    close.last(),
    policy.fill(
      &policy.close.gate.done,
      &[("what", &policy.close.gate.passed), ("record", "waiting to go live: abc")]
    )
  );
}

#[tokio::test]
async fn a_suite_that_wrote_no_verdict_failed_whatever_its_outcome() {
  let policy = policy();
  let mut close = told(run(Outcome::Passed, None, &[]));
  let result = run_suite_then_record(
    &mut close,
    &Said {
      policy: &policy,
      log: LOG,
    },
  )
  .await;

  assert_eq!((result.outcome, result.code, close.records), (RecordOutcome::SuiteFailed, 1, 0));
}

#[tokio::test]
async fn a_skipped_suite_records_the_commit_with_the_reason_said() {
  let policy = policy();
  let mut close = told(passed());

  close.choice = SuiteChoice::Skip("the documents alone moved".to_owned());

  assert_eq!(
    run_suite_then_record(
      &mut close,
      &Said {
        policy: &policy,
        log: LOG
      }
    )
    .await
    .outcome,
    RecordOutcome::Recorded
  );
  assert!(close.asked.is_empty());
  assert_eq!(close.said.borrow()[0], gate(&policy, &policy.close.gate.check, &[]));
  assert_eq!(
    close.said.borrow()[1],
    gate(&policy, &policy.close.gate.skipped, &[("why", "the documents alone moved")])
  );
}

#[tokio::test]
async fn a_branch_that_did_not_land_records_nothing() {
  let policy = policy();
  let mut close = told(passed());

  close.land = Some(Err("main could not be fast-forwarded".to_owned()));

  let result = run_suite_then_record(
    &mut close,
    &Said {
      policy: &policy,
      log: LOG,
    },
  )
  .await;

  assert_eq!((result.code, result.outcome), (1, RecordOutcome::LandFailed));
  assert_eq!((close.lands, close.records), (1, 0));
  assert_eq!(
    close.last(),
    policy.fill(
      &policy.close.gate.not_landed,
      &[("what", &policy.close.gate.passed), ("refused", "main could not be fast-forwarded")]
    )
  );
}

#[tokio::test]
async fn a_commit_already_live_or_waiting_runs_nothing() {
  let policy = policy();
  let mut close = told(passed());

  close.handed = Some(policy.close.gate.waiting.clone());

  assert_eq!(
    run_suite_then_record(
      &mut close,
      &Said {
        policy: &policy,
        log: LOG
      }
    )
    .await
    .outcome,
    RecordOutcome::NothingNew
  );
  assert_eq!((close.asked.len(), close.records), (0, 0));
}

#[tokio::test]
async fn a_test_that_fails_and_then_passes_when_run_once_more_lands_named_as_flaky() {
  let policy = policy();
  let mut close = told(run(
    Outcome::Failed,
    Some("suite: failed — 6 of 7 units passed"),
    &[("the reply comes back", FLAKE)],
  ));

  close.again = Some(passed());
  close.land = Some(Ok(Vec::new()));

  let result = run_suite_then_record(
    &mut close,
    &Said {
      policy: &policy,
      log: LOG,
    },
  )
  .await;
  let named = format!("the reply comes back ({FLAKE})");

  assert_eq!((result.code, result.outcome), (0, RecordOutcome::Recorded));
  assert_eq!(close.asked, [false, true]);
  assert_eq!((close.lands, close.records), (1, 1));
  assert!(
    close
      .said
      .borrow()
      .contains(&gate(&policy, &policy.close.gate.once_more, &[("files", FLAKE)]))
  );
  assert_eq!(
    close.last(),
    format!(
      "{}{}",
      policy.fill(
        &policy.close.gate.done,
        &[("what", &policy.close.gate.passed), ("record", "waiting to go live: abc")]
      ),
      policy.fill(&policy.close.gate.flaky_named, &[("tests", &named)])
    )
  );
}

#[tokio::test]
async fn a_test_that_fails_twice_records_nothing_naming_what_failed() {
  let policy = policy();
  let failing = run(
    Outcome::Failed,
    Some("suite: failed — 6 of 7 units passed"),
    &[("the reply comes back", FLAKE)],
  );
  let mut close = told(failing.clone());

  close.again = Some(failing);
  close.land = Some(Ok(Vec::new()));

  let result = run_suite_then_record(
    &mut close,
    &Said {
      policy: &policy,
      log: LOG,
    },
  )
  .await;

  assert_eq!((result.code, result.outcome), (1, RecordOutcome::SuiteFailed));
  assert_eq!((close.asked.len(), close.lands, close.records), (2, 0, 0));
  assert_eq!(result.failed_units, [FLAKE]);
  assert_eq!(result.failed, ["the reply comes back"]);
  assert_eq!(close.last(), gate(&policy, &policy.close.gate.suite_failed, &[]));
}

#[tokio::test]
async fn a_test_the_landing_s_check_again_found_flaky_is_named_beside_the_gate_s_own() {
  let policy = policy();
  let mut close = told(run(Outcome::Failed, Some("suite: failed"), &[("the reply comes back", FLAKE)]));
  let named = format!("the reply comes back ({FLAKE})");
  let again = format!("the queue drains ({FLAKE})");

  close.again = Some(passed());
  close.land = Some(Ok(vec![named.clone(), again.clone()]));

  let result = run_suite_then_record(
    &mut close,
    &Said {
      policy: &policy,
      log: LOG,
    },
  )
  .await;

  assert_eq!((result.code, result.outcome), (0, RecordOutcome::Recorded));
  assert!(
    close
      .last()
      .ends_with(&policy.fill(&policy.close.gate.flaky_named, &[("tests", &format!("{named}, {again}"))]))
  );
}

#[test]
fn the_flaky_tests_are_those_the_first_run_failed_and_the_last_did_not() {
  let first = run(Outcome::Failed, Some("failed"), &[("a", "one"), ("b", "two")]);
  let last = run(Outcome::Failed, Some("failed"), &[("b", "two")]);

  assert_eq!(flaky(&first, &last), ["a (one)"]);
  assert!(flaky(&last, &last).is_empty());
}
