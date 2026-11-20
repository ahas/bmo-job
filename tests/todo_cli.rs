//! `bmo-job todo` from a shell (docs/cli.md): a run leaves its account and what it could not reach on its item
//! through the service's own report routes, and moves none of its item's state.

use crate::support;

use std::collections::BTreeMap;

use bmo_job::job_close::{Standing, waiting_commit, waiting_commits, write_standing};
use bmo_job::todo_cli::{AddArgs, StopHereArgs, TodoArgs, add, close, stop_here};
use bmo_job_client::{
  ItemChange, ItemState, LeaseAsk, LeaseWait, NewItem, NewReview, PageQuery, ReportKind, ReviewKind,
  WorkOrderBody,
};

use support::Service;

#[tokio::test]
async fn a_closing_leaves_its_reports_and_moves_no_state() {
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
        frozen: None,
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

  assert_eq!(lines.len(), 3, "two reports and the line naming what it waits on");
  assert!(lines[2].contains("\"waitsOn\":\"orderer\""), "{}", lines[2]);

  let kept = client.item(item.id).await.unwrap();

  assert_eq!(kept.state, ItemState::Leased, "its orderer moves it");

  let reports = client.reports(item.id, &PageQuery::default()).await.unwrap();

  assert_eq!(reports.items.len(), 2);
  assert_eq!(reports.items[0].kind, ReportKind::Held);
  assert_eq!(reports.items[0].body, "No hardware to reach.");
  assert_eq!(reports.items[0].actor_name, "helper");
  assert_eq!(
    reports.items[0].labels,
    BTreeMap::from([("account".to_owned(), "unfinished".to_owned())]),
    "the note is labelled the unfinished one"
  );
  assert_eq!(reports.items[1].kind, ReportKind::Final);
  assert_eq!(reports.items[1].body, "The work is done.");
  assert_eq!(reports.items[1].actor_name, "helper");
  assert!(reports.items[1].labels.is_empty(), "the account carries no label");
}

#[tokio::test]
async fn each_repository_s_close_is_written_on_the_item_and_a_closing_names_every_commit_it_waits_on() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = client
    .create_item(&NewItem {
      title: "a waiting item".to_owned(),
      brief: "close it once it is live".to_owned(),
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
        frozen: None,
      },
      &LeaseWait { wait_ms: None },
    )
    .await
    .unwrap()
    .expect("the item is leased");

  write_standing(&client, item.id, "web", &Standing::Testing).await.unwrap();

  let testing = client.item(item.id).await.unwrap();

  assert_eq!(testing.labels.get("close-phase:web").map(String::as_str), Some("testing"));
  assert_eq!(
    testing.labels.get("lane").map(String::as_str),
    Some("todo"),
    "its other labels are kept"
  );
  assert_eq!(waiting_commit(&testing), None);

  write_standing(&client, item.id, "web", &Standing::WaitingBatch("0123abcd".to_owned()))
    .await
    .unwrap();
  write_standing(&client, item.id, "drive", &Standing::WaitingBatch("4567ef01".to_owned()))
    .await
    .unwrap();

  let waiting = client.item(item.id).await.unwrap();

  assert_eq!(
    waiting_commits(&waiting.labels),
    BTreeMap::from([
      ("drive".to_owned(), "4567ef01".to_owned()),
      ("web".to_owned(), "0123abcd".to_owned())
    ]),
    "the second repository's close keeps the first's commit"
  );
  assert_eq!(waiting_commit(&waiting), Some("4567ef01"));

  let lines = close(
    &client,
    &TodoArgs {
      done: item.id,
      report: Some("The work is done.".to_owned()),
      unfinished: None,
      by: "helper".to_owned(),
    },
  )
  .await
  .unwrap();
  let waits: serde_json::Value = serde_json::from_str(&lines[1]).unwrap();

  assert_eq!(lines.len(), 2, "the report and the line naming what it waits on");
  assert_eq!(
    waits,
    serde_json::json!({
      "itemId": item.id,
      "state": "leased",
      "waitsOn": "deployment",
      "commit": "4567ef01",
      "commits": { "drive": "4567ef01", "web": "0123abcd" },
    })
  );

  let kept = client.item(item.id).await.unwrap();

  assert_eq!(kept.state, ItemState::Leased, "it is done once its commits are live");
  assert_eq!(client.reports(item.id, &PageQuery::default()).await.unwrap().items.len(), 1);

  write_standing(&client, item.id, "drive", &Standing::Off).await.unwrap();
  write_standing(&client, item.id, "web", &Standing::Off).await.unwrap();

  let off = client.item(item.id).await.unwrap();

  assert_eq!(off.labels, BTreeMap::from([("lane".to_owned(), "todo".to_owned())]));

  close(
    &client,
    &TodoArgs {
      done: item.id,
      report: None,
      unfinished: None,
      by: "helper".to_owned(),
    },
  )
  .await
  .unwrap();

  assert_eq!(client.item(item.id).await.unwrap().state, ItemState::Leased);
}

#[test]
fn the_unit_declares_the_todo_and_job_nouns_bmo_finds_them_by() {
  let output = std::process::Command::new(env!("CARGO_BIN_EXE_bmo-job")).arg("nouns").output().unwrap();

  assert!(output.status.success());
  assert_eq!(String::from_utf8_lossy(&output.stdout), "job\njob ask\thome\ntodo\n");
}

#[tokio::test]
async fn the_command_leaves_the_account_with_the_address_and_token_the_run_names() {
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
        frozen: None,
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
  assert_eq!(client.item(item.id).await.unwrap().state, ItemState::Leased);

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
        frozen: None,
      },
      &LeaseWait { wait_ms: None },
    )
    .await
    .unwrap()
    .expect("the item is leased");

  item.id
}

#[tokio::test]
async fn a_stop_here_leaves_the_account_on_the_item_and_moves_no_state() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = leased(&service, "stopped short").await;
  let args = StopHereArgs {
    item,
    report: Some("The parser is done; the next run starts at the lexer.".to_owned()),
    by: "helper".to_owned(),
  };

  stop_here(
    &client,
    &StopHereArgs {
      report: None,
      ..args.clone()
    },
  )
  .await
  .expect_err("an account names what is finished");
  stop_here(&client, &args).await.expect("kept");
  stop_here(
    &client,
    &StopHereArgs {
      report: Some("And the lexer's first half.".to_owned()),
      ..args.clone()
    },
  )
  .await
  .expect("kept");

  let stands = client.item(item).await.unwrap();

  assert_eq!(stands.state, ItemState::Leased, "holding it is its orderer's");
  assert!(!stands.labels.contains_key("hold-why"));

  let reports = client.reports(item, &PageQuery::default()).await.unwrap();

  assert_eq!(reports.items.len(), 2);
  assert!(reports.items.iter().all(|report| report.kind == ReportKind::Held));
  assert!(
    reports.items.iter().all(|report| report.labels.is_empty()),
    "a stop-here account is no unfinished note"
  );
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

  client
    .change_item(
      item,
      &ItemChange {
        assignee_name: Some(Some("the-repository".to_owned())),
        ..ItemChange::default()
      },
    )
    .await
    .unwrap();

  let raised = add(
    &client,
    &AddArgs {
      title: "what is left".to_owned(),
      judgement: "It was left because the source closed first.".to_owned(),
      raised_by: Some(item),
      repositories: Vec::new(),
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
  // A row names the repository its work is in: the one given, else the raising item's own.
  assert_eq!(row.assignee_name.as_deref(), Some("the-repository"));
  assert!(!row.labels.contains_key("repositories"), "a row of one repository carries no list");

  let elsewhere = add(
    &client,
    &AddArgs {
      title: "work in another repository".to_owned(),
      judgement: "The source's run found it in a repository of its own.".to_owned(),
      raised_by: Some(item),
      repositories: vec!["another-repository".to_owned()],
      by: "helper".to_owned(),
      ..AddArgs::default()
    },
  )
  .await
  .expect("raised");
  let elsewhere: serde_json::Value = serde_json::from_str(&elsewhere).unwrap();
  let elsewhere = client.item(elsewhere["id"].as_i64().unwrap()).await.unwrap();

  assert_eq!(elsewhere.assignee_name.as_deref(), Some("another-repository"));
  assert!(
    !elsewhere.labels.contains_key("repositories"),
    "a row of one repository carries no list"
  );

  // A row whose work spans repositories names the first as its assignee and every one, once each, in its label.
  let spanning = add(
    &client,
    &AddArgs {
      title: "work across repositories".to_owned(),
      judgement: "The source's run found it spans a base library and its consumers.".to_owned(),
      raised_by: Some(item),
      repositories: vec![
        "the-base".to_owned(),
        " a-consumer ".to_owned(),
        "the-base".to_owned(),
        String::new(),
        "another-consumer".to_owned(),
      ],
      by: "helper".to_owned(),
      ..AddArgs::default()
    },
  )
  .await
  .expect("raised");
  let spanning: serde_json::Value = serde_json::from_str(&spanning).unwrap();
  let spanning = client.item(spanning["id"].as_i64().unwrap()).await.unwrap();

  assert_eq!(spanning.assignee_name.as_deref(), Some("the-base"));
  assert_eq!(spanning.labels["repositories"], "the-base,a-consumer,another-consumer");

  add(&client, &AddArgs::default())
    .await
    .expect_err("a row names its title, judgement and source");
}

#[tokio::test]
async fn a_leased_item_s_steps_stand_fixed() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = client
    .create_item(&NewItem {
      title: "ordered work".to_owned(),
      brief: "build the parser".to_owned(),
      queue_name: "todo".to_owned(),
      labels: BTreeMap::from([("lane".to_owned(), "todo".to_owned())]),
      ..NewItem::default()
    })
    .await
    .unwrap();
  let order = |steps: &[&str]| WorkOrderBody {
    purpose: "the parser".to_owned(),
    directive: "build it".to_owned(),
    steps: steps.iter().map(|step| (*step).to_owned()).collect(),
    state: None,
  };

  client.put_work_order(item.id, &order(&["investigate"])).await.unwrap();
  client
    .put_work_order(item.id, &order(&["investigate", "implement"]))
    .await
    .expect("open, its steps move");
  client
    .take_lease(
      &LeaseAsk {
        queue_name: "todo".to_owned(),
        holder_name: "worker".to_owned(),
        ttl_ms: 60_000,
        assignee_name: None,
        item_id: Some(item.id),
        frozen: None,
      },
      &LeaseWait { wait_ms: None },
    )
    .await
    .unwrap()
    .expect("the item is leased");

  client
    .put_work_order(item.id, &order(&["investigate", "implement", "lexer"]))
    .await
    .expect_err("a job's plan stands fixed while it holds the item");
  client
    .put_work_order(item.id, &order(&["investigate", "implement"]))
    .await
    .expect("the same steps again");

  assert_eq!(client.work_order(item.id).await.unwrap().steps, ["investigate", "implement"]);
}

#[tokio::test]
async fn a_closing_names_an_open_review_and_leaves_the_item_to_its_orderer() {
  let service = Service::start_unattended(&["lane=todo"]).await;
  let client = service.client();
  let item = leased(&service, "a reviewed item").await;
  let entry = |kind: ReviewKind| NewReview {
    kind,
    body: kind.as_str().to_owned(),
    actor_name: "examiner".to_owned(),
    r#ref: Some("job:1".to_owned()),
    stage: None,
    round: None,
    labels: Default::default(),
  };
  let closing = TodoArgs {
    done: item,
    report: Some("The work is done.".to_owned()),
    unfinished: None,
    by: "helper".to_owned(),
  };

  client.add_review(item, &entry(ReviewKind::Opened)).await.unwrap();
  let lines = close(&client, &closing).await.unwrap();

  assert!(lines.last().is_some_and(|line| line.contains("\"waitsOn\":\"review\"")), "{lines:?}");
  assert_eq!(client.item(item).await.unwrap().state, ItemState::Leased);

  client.add_review(item, &entry(ReviewKind::Accepted)).await.unwrap();
  let lines = close(&client, &closing).await.unwrap();

  assert!(lines.last().is_some_and(|line| line.contains("\"waitsOn\":\"orderer\"")), "{lines:?}");
  assert_eq!(client.item(item).await.unwrap().state, ItemState::Leased, "its orderer closes it");
}
