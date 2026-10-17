//! Impact grounds (docs/rows.md, "The verbs of filing"): written on the item with the verdict they come to,
//! and a job's own grounds kept beside them, told to the agent only where they come to another verdict or
//! the agent filed none.

mod support;

use job_codes::todo_verbs::{ImpactDifference, TodoAnswer, Verdict};
use job_receiver::Said;
use queue_rows::impact::{impact, impact_check};
use support::Scene;

const ONE_NODE: &str = r#"{"nodes":[{"id":"a","contract":"the API"}]}"#;
const TWO_CONTRACTS: &str =
  r#"{"nodes":[{"id":"a","contract":"the API"},{"id":"b","contract":"the schema"}]}"#;

#[tokio::test]
async fn the_grounds_are_written_with_their_verdict_and_never_move_the_setting() {
  let scene = Scene::new().await;

  scene.item(7, "Weigh it").await;

  assert_eq!(
    impact(&scene.ground(), "7", TWO_CONTRACTS).await.unwrap(),
    TodoAnswer::ImpactSet {
      id: 7,
      title: "Weigh it".into(),
      contracts: 2,
      new_home: false,
      verdict: Verdict::Cowork,
      cowork: false,
    }
  );
  assert_ne!(scene.column::<Option<i64>>(7, "cowork").await, Some(1));
  assert_eq!(impact(&scene.ground(), "7", "{").await.unwrap_err().code, "impact_unread");
  assert_eq!(impact(&scene.ground(), "8", ONE_NODE).await.unwrap_err().code, "no_such_item");
}

#[tokio::test]
async fn a_jobs_grounds_are_kept_and_told_only_where_they_differ_or_none_were_filed() {
  let scene = Scene::new().await;

  scene.item(7, "Weigh it").await;

  let job = scene.job("run-1");

  assert_eq!(
    impact_check(&job, "7", ONE_NODE).await.unwrap(),
    TodoAnswer::ImpactCheckTold {
      id: 7,
      difference: ImpactDifference::Unfiled,
    }
  );

  impact(&scene.ground(), "7", ONE_NODE).await.unwrap();

  assert_eq!(impact_check(&job, "7", ONE_NODE).await.unwrap(), TodoAnswer::ImpactCheckKept { id: 7 });
  assert_eq!(
    impact_check(&job, "7", TWO_CONTRACTS).await.unwrap(),
    TodoAnswer::ImpactCheckTold {
      id: 7,
      difference: ImpactDifference::Differs {
        verdict: Verdict::Cowork,
        cowork: false
      },
    }
  );

  let orders = scene.units.instructions();

  assert_eq!(orders.len(), 2);
  assert_eq!(orders[1].sort, "impact_mismatch");

  let Said::Order(told) = &orders[1].said else {
    panic!("the agent is told with an order")
  };

  assert!(
    told.contains("The grounds you filed it with:\n- a: the API\nThey come to a single job."),
    "{told}"
  );
  assert!(
    told.contains("The grounds the job found:\n- a: the API\n- b: the schema\nThey come to a co-work."),
    "{told}"
  );
  assert!(told.contains("bmo todo --cowork 7 on|off"), "{told}");
  assert!(scene.column::<Option<String>>(7, "job_impact").await.is_some());
}
