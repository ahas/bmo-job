//! The testbed port's contract: every call runs bmo-testbed's binary with the words the API document
//! gives it, in the checkout it names, and reads its exit code and its output as that document says
//! (docs/api.md, "bmo-testbed").

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use job_testbed::{
  Check, Classified, Close, CloseTurn, Count, Deployed, Failure, Outcome, Pending, PendingDeploy,
  PendingOf, Relink, Relinked, Suite, SuiteLog, Testbed, TestbedClient, TestbedError, UnitPlan,
};
use serde_json::json;
use support::testbed::{Run, Stand};

fn words(said: &[&str]) -> Vec<String> {
  said.iter().map(|word| word.to_string()).collect()
}

/// A directory as the stand-in reads its own working directory.
fn real(dir: &Path) -> PathBuf {
  fs::canonicalize(dir).expect("a directory has a real path")
}

fn pending_json() -> serde_json::Value {
  json!({
    "repository": "bmo-job",
    "commit": "abc123",
    "item": 1744,
    "worker": "worker-1",
    "job": 7,
    "reason": "Add the testbed port",
    "urgent": false,
    "created_at": 1_700_000_000,
  })
}

fn pending() -> Pending {
  Pending {
    repository: "bmo-job".into(),
    commit: "abc123".into(),
    item: Some(1744),
    worker: Some("worker-1".into()),
    job: Some(7),
    reason: "Add the testbed port".into(),
    urgent: false,
    created_at: 1_700_000_000,
  }
}

#[tokio::test]
async fn a_check_runs_in_its_checkout_writing_both_streams_to_its_log() {
  let stand = Stand::new();
  let checkout = stand.folder("checkout");
  let log = stand.folder("logs").join("check.log");
  stand.answer("check", "checking\n", "all clean\n", 0);

  let check = Check {
    checkout: checkout.clone(),
    since: Some("abc123".into()),
    log: log.clone(),
  };
  assert_eq!(stand.client().check(&check).await.unwrap(), Outcome::Passed);

  assert_eq!(
    stand.run(),
    Run {
      words: words(&["check", "--since", "abc123"]),
      dir: real(&checkout),
    }
  );
  let written = fs::read_to_string(&log).unwrap();
  assert!(written.contains("checking") && written.contains("all clean"), "{written}");
}

#[tokio::test]
async fn a_count_with_no_commit_reads_exit_code_one_as_failed() {
  let stand = Stand::new();
  let checkout = stand.folder("checkout");
  stand.answer("count", "", "a test is gone\n", 1);

  let count = Count {
    checkout,
    since: None,
    log: stand.folder("logs").join("count.log"),
  };
  assert_eq!(stand.client().count(&count).await.unwrap(), Outcome::Failed);
  assert_eq!(stand.run().words, words(&["count"]));
}

#[tokio::test]
async fn a_suite_hands_on_every_word_and_reads_exit_code_75_as_not_run() {
  let stand = Stand::new();
  let checkout = stand.folder("checkout");
  stand.answer("suite", "", "", 75);

  let suite = Suite {
    checkout: checkout.clone(),
    since: Some("abc123".into()),
    again: true,
    guard: Some(900),
    file_guard: Some(120),
    turn_wait: Some(30),
    words: words(&["tests/testbed.rs", "--nocapture"]),
    log: stand.folder("logs").join("suite.log"),
  };
  assert_eq!(stand.client().suite(&suite).await.unwrap(), Outcome::NotRun);

  assert_eq!(
    stand.run(),
    Run {
      words: words(&[
        "suite",
        "--since",
        "abc123",
        "--again",
        "--guard",
        "900",
        "--file-guard",
        "120",
        "--turn-wait",
        "30",
        "tests/testbed.rs",
        "--nocapture",
      ]),
      dir: real(&checkout),
    }
  );
}

#[tokio::test]
async fn a_suite_with_nothing_more_runs_bare() {
  let stand = Stand::new();
  stand.answer("suite", "", "", 0);

  let suite = Suite {
    checkout: stand.folder("checkout"),
    log: stand.folder("logs").join("suite.log"),
    ..Suite::default()
  };
  assert_eq!(stand.client().suite(&suite).await.unwrap(), Outcome::Passed);
  assert_eq!(stand.run().words, words(&["suite"]));
}

#[tokio::test]
async fn a_refused_suite_hands_on_the_last_line_of_its_log() {
  let stand = Stand::new();
  stand.answer("suite", "reading the change\n", "no such unit: nowhere\n\n", 2);

  let suite = Suite {
    checkout: stand.folder("checkout"),
    words: words(&["nowhere"]),
    log: stand.folder("logs").join("suite.log"),
    ..Suite::default()
  };
  let error = stand.client().suite(&suite).await.unwrap_err();
  assert!(
    matches!(&error, TestbedError::Refused { call: "suite", said } if said == "no such unit: nowhere"),
    "{error:?}"
  );
}

#[tokio::test]
async fn a_check_ending_with_an_unnamed_code_is_a_failure_naming_it() {
  let stand = Stand::new();

  let check = Check {
    checkout: stand.folder("checkout"),
    since: None,
    log: stand.folder("logs").join("check.log"),
  };
  let error = stand.client().check(&check).await.unwrap_err();
  assert!(
    matches!(&error, TestbedError::Broke { call: "check", code: Some(3), said } if said == "no_fixture"),
    "{error:?}"
  );
}

#[tokio::test]
async fn a_log_that_cannot_be_written_is_a_call_that_did_not_start() {
  let stand = Stand::new();
  stand.answer("check", "", "", 0);

  let check = Check {
    checkout: stand.folder("checkout"),
    since: None,
    log: stand.folder("logs").join("missing").join("check.log"),
  };
  let error = stand.client().check(&check).await.unwrap_err();
  assert!(matches!(error, TestbedError::Unstarted { call: "check", .. }), "{error:?}");
  assert!(stand.runs().is_empty());
}

#[tokio::test]
async fn a_suite_log_is_read_on_exit_code_one() {
  let stand = Stand::new();
  let log = stand.folder("logs").join("suite.log");
  let reading = json!({
    "passed": 40,
    "failed": 1,
    "skipped": 2,
    "verdict": "failed",
    "failed_units": ["job"],
    "unstarted_units": [],
    "failures": [{ "test": "testbed::a_check", "unit": "job" }],
  });
  stand.answer("log", &reading.to_string(), "", 1);

  assert_eq!(
    stand.client().read_log(&log).await.unwrap(),
    SuiteLog {
      passed: 40,
      failed: 1,
      skipped: 2,
      verdict: "failed".into(),
      failed_units: words(&["job"]),
      unstarted_units: Vec::new(),
      failures: vec![Failure {
        test: "testbed::a_check".into(),
        unit: "job".into(),
      }],
    }
  );
  assert_eq!(stand.run().words, words(&["log", log.to_str().unwrap(), "--json"]));
}

#[tokio::test]
async fn a_close_runs_its_command_after_the_separator_and_hands_on_its_exit_code() {
  let stand = Stand::new();
  let checkout = stand.folder("checkout");
  stand.answer("close", "", "", 9);

  let close = Close {
    checkout: checkout.clone(),
    command: words(&["bmo-job", "close", "--item", "7"]),
  };
  assert_eq!(stand.client().close(&close).await.unwrap(), CloseTurn::Ran(9));
  assert_eq!(
    stand.run(),
    Run {
      words: words(&["close", "--", "bmo-job", "close", "--item", "7"]),
      dir: real(&checkout),
    }
  );
}

#[tokio::test]
async fn a_close_reads_75_as_not_run_and_2_as_the_command_s_own() {
  let stand = Stand::new();
  let close = Close {
    checkout: stand.folder("checkout"),
    command: words(&["true"]),
  };

  stand.answer("close", "", "", 75);
  assert_eq!(stand.client().close(&close).await.unwrap(), CloseTurn::NotRun);

  stand.answer("close", "", "", 2);
  assert_eq!(stand.client().close(&close).await.unwrap(), CloseTurn::Ran(2));
}

#[tokio::test]
async fn a_heavy_line_is_classified_on_exit_code_one() {
  let stand = Stand::new();
  let classified = json!({
    "heavy": true,
    "language": "rust",
    "command": ["cargo", "test"],
    "instead": "bmo testbed -- cargo test",
  });
  stand.answer("classify", &classified.to_string(), "", 1);

  assert_eq!(
    stand.client().classify("cargo test").await.unwrap(),
    Classified {
      heavy: true,
      language: Some("rust".into()),
      command: Some(words(&["cargo", "test"])),
      instead: Some("bmo testbed -- cargo test".into()),
    }
  );
  assert_eq!(stand.run().words, words(&["classify", "--json", "cargo test"]));
}

#[tokio::test]
async fn a_light_line_is_classified_on_exit_code_zero() {
  let stand = Stand::new();
  stand.answer("classify", r#"{"heavy": false}"#, "", 0);

  assert_eq!(
    stand.client().classify("git status").await.unwrap(),
    Classified {
      heavy: false,
      language: None,
      command: None,
      instead: None,
    }
  );
}

#[tokio::test]
async fn a_held_move_is_planned_in_its_checkout() {
  let stand = Stand::new();
  let checkout = stand.folder("checkout");
  let plan = json!({
    "relinks": [{ "name": "bmo-job.service", "file": "deploy/bmo-job.service" }],
    "running": ["bmo-job.service"],
    "unmatched": [],
  });
  stand.answer("units-plan", &plan.to_string(), "", 1);

  let planned = stand.client().plan_units(&checkout, "abc123", true).await.unwrap();
  assert_eq!(
    planned,
    UnitPlan {
      relinks: vec![Relink {
        name: "bmo-job.service".into(),
        file: "deploy/bmo-job.service".into(),
      }],
      running: words(&["bmo-job.service"]),
      unmatched: Vec::new(),
    }
  );
  assert!(planned.held());
  assert_eq!(
    stand.run(),
    Run {
      words: words(&["units", "plan", "--to", "abc123", "--hold", "--json"]),
      dir: real(&checkout),
    }
  );
}

#[tokio::test]
async fn a_move_with_nowhere_unmatched_and_nothing_running_may_go() {
  let stand = Stand::new();
  stand.answer("units-plan", r#"{"relinks": [], "running": [], "unmatched": []}"#, "", 0);

  let planned = stand.client().plan_units(&stand.folder("checkout"), "abc123", false).await.unwrap();
  assert!(!planned.held());
  assert_eq!(stand.run().words, words(&["units", "plan", "--to", "abc123", "--json"]));
}

#[tokio::test]
async fn a_relink_answers_what_it_moved_and_left() {
  let stand = Stand::new();
  let checkout = stand.folder("checkout");
  let relinked = json!({ "relinked": ["bmo-job.service"], "dangling": [], "unmatched": ["old.timer"] });
  stand.answer("units-relink", &relinked.to_string(), "", 1);

  assert_eq!(
    stand.client().relink_units(&checkout).await.unwrap(),
    Relinked {
      relinked: words(&["bmo-job.service"]),
      dangling: Vec::new(),
      unmatched: words(&["old.timer"]),
    }
  );
  assert_eq!(
    stand.run(),
    Run {
      words: words(&["units", "relink", "--json"]),
      dir: real(&checkout),
    }
  );
}

#[tokio::test]
async fn a_pending_deploy_is_recorded_in_its_checkout_with_every_word() {
  let stand = Stand::new();
  let checkout = stand.folder("checkout");
  stand.answer("pending-add", &pending_json().to_string(), "", 0);

  let deploy = PendingDeploy {
    checkout: checkout.clone(),
    commit: "abc123".into(),
    item: Some(1744),
    worker: Some("worker-1".into()),
    job: Some(7),
    reason: "Add the testbed port".into(),
  };
  assert_eq!(stand.client().record_pending(&deploy).await.unwrap(), pending());
  assert_eq!(
    stand.run(),
    Run {
      words: words(&[
        "pending",
        "add",
        "--commit",
        "abc123",
        "--item",
        "1744",
        "--worker",
        "worker-1",
        "--job",
        "7",
        "--reason",
        "Add the testbed port",
        "--json",
      ]),
      dir: real(&checkout),
    }
  );
}

#[tokio::test]
async fn a_pending_deploy_with_only_a_reason_names_nothing_else() {
  let stand = Stand::new();
  stand.answer("pending-add", &pending_json().to_string(), "", 0);

  let deploy = PendingDeploy {
    checkout: stand.folder("checkout"),
    commit: "abc123".into(),
    item: None,
    worker: None,
    job: None,
    reason: "Add the testbed port".into(),
  };
  stand.client().record_pending(&deploy).await.unwrap();
  assert_eq!(
    stand.run().words,
    words(&["pending", "add", "--commit", "abc123", "--reason", "Add the testbed port", "--json"])
  );
}

#[tokio::test]
async fn pending_deploys_are_listed_all_or_of_one_job_or_item() {
  let stand = Stand::new();
  stand.answer("pending-list", &json!([pending_json()]).to_string(), "", 0);
  let client = stand.client();

  assert_eq!(client.pending(PendingOf::All).await.unwrap(), vec![pending()]);
  client.pending(PendingOf::Job(7)).await.unwrap();
  client.pending(PendingOf::Item(1744)).await.unwrap();

  let said: Vec<Vec<String>> = stand.runs().into_iter().map(|run| run.words).collect();
  assert_eq!(
    said,
    vec![
      words(&["pending", "list", "--json"]),
      words(&["pending", "list", "--job", "7", "--json"]),
      words(&["pending", "list", "--item", "1744", "--json"]),
    ]
  );
}

#[tokio::test]
async fn the_live_deploy_is_read_in_its_checkout() {
  let stand = Stand::new();
  let checkout = stand.folder("checkout");
  stand.answer("deployed", r#"{"commit": "abc123", "release": "/srv/releases/abc123"}"#, "", 0);

  assert_eq!(
    stand.client().deployed(&checkout).await.unwrap(),
    Some(Deployed {
      commit: "abc123".into(),
      release: "/srv/releases/abc123".into(),
    })
  );
  assert_eq!(
    stand.run(),
    Run {
      words: words(&["deployed", "--json"]),
      dir: real(&checkout),
    }
  );
}

#[tokio::test]
async fn nothing_live_reads_as_none() {
  let stand = Stand::new();
  stand.answer("deployed", "null\n", "", 0);

  assert_eq!(stand.client().deployed(&stand.folder("checkout")).await.unwrap(), None);
}

#[tokio::test]
async fn a_refused_call_hands_on_the_last_line_it_wrote() {
  let stand = Stand::new();
  stand.answer("pending-add", "", "reading the checkout\nnot a checkout of a declared repository\n", 2);

  let deploy = PendingDeploy {
    checkout: stand.folder("checkout"),
    commit: "abc123".into(),
    item: None,
    worker: None,
    job: None,
    reason: "Add the testbed port".into(),
  };
  let error = stand.client().record_pending(&deploy).await.unwrap_err();
  assert!(
    matches!(
      &error,
      TestbedError::Refused { call: "record_pending", said }
        if said == "not a checkout of a declared repository"
    ),
    "{error:?}"
  );
}

#[tokio::test]
async fn an_unnamed_exit_code_is_a_failure_naming_it() {
  let stand = Stand::new();

  let error = stand.client().classify("cargo test").await.unwrap_err();
  assert!(
    matches!(&error, TestbedError::Broke { call: "classify", code: Some(3), said } if said == "no_fixture"),
    "{error:?}"
  );
}

#[tokio::test]
async fn an_exit_code_one_where_the_call_names_only_zero_is_a_failure() {
  let stand = Stand::new();
  stand.answer("deployed", "null\n", "", 1);

  let error = stand.client().deployed(&stand.folder("checkout")).await.unwrap_err();
  assert!(
    matches!(
      error,
      TestbedError::Broke {
        call: "deployed",
        code: Some(1),
        ..
      }
    ),
    "{error:?}"
  );
}

#[tokio::test]
async fn an_answer_that_does_not_read_is_malformed() {
  let stand = Stand::new();
  stand.answer("units-relink", r#"{"relinked": "all"}"#, "", 0);

  let error = stand.client().relink_units(&stand.folder("checkout")).await.unwrap_err();
  assert!(
    matches!(
      error,
      TestbedError::Malformed {
        call: "relink_units",
        ..
      }
    ),
    "{error:?}"
  );
}

#[tokio::test]
async fn a_program_that_does_not_start_is_a_call_that_did_not_start() {
  let stand = Stand::new();
  let client = TestbedClient::new(stand.folder("bin").join("bmo-testbed")).unwrap();

  let error = client.classify("cargo test").await.unwrap_err();
  assert!(
    matches!(
      error,
      TestbedError::Unstarted {
        call: "classify",
        ..
      }
    ),
    "{error:?}"
  );
}

#[test]
fn a_relative_program_is_refused() {
  let error = TestbedClient::new("bin/bmo-testbed").unwrap_err();
  assert!(
    matches!(&error, TestbedError::NotAProgram { program } if program == "bin/bmo-testbed"),
    "{error:?}"
  );
}
