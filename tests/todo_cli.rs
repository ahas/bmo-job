//! `bmo-job todo --done` from a shell (docs/cli.md): the closing goes through the service's own report and
//! item routes, keeping the run's account and what it could not reach on the item, and closing it.

use crate::support;

use std::collections::BTreeMap;

use bmo_job::todo_cli::{AddArgs, StopHereArgs, TodoArgs, add, close, stop_here};
use bmo_job::work_order_cli::add_step;
use bmo_job_client::{ItemState, LeaseAsk, LeaseWait, NewItem, PageQuery, ReportKind, WorkOrderBody};

use support::Service;

#[tokio::test]
async fn a_closing_reports_and_closes_through_the_service_routes() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = client
    .create_item(&NewItem {
      title: "a closing item".to_owned(),
      brief: "close it".to_owned(),
      queue_name: "todo".to_owned(),
      labels: BTreeMap::from([("lane".to_owned(), "todo".to_owned())]),
      ..NewItem::default()
    })
    .await
    .unwrap();

  assert_eq!(item.state, ItemState::Open);

  let lease = client
    .take_lease(
      &LeaseAsk {
        queue_name: "todo".to_owned(),
        holder_name: "worker".to_owned(),
        ttl_ms: 60_000,
        assignee_name: None,
        item_id: Some(item.id),
      },
      &LeaseWait { wait_ms: None },
    )
    .await
    .unwrap()
    .expect("the item is leased");

  assert_eq!(lease.item_id, item.id);

  let lines = close(
    &client,
    &TodoArgs {
      done: item.id,
      report: Some("The work is done.".to_owned()),
      unfinished: Some("No hardware to reach.".to_owned()),
      by: "helper".to_owned(),
    },
  )
  .await
  .unwrap();

  assert_eq!(lines.len(), 3, "two reports and the item");

  let kept = client.item(item.id).await.unwrap();

  assert_eq!(kept.state, ItemState::Done);

  let reports = client.reports(item.id, &PageQuery::default()).await.unwrap();

  assert_eq!(reports.items.len(), 2);
  assert_eq!(reports.items[0].kind, ReportKind::Held);
  assert_eq!(reports.items[0].body, "No hardware to reach.");
  assert_eq!(reports.items[0].actor_name, "helper");
  assert_eq!(reports.items[1].kind, ReportKind::Final);
  assert_eq!(reports.items[1].body, "The work is done.");
  assert_eq!(reports.items[1].actor_name, "helper");
}

#[test]
fn the_unit_declares_the_todo_and_job_nouns_bmo_finds_them_by() {
  let output = std::process::Command::new(env!("CARGO_BIN_EXE_bmo-job")).arg("nouns").output().unwrap();

  assert!(output.status.success());
  assert_eq!(String::from_utf8_lossy(&output.stdout), "job\njob ask\thome\ntodo\nwork-order\n");
}

#[tokio::test]
async fn the_command_closes_the_item_with_the_address_and_token_the_run_names() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = client
    .create_item(&NewItem {
      title: "a command's item".to_owned(),
      brief: "close it".to_owned(),
      queue_name: "todo".to_owned(),
      labels: BTreeMap::from([("lane".to_owned(), "todo".to_owned())]),
      ..NewItem::default()
    })
    .await
    .unwrap();

  client
    .take_lease(
      &LeaseAsk {
        queue_name: "todo".to_owned(),
        holder_name: "worker".to_owned(),
        ttl_ms: 60_000,
        assignee_name: None,
        item_id: Some(item.id),
      },
      &LeaseWait { wait_ms: None },
    )
    .await
    .unwrap()
    .expect("the item is leased");

  let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_bmo-job"))
    .args([
      "todo",
      "--done",
      &format!("#{}", item.id),
      "--report",
      "The work is done.",
      "--unfinished",
      "No hardware to reach.",
      "--by",
      "helper",
    ])
    .env("BMO_JOB_URL", &service.url)
    .env("BMO_JOB_TOKEN", support::TOKEN)
    .output()
    .await
    .unwrap();

  assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
  assert_eq!(client.item(item.id).await.unwrap().state, ItemState::Done);

  let reports = client.reports(item.id, &PageQuery::default()).await.unwrap();

  assert_eq!(reports.items.len(), 2);
  assert_eq!(reports.items[0].kind, ReportKind::Held);
  assert_eq!(reports.items[1].kind, ReportKind::Final);
}

async fn leased(service: &Service, title: &str) -> i64 {
  let client = service.client();
  let item = client
    .create_item(&NewItem {
      title: title.to_owned(),
      brief: "the work".to_owned(),
      queue_name: "todo".to_owned(),
      scope: Some("56".to_owned()),
      labels: BTreeMap::from([("lane".to_owned(), "todo".to_owned())]),
      ..NewItem::default()
    })
    .await
    .unwrap();

  client
    .take_lease(
      &LeaseAsk {
        queue_name: "todo".to_owned(),
        holder_name: "worker".to_owned(),
        ttl_ms: 60_000,
        assignee_name: None,
        item_id: Some(item.id),
      },
      &LeaseWait { wait_ms: None },
    )
    .await
    .unwrap()
    .expect("the item is leased");

  item.id
}

#[tokio::test]
async fn a_stop_here_holds_the_item_with_its_reason_and_leaves_the_account_on_it() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = leased(&service, "stopped short").await;
  let args = StopHereArgs {
    item,
    report: Some("The parser is done; the next run starts at the lexer.".to_owned()),
    why: None,
    after_deploy: Some("4242".to_owned()),
    by: "helper".to_owned(),
    ..StopHereArgs::default()
  };

  stop_here(&client, &args).await.expect_err("an item not yet held needs its reason");

  stop_here(
    &client,
    &StopHereArgs {
      why: Some("waits on the deployment".to_owned()),
      ..args.clone()
    },
  )
  .await
  .expect("held");

  let held = client.item(item).await.unwrap();

  assert_eq!(held.state, ItemState::Held);
  assert_eq!(held.labels["hold-why"], "waits on the deployment");
  assert_eq!(held.labels["hold-after-deploy"], "4242");

  // An item already held takes the account alone.
  stop_here(
    &client,
    &StopHereArgs {
      item,
      report: Some("And the lexer's first half.".to_owned()),
      by: "helper".to_owned(),
      ..StopHereArgs::default()
    },
  )
  .await
  .expect("kept");

  let reports = client.reports(item, &PageQuery::default()).await.unwrap();

  assert_eq!(reports.items.len(), 2);
  assert!(reports.items.iter().all(|report| report.kind == ReportKind::Held));

  // A held item is not finished.
  close(
    &client,
    &TodoArgs {
      done: item,
      report: Some("done".to_owned()),
      unfinished: None,
      by: "helper".to_owned(),
    },
  )
  .await
  .expect_err("a held item is never closed");
}

#[tokio::test]
async fn a_closing_carrying_a_page_hosted_elsewhere_is_refused() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = leased(&service, "a report with a gist").await;

  close(
    &client,
    &TodoArgs {
      done: item,
      report: Some("See https://gist.github.com/someone/abc.".to_owned()),
      unfinished: None,
      by: "helper".to_owned(),
    },
  )
  .await
  .expect_err("refused");

  assert_eq!(client.item(item).await.unwrap().state, ItemState::Leased);
}

#[tokio::test]
async fn a_row_raised_by_a_job_stands_a_draft_in_its_items_queue_and_scope() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = leased(&service, "the source").await;
  let raised = add(
    &client,
    &AddArgs {
      title: "what is left".to_owned(),
      judgement: "It was left because the source closed first.".to_owned(),
      raised_by: Some(item),
      from_todo: Some("12:2".to_owned()),
      by: "helper".to_owned(),
    },
  )
  .await
  .expect("raised");
  let raised: serde_json::Value = serde_json::from_str(&raised).unwrap();
  let row = client.item(raised["id"].as_i64().unwrap()).await.unwrap();

  assert_eq!(row.state, ItemState::Draft);
  assert_eq!(row.queue_name, "todo");
  assert_eq!(row.scope.as_deref(), Some("56"));
  assert_eq!(row.raised_by, Some(item));
  assert_eq!(row.labels["from-todo"], "12:2");
  assert_eq!(row.requester_name.as_deref(), Some("helper"));

  add(&client, &AddArgs::default())
    .await
    .expect_err("a row names its title, judgement and source");
}

#[tokio::test]
async fn a_step_is_added_to_the_end_of_a_work_order() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = leased(&service, "ordered work").await;

  add_step(&client, item, "lexer", None).await.expect_err("no work order stands yet");

  client
    .put_work_order(
      item,
      &WorkOrderBody {
        purpose: "the parser".to_owned(),
        directive: "build it".to_owned(),
        steps: vec!["grammar".to_owned()],
        state: None,
      },
    )
    .await
    .unwrap();

  add_step(&client, item, "lexer", Some("hand-written")).await.expect("added");

  assert_eq!(client.work_order(item).await.unwrap().steps, ["grammar", "lexer: hand-written"]);
}
