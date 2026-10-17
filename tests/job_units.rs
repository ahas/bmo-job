//! The units linked into a checkout following its files (docs/runs.md, "Landing on main"), asked of the
//! stand-in for bmo-testbed: a move's plan read into the links it relinks or the units that hold it, and the
//! relink asked once main has moved.

mod support;

use bmo_job::job_run::checkout::Checkout;
use bmo_job::job_run::checkout::units::{CheckoutUnits, TestbedLinks, UnitsHeld};
use job_testbed::Relink;
use serde_json::json;
use support::queue::policy;
use support::testbed::Stand;

fn words(list: &[&str]) -> Vec<String> {
  list.iter().map(|word| (*word).to_owned()).collect()
}

fn checkout(stand: &Stand) -> Checkout {
  Checkout::local(stand.folder("checkout").to_string_lossy())
}

#[tokio::test]
async fn a_link_the_commit_would_leave_dangling_is_planned_and_relinked_once_main_moves() {
  let stand = Stand::new();
  let policy = policy();
  let client = stand.client();
  let links = TestbedLinks(&client);
  let units = CheckoutUnits {
    links: &links,
    policy: &policy,
  };
  let plan = json!({ "relinks": [{ "name": "runner.service", "file": "new/runner.service" }], "running": [], "unmatched": [] });

  stand.answer("units-plan", &plan.to_string(), "", 0);
  stand.answer(
    "units-relink",
    r#"{"relinked": ["runner.service"], "dangling": [], "unmatched": []}"#,
    "",
    0,
  );

  let relinks = units.plan(&checkout(&stand), "abc123", true).await.unwrap();

  assert_eq!(
    relinks,
    vec![Relink {
      name: "runner.service".into(),
      file: "new/runner.service".into(),
    }]
  );

  units.follow(&checkout(&stand), &relinks).await;

  let runs = stand.runs();

  assert_eq!(runs.len(), 2);
  assert_eq!(runs[0].words, words(&["units", "plan", "--to", "abc123", "--hold", "--json"]));
  assert_eq!(runs[1].words, words(&["units", "relink", "--json"]));
}

#[tokio::test]
async fn a_unit_running_in_a_folder_the_commit_takes_away_holds_the_move() {
  let stand = Stand::new();
  let policy = policy();
  let client = stand.client();
  let links = TestbedLinks(&client);
  let units = CheckoutUnits {
    links: &links,
    policy: &policy,
  };
  let plan = json!({ "relinks": [{ "name": "runner.service", "file": "new/runner.service" }], "running": ["runner.service"], "unmatched": [] });

  stand.answer("units-plan", &plan.to_string(), "", 1);

  assert_eq!(
    units.plan(&checkout(&stand), "abc123", true).await,
    Err(UnitsHeld::Running(words(&["runner.service"])))
  );
  // Where the files have already moved, nothing running holds them.
  assert_eq!(units.plan(&checkout(&stand), "abc123", false).await.map(|relinks| relinks.len()), Ok(1));
}

#[tokio::test]
async fn a_dangling_link_with_no_one_file_of_its_name_is_refused_naming_it() {
  let stand = Stand::new();
  let policy = policy();
  let client = stand.client();
  let links = TestbedLinks(&client);
  let units = CheckoutUnits {
    links: &links,
    policy: &policy,
  };
  let fault = "runner.service: new/runner.service and other/runner.service";

  stand.answer(
    "units-plan",
    &json!({ "relinks": [], "running": [], "unmatched": [fault] }).to_string(),
    "",
    1,
  );

  assert_eq!(
    units.plan(&checkout(&stand), "abc123", true).await,
    Err(UnitsHeld::Unmatched(words(&[fault])))
  );
}

#[tokio::test]
async fn a_plan_bmo_testbed_could_not_give_holds_the_move_and_says_why() {
  let stand = Stand::new();
  let policy = policy();
  let client = stand.client();
  let links = TestbedLinks(&client);
  let units = CheckoutUnits {
    links: &links,
    policy: &policy,
  };

  stand.answer("units-plan", "", "not a checkout", 2);

  let Err(UnitsHeld::Unmatched(faults)) = units.plan(&checkout(&stand), "abc123", true).await else {
    panic!("the move went on without a plan");
  };

  assert!(faults[0].contains("not a checkout"), "{faults:?}");
}

#[tokio::test]
async fn nothing_planned_asks_for_no_relink_and_a_checkout_put_back_relinks_what_its_commit_left() {
  let stand = Stand::new();
  let policy = policy();
  let client = stand.client();
  let links = TestbedLinks(&client);
  let units = CheckoutUnits {
    links: &links,
    policy: &policy,
  };

  units.follow(&checkout(&stand), &[]).await;
  assert!(stand.runs().is_empty());

  stand.answer(
    "units-plan",
    r#"{"relinks": [{ "name": "kept.service", "file": "kept.service" }], "running": [], "unmatched": []}"#,
    "",
    0,
  );
  stand.answer(
    "units-relink",
    r#"{"relinked": ["kept.service"], "dangling": [], "unmatched": []}"#,
    "",
    0,
  );
  units.follow_moved(&checkout(&stand)).await;

  let runs = stand.runs();

  assert_eq!(runs[0].words, words(&["units", "plan", "--to", "HEAD", "--json"]));
  assert_eq!(runs[1].words, words(&["units", "relink", "--json"]));
}
