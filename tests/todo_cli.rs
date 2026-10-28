//! `bmo-job todo --done` from a shell (docs/cli.md): the closing goes through the service's own report and
//! item routes, keeping the run's account and what it could not reach on the item, and closing it.

mod support;

use std::collections::BTreeMap;

use bmo_job::todo_cli::{TodoArgs, close};
use bmo_job_client::{ItemState, LeaseAsk, LeaseWait, NewItem, PageQuery, ReportKind};

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
  assert_eq!(String::from_utf8_lossy(&output.stdout), "job\ntodo\n");
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
