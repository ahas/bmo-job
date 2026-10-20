//! Notes and reports on an item through the real server (docs/api.md, "Notes" and "Reports").

mod support;

use bmo_job_client::{NewNote, NewReport, PageQuery, ReportKind, WaitQuery};

use support::Service;

fn note(text: &str) -> NewNote {
  NewNote {
    text: text.to_owned(),
    actor_name: "asker".to_owned(),
    ..NewNote::default()
  }
}

#[tokio::test]
async fn notes_are_appended_read_past_a_cursor_and_taken_back() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "one").await;
  let first = client.add_note(item.id, &note("also cover the edge case")).await.expect("appended");
  let second = client.add_note(item.id, &note("and the docs")).await.expect("appended");

  let all = client.notes(item.id, &WaitQuery::default()).await.expect("read");
  assert_eq!(all.items.iter().map(|note| note.id).collect::<Vec<_>>(), [first.id, second.id]);
  let past = WaitQuery {
    after: Some(first.id),
    ..WaitQuery::default()
  };
  assert_eq!(client.notes(item.id, &past).await.expect("read").items, std::slice::from_ref(&second));

  client.delete_note(item.id, first.id).await.expect("taken back");
  assert_eq!(client.notes(item.id, &WaitQuery::default()).await.expect("read").items, [second]);
  assert_eq!(client.delete_note(item.id, first.id).await.expect_err("gone").code(), Some("not-found"));
  assert_eq!(
    client.add_note(item.id, &note(" ")).await.expect_err("empty").code(),
    Some("bad-request")
  );
  assert_eq!(client.add_note(9_999, &note("x")).await.expect_err("no item").code(), Some("not-found"));
}

#[tokio::test]
async fn a_wait_for_notes_ends_when_one_is_appended() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "one").await;
  let waiting = {
    let client = client.clone();
    tokio::spawn(async move {
      client
        .notes(
          item.id,
          &WaitQuery {
            wait_ms: Some(60_000),
            ..WaitQuery::default()
          },
        )
        .await
    })
  };
  let appended = client.add_note(item.id, &note("one more thing")).await.expect("appended");
  let page = waiting.await.expect("the wait ends").expect("read");
  assert_eq!(page.items, [appended]);

  let nothing = WaitQuery {
    wait_ms: Some(20),
    after: Some(page.items[0].id),
    ..WaitQuery::default()
  };
  assert!(client.notes(item.id, &nothing).await.expect("read").items.is_empty());
}

#[tokio::test]
async fn reports_are_appended_oldest_first() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "one").await;
  for (kind, body) in [(ReportKind::Progress, "halfway"), (ReportKind::Final, "done, tests pass")] {
    let report = NewReport {
      kind,
      body: body.to_owned(),
      actor_name: "builder".to_owned(),
      labels: Default::default(),
    };
    client.add_report(item.id, &report).await.expect("appended");
  }
  let reports = client.reports(item.id, &PageQuery::default()).await.expect("read");
  let kinds: Vec<ReportKind> = reports.items.iter().map(|report| report.kind).collect();
  assert_eq!(kinds, [ReportKind::Progress, ReportKind::Final]);

  let wrong = service
    .raw(reqwest::Method::POST, &format!("/items/{}/reports", item.id))
    .json(&serde_json::json!({ "kind": "rumour", "body": "x", "actorName": "y" }))
    .send()
    .await
    .expect("answered");
  assert_eq!(wrong.status(), 400);
}
