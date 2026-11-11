//! An item's review through the real server (docs/api.md, "Reviews").

use crate::support;

use bmo_job_client::{NewReview, ReviewKind, ReviewQuery};

use support::Service;

fn entry(kind: ReviewKind, round: i64, run: &str) -> NewReview {
  NewReview {
    kind,
    body: format!("{} of round {round}", kind.as_str()),
    actor_name: "examiner".to_owned(),
    r#ref: Some(run.to_owned()),
    stage: Some(1),
    round: Some(round),
    labels: Default::default(),
  }
}

#[tokio::test]
async fn entries_are_kept_in_order_and_the_item_counts_its_rejections() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "one").await;
  let moves = [
    (ReviewKind::Report, 1, "job:1"),
    (ReviewKind::Question, 1, "job:1"),
    (ReviewKind::Answer, 1, "job:1"),
    (ReviewKind::Rejection, 1, "job:1"),
    (ReviewKind::Report, 2, "job:1"),
    (ReviewKind::Rejection, 2, "job:1"),
    (ReviewKind::Report, 1, "job:2"),
    (ReviewKind::Accepted, 1, "job:2"),
  ];
  for (kind, round, run) in moves {
    client.add_review(item.id, &entry(kind, round, run)).await.expect("appended");
  }

  let all = client.reviews(item.id, &ReviewQuery::default()).await.expect("read");
  let read: Vec<_> = all.items.iter().map(|entry| (entry.kind, entry.round, entry.r#ref.clone())).collect();
  let written: Vec<_> = moves
    .iter()
    .map(|(kind, round, run)| (*kind, *round, Some((*run).to_owned())))
    .collect();
  assert_eq!(read, written);

  let one_run = ReviewQuery {
    r#ref: Some("job:2".to_owned()),
    ..ReviewQuery::default()
  };
  assert_eq!(client.reviews(item.id, &one_run).await.expect("read").items.len(), 2);
  assert_eq!(client.item(item.id).await.expect("read").rejections, 2);
}

#[tokio::test]
async fn a_round_or_stage_below_one_and_an_unknown_kind_are_refused() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "one").await;
  let path = format!("/items/{}/reviews", item.id);
  for body in [
    serde_json::json!({ "kind": "report", "body": "x", "actorName": "y", "round": 0 }),
    serde_json::json!({ "kind": "report", "body": "x", "actorName": "y", "stage": 0 }),
    serde_json::json!({ "kind": "rumour", "body": "x", "actorName": "y" }),
    serde_json::json!({ "kind": "report", "body": " ", "actorName": "y" }),
  ] {
    let answer = service.raw(reqwest::Method::POST, &path).json(&body).send().await.expect("answered");
    assert_eq!(answer.status(), 400, "{body}");
  }
  let missing = service
    .raw(reqwest::Method::POST, "/items/9999/reviews")
    .json(&serde_json::json!({ "kind": "report", "body": "x", "actorName": "y" }))
    .send()
    .await
    .expect("answered");
  assert_eq!(missing.status(), 404);
  assert!(client.reviews(item.id, &ReviewQuery::default()).await.expect("read").items.is_empty());
}
