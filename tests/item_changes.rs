//! What an item's events say of a change, and the item an item was raised from, through the real server
//! (docs/api.md, "Events" and "Raised from an item").

use crate::support;

use bmo_job_client::{
  ApprovalGiven, EventQuery, ItemChange, ItemEvent, ItemQuery, ItemState, JobClient, LeaseAsk, LeaseWait,
  NewItem, NewReport, ReportKind,
};

use support::{APPROVER, CALLER, CALLER_TOKEN, Service};

const AT_ONCE: LeaseWait = LeaseWait { wait_ms: None };

fn raised(queue: &str, title: &str) -> NewItem {
  NewItem {
    title: title.to_owned(),
    brief: format!("the brief of {title}"),
    queue_name: queue.to_owned(),
    ..NewItem::default()
  }
}

fn moved(state: ItemState) -> ItemChange {
  ItemChange {
    state: Some(state),
    ..ItemChange::default()
  }
}

fn approval() -> ItemChange {
  ItemChange {
    approval: Some(ApprovalGiven::default()),
    ..ItemChange::default()
  }
}

/// A lease on the item `item_id` of `queue`, taken by `client`.
async fn leased(client: &JobClient, queue: &str, item_id: i64, ttl_ms: i64) -> bmo_job_client::Lease {
  let ask = LeaseAsk {
    queue_name: queue.to_owned(),
    holder_name: "taker".to_owned(),
    item_id: Some(item_id),
    ttl_ms,
    ..LeaseAsk::default()
  };
  client.take_lease(&ask, &AT_ONCE).await.expect("answered").expect("a lease")
}

/// Every event of `kind` about the item `item_id`, oldest first, as an item's event reads.
async fn told(service: &Service, kind: &str, item_id: i64) -> Vec<ItemEvent> {
  let query = EventQuery {
    kinds: Some(kind.to_owned()),
    limit: Some(500),
    ..EventQuery::default()
  };
  let events = service.client().events_page(&query).await.expect("read").items;
  events
    .into_iter()
    .filter(|event| event.item_id() == Some(item_id))
    .map(|event| serde_json::from_value(event.data.into()).expect("an item's event"))
    .collect()
}

/// The state each `item.changed` of the item left and who changed it, oldest first.
async fn changes(service: &Service, item_id: i64) -> Vec<(Option<ItemState>, ItemState, Option<String>)> {
  told(service, "item.changed", item_id)
    .await
    .into_iter()
    .map(|event| (event.previous_state, event.item.state, event.changed_by))
    .collect()
}

fn by(name: &str) -> Option<String> {
  Some(name.to_owned())
}

#[tokio::test]
async fn each_move_tells_the_state_it_left_and_the_caller_who_made_it() {
  let service = Service::start().await;
  let caller = service.client_with(CALLER_TOKEN);
  let shared = service.client();

  let item = caller.create_item(&raised("build", "parser")).await.expect("raised");
  let created = told(&service, "item.created", item.id).await;
  assert_eq!(created.len(), 1);
  assert_eq!((created[0].previous_state, created[0].changed_by.clone()), (None, by(CALLER)));
  assert_eq!(created[0].item, item);

  service.approver().change_item(item.id, &approval()).await.expect("opened");
  leased(&caller, "build", item.id, 60_000).await;
  let held = ItemChange {
    reason: Some("waits on a decision".to_owned()),
    ..moved(ItemState::Held)
  };
  caller.change_item(item.id, &held).await.expect("held");
  shared.change_item(item.id, &moved(ItemState::Open)).await.expect("open again");
  let retitled = ItemChange {
    title: Some("the parser".to_owned()),
    ..ItemChange::default()
  };
  caller.change_item(item.id, &retitled).await.expect("retitled");
  let lease = leased(&caller, "build", item.id, 60_000).await;
  service.approver().release_lease(lease.id).await.expect("given back");
  leased(&caller, "build", item.id, 60_000).await;
  let report = NewReport {
    kind: ReportKind::Final,
    body: "The work is done.".to_owned(),
    actor_name: "taker".to_owned(),
    labels: Default::default(),
  };
  caller.add_report(item.id, &report).await.expect("reported");
  caller.change_item(item.id, &moved(ItemState::Done)).await.expect("done");

  use ItemState::*;
  assert_eq!(
    changes(&service, item.id).await,
    [
      (Some(Draft), Open, by(APPROVER)),
      (Some(Open), Leased, by(CALLER)),
      (Some(Leased), Held, by(CALLER)),
      (Some(Held), Open, None),
      (None, Open, by(CALLER)),
      (Some(Open), Leased, by(CALLER)),
      (Some(Leased), Open, by(APPROVER)),
      (Some(Open), Leased, by(CALLER)),
      (Some(Leased), Done, by(CALLER)),
    ]
  );

  let dropped = service.open_item("build", "dropped").await;
  caller.change_item(dropped.id, &moved(Dropped)).await.expect("dropped");
  let merged = service.open_item("build", "merged").await;
  leased(&caller, "build", merged.id, 60_000).await;
  let merge = ItemChange {
    merged_into: Some(item.id),
    ..moved(Merged)
  };
  service.approver().change_item(merged.id, &merge).await.expect("merged");
  assert_eq!(changes(&service, dropped.id).await.last(), Some(&(Some(Open), Dropped, by(CALLER))));
  assert_eq!(changes(&service, merged.id).await.last(), Some(&(Some(Leased), Merged, by(APPROVER))));
}

#[tokio::test]
async fn an_approval_that_opens_a_draft_names_the_approver_and_a_recommendation_moves_nothing() {
  let service = Service::start().await;
  let item = service.client().create_item(&raised("build", "draft")).await.expect("raised");
  assert_eq!(
    told(&service, "item.created", item.id).await[0].changed_by,
    None,
    "the shared token is no caller"
  );

  service
    .client_with(CALLER_TOKEN)
    .change_item(item.id, &approval())
    .await
    .expect("recommended");
  service.approver().change_item(item.id, &approval()).await.expect("approved");
  assert_eq!(
    changes(&service, item.id).await,
    [
      (None, ItemState::Draft, by(CALLER)),
      (Some(ItemState::Draft), ItemState::Open, by(APPROVER))
    ]
  );
}

#[tokio::test]
async fn the_sweep_that_reopens_an_item_names_the_service() {
  let service = Service::start().await;
  let item = service.open_item("build", "run out").await;
  leased(&service.client_with(CALLER_TOKEN), "build", item.id, 1_000).await;
  service.pass(1_001).await;
  assert_eq!(
    changes(&service, item.id).await.last(),
    Some(&(Some(ItemState::Leased), ItemState::Open, by("bmo-job")))
  );
}

#[tokio::test]
async fn raised_by_is_kept_returned_filtered_and_refused_for_no_item() {
  let service = Service::start().await;
  let client = service.client();
  let parent = client.create_item(&raised("build", "parent")).await.expect("raised");
  let child = client
    .create_item(&NewItem {
      raised_by: Some(parent.id),
      ..raised("build", "child")
    })
    .await
    .expect("raised");
  client.create_item(&raised("build", "unrelated")).await.expect("raised");
  assert_eq!(child.raised_by, Some(parent.id));
  assert_eq!(parent.raised_by, None);
  assert_eq!(client.item(child.id).await.expect("read").raised_by, Some(parent.id));
  assert_eq!(told(&service, "item.created", child.id).await[0].item.raised_by, Some(parent.id));

  let from_parent = ItemQuery {
    raised_by: Some(parent.id),
    ..ItemQuery::default()
  };
  let listed = client.items(&from_parent).await.expect("listed").items;
  assert_eq!(listed.iter().map(|item| item.id).collect::<Vec<_>>(), [child.id]);

  let refused = client
    .create_item(&NewItem {
      raised_by: Some(parent.id + 100),
      ..raised("build", "orphan")
    })
    .await
    .expect_err("no such item");
  assert_eq!((refused.status(), refused.code()), (Some(422), Some("unknown-item")));

  let changed = service
    .raw(reqwest::Method::PATCH, &format!("/items/{}", child.id))
    .json(&serde_json::json!({ "raisedBy": null }))
    .send()
    .await
    .expect("answered");
  assert_eq!(changed.status(), 400, "raisedBy is fixed when the item is raised");
  assert_eq!(client.item(child.id).await.expect("read").raised_by, Some(parent.id));
}

#[tokio::test]
async fn a_raise_with_no_raised_by_takes_the_item_of_the_callers_one_lease() {
  let service = Service::start().await;
  let caller = service.client_with(CALLER_TOKEN);

  let none = caller.create_item(&raised("build", "with no lease")).await.expect("raised");
  assert_eq!(none.raised_by, None, "a caller holding no lease");

  let held = service.open_item("build", "held").await;
  leased(&caller, "build", held.id, 60_000).await;
  let follow_up = caller.create_item(&raised("build", "follow-up")).await.expect("raised");
  assert_eq!(follow_up.raised_by, Some(held.id), "a caller holding one lease");
  let named = caller
    .create_item(&NewItem {
      raised_by: Some(none.id),
      ..raised("build", "named")
    })
    .await
    .expect("raised");
  assert_eq!(named.raised_by, Some(none.id), "a raisedBy given is kept over the lease");
  let shared = service.client().create_item(&raised("build", "shared")).await.expect("raised");
  assert_eq!(shared.raised_by, None, "the shared token is no caller and holds no lease");
  let other = service.approver().create_item(&raised("build", "other")).await.expect("raised");
  assert_eq!(other.raised_by, None, "another caller holds none of this caller's leases");

  let second = service.open_item("build", "second").await;
  leased(&caller, "build", second.id, 60_000).await;
  let two = caller.create_item(&raised("build", "with two leases")).await.expect("raised");
  assert_eq!(two.raised_by, None, "a caller holding two leases");
}
