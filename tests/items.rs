//! Items through the real server: raised as drafts, opened by an approval, moved by the moves allowed, and
//! listed in queue order (docs/api.md, "Items").

mod support;

use std::collections::BTreeMap;

use bmo_job_client::{
  ApprovalGiven, ApprovalKind, ItemChange, ItemQuery, ItemState, LeaseAsk, LeaseWait, NewItem,
};

use support::{APPROVER, CALLER, CALLER_TOKEN, START, Service};

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

#[tokio::test]
async fn an_item_is_raised_as_a_draft_and_opened_by_an_approval() {
  let service = Service::start().await;
  let client = service.client();
  let mut new = raised("build", "write the parser");
  new.requester_name = Some("asker".to_owned());
  new.scope = Some("project-7".to_owned());
  new.labels = BTreeMap::from([("area".to_owned(), "parsing".to_owned())]);
  let item = client.create_item(&new).await.expect("raised");
  assert_eq!(item.state, ItemState::Draft);
  assert_eq!((item.position, item.priority), (1, 0));
  assert_eq!(item.created_at, START);
  assert_eq!(item.requester_name.as_deref(), Some("asker"));

  let refused = client
    .change_item(item.id, &moved(ItemState::Open))
    .await
    .expect_err("a draft opens with an approval");
  assert_eq!((refused.status(), refused.code()), (Some(409), Some("conflict")));

  service.clock.advance(5);
  let approval = ItemChange {
    approval: Some(ApprovalGiven::default()),
    ..ItemChange::default()
  };
  let opened = service.approver().change_item(item.id, &approval).await.expect("approved");
  assert_eq!(opened.state, ItemState::Open);
  assert_eq!(opened.approvals.len(), 1);
  assert_eq!(opened.approvals[0].at, START + 5);
  assert_eq!(opened.approvals[0].kind, ApprovalKind::Approval);
  assert_eq!(opened.approvals[0].caller_name.as_deref(), Some(APPROVER));

  let again = service.approver().change_item(item.id, &approval).await.expect("approved again");
  assert_eq!((again.state, again.approvals.len()), (ItemState::Open, 2));
}

#[tokio::test]
async fn a_callers_approval_that_is_no_approvers_is_a_recommendation_and_leaves_the_draft() {
  let service = Service::start().await;
  let item = service.client().create_item(&raised("build", "recommended")).await.expect("raised");
  let approval = ItemChange {
    approval: Some(ApprovalGiven::at(Some(START + 1))),
    ..ItemChange::default()
  };

  let caller = service.client_with(CALLER_TOKEN);
  let recommended = caller.change_item(item.id, &approval).await.expect("recorded");
  assert_eq!(recommended.state, ItemState::Draft);
  assert_eq!(recommended.approvals.len(), 1);
  assert_eq!(recommended.approvals[0].kind, ApprovalKind::Recommendation);
  assert_eq!(recommended.approvals[0].caller_name.as_deref(), Some(CALLER));
  assert_eq!(recommended.approvals[0].at, START + 1);

  let shared = service.client().change_item(item.id, &approval).await.expect("recorded");
  assert_eq!(shared.state, ItemState::Draft);
  assert_eq!(shared.approvals[1].kind, ApprovalKind::Recommendation);
  assert_eq!(shared.approvals[1].caller_name, None);

  let mut opening = approval.clone();
  opening.state = Some(ItemState::Open);
  let refused = caller.change_item(item.id, &opening).await.expect_err("a recommendation opens nothing");
  assert_eq!((refused.status(), refused.code()), (Some(409), Some("conflict")));
  let refused = caller
    .change_item(item.id, &moved(ItemState::Open))
    .await
    .expect_err("no approver's approval");
  assert_eq!((refused.status(), refused.code()), (Some(409), Some("conflict")));
  assert_eq!(service.client().item(item.id).await.expect("read").approvals.len(), 2);

  let opened = service.approver().change_item(item.id, &approval).await.expect("approved");
  assert_eq!(opened.state, ItemState::Open);
  let kinds: Vec<ApprovalKind> = opened.approvals.iter().map(|approval| approval.kind).collect();
  assert_eq!(
    kinds,
    [ApprovalKind::Recommendation, ApprovalKind::Recommendation, ApprovalKind::Approval]
  );
}

#[tokio::test]
async fn a_typed_actor_name_has_no_effect() {
  let service = Service::start().await;
  let item = service.client().create_item(&raised("build", "typed")).await.expect("raised");
  let typed = serde_json::json!({ "approval": { "actorName": APPROVER } });

  let answer = service
    .raw(reqwest::Method::PATCH, &format!("/items/{}", item.id))
    .json(&typed)
    .send()
    .await
    .expect("answered");
  assert_eq!(answer.status(), 200);
  let recommended: serde_json::Value = answer.json().await.expect("the item");
  assert_eq!(recommended["state"], "draft");
  assert_eq!(
    recommended["approvals"],
    serde_json::json!([{ "kind": "recommendation", "callerName": null, "at": START }])
  );

  let answer = reqwest::Client::new()
    .patch(format!("{}/items/{}", service.url, item.id))
    .bearer_auth(support::APPROVER_TOKEN)
    .json(&serde_json::json!({ "approval": { "actorName": "somebody-else" } }))
    .send()
    .await
    .expect("answered");
  let approved: serde_json::Value = answer.json().await.expect("the item");
  assert_eq!(approved["state"], "open");
  assert_eq!(approved["approvals"][1]["callerName"], APPROVER);
  assert_eq!(approved["approvals"][1]["kind"], "approval");
}

#[tokio::test]
async fn the_moves_follow_the_table() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "one").await;

  let refused = client
    .change_item(item.id, &moved(ItemState::Done))
    .await
    .expect_err("only a leased item is done");
  assert_eq!(refused.status(), Some(409));
  let refused = client
    .change_item(item.id, &moved(ItemState::Leased))
    .await
    .expect_err("a lease alone leases");
  assert_eq!(refused.status(), Some(409));

  let held = client.change_item(item.id, &moved(ItemState::Held)).await.expect("held");
  assert_eq!(held.state, ItemState::Held);
  let open = client.change_item(item.id, &moved(ItemState::Open)).await.expect("released");
  assert_eq!(open.state, ItemState::Open);

  let ask = LeaseAsk {
    queue_name: "build".to_owned(),
    holder_name: "taker".to_owned(),
    ttl_ms: 60_000,
    ..LeaseAsk::default()
  };
  let lease = client.take_lease(&ask, &LeaseWait::default()).await.expect("taken").expect("a lease");
  service.clock.advance(10);
  let done = client.change_item(item.id, &moved(ItemState::Done)).await.expect("done");
  assert_eq!((done.state, done.lease_id, done.ended_at), (ItemState::Done, None, Some(START + 10)));
  let gone = client.lease(lease.id).await.expect_err("the lease ended with its item");
  assert_eq!(gone.code(), Some("gone"));

  let refused = client
    .change_item(item.id, &moved(ItemState::Dropped))
    .await
    .expect_err("done is final");
  assert_eq!(refused.status(), Some(409));
}

#[tokio::test]
async fn a_merge_names_a_standing_item_other_than_itself() {
  let service = Service::start().await;
  let client = service.client();
  let kept = service.open_item("build", "kept").await;
  let folded = service.open_item("build", "folded").await;
  let merge = |into: i64| ItemChange {
    state: Some(ItemState::Merged),
    merged_into: Some(into),
    ..ItemChange::default()
  };

  let unknown = client.change_item(folded.id, &merge(9_999)).await.expect_err("no such item");
  assert_eq!((unknown.status(), unknown.code()), (Some(422), Some("unknown-item")));
  let itself = client.change_item(folded.id, &merge(folded.id)).await.expect_err("not into itself");
  assert_eq!(itself.code(), Some("conflict"));
  let alone = ItemChange {
    merged_into: Some(kept.id),
    ..ItemChange::default()
  };
  assert_eq!(
    client.change_item(folded.id, &alone).await.expect_err("with its state").code(),
    Some("bad-request")
  );

  let merged = client.change_item(folded.id, &merge(kept.id)).await.expect("merged");
  assert_eq!((merged.state, merged.merged_into), (ItemState::Merged, Some(kept.id)));
  let into_merged = client
    .change_item(kept.id, &merge(folded.id))
    .await
    .expect_err("a merged item takes no merge");
  assert_eq!(into_merged.code(), Some("conflict"));
}

#[tokio::test]
async fn the_list_is_in_queue_order_and_pages_past_an_item() {
  let service = Service::start().await;
  let client = service.client();
  let first = client.create_item(&raised("build", "first")).await.expect("raised");
  let second = client.create_item(&raised("build", "second")).await.expect("raised");
  let mut urgent = raised("build", "urgent");
  urgent.priority = Some(5);
  let urgent = client.create_item(&urgent).await.expect("raised");
  let moved_up = ItemChange {
    position: Some(0),
    ..ItemChange::default()
  };
  client.change_item(second.id, &moved_up).await.expect("moved");

  let ids = |page: &bmo_job_client::Page<bmo_job_client::Item>| {
    page.items.iter().map(|item| item.id).collect::<Vec<_>>()
  };
  let all = client.items(&ItemQuery::default()).await.expect("listed");
  assert_eq!(ids(&all), [urgent.id, second.id, first.id]);

  let page = ItemQuery {
    limit: Some(1),
    ..ItemQuery::default()
  };
  let one = client.items(&page).await.expect("listed");
  assert_eq!((ids(&one), one.more), (vec![urgent.id], true));
  let next = ItemQuery {
    after: Some(urgent.id),
    limit: Some(1),
    ..ItemQuery::default()
  };
  assert_eq!(ids(&client.items(&next).await.expect("listed")), [second.id]);
  let last = ItemQuery {
    after: Some(second.id),
    ..ItemQuery::default()
  };
  let last = client.items(&last).await.expect("listed");
  assert_eq!((ids(&last), last.more), (vec![first.id], false));
}

#[tokio::test]
async fn the_list_filters_by_state_queue_scope_label_ids_and_words() {
  let service = Service::start().await;
  let client = service.client();
  let mut parser = raised("build", "Write the Parser");
  parser.scope = Some("project-7".to_owned());
  parser.labels = BTreeMap::from([("area".to_owned(), "parsing".to_owned())]);
  parser.assignee_name = Some("builder".to_owned());
  let parser = client.create_item(&parser).await.expect("raised");
  let docs = service.open_item("docs", "write the guide").await;

  let ids = |query: ItemQuery| {
    let client = client.clone();
    async move {
      client
        .items(&query)
        .await
        .expect("listed")
        .items
        .iter()
        .map(|item| item.id)
        .collect::<Vec<_>>()
    }
  };
  assert_eq!(
    ids(ItemQuery {
      state: Some("open".into()),
      ..Default::default()
    })
    .await,
    [docs.id]
  );
  assert_eq!(
    ids(ItemQuery {
      state: Some("draft,open".into()),
      ..Default::default()
    })
    .await
    .len(),
    2
  );
  assert_eq!(
    ids(ItemQuery {
      queue_name: Some("docs".into()),
      ..Default::default()
    })
    .await,
    [docs.id]
  );
  assert_eq!(
    ids(ItemQuery {
      scope: Some("project-7".into()),
      ..Default::default()
    })
    .await,
    [parser.id]
  );
  assert_eq!(
    ids(ItemQuery {
      assignee_name: Some("builder".into()),
      ..Default::default()
    })
    .await,
    [parser.id]
  );
  assert_eq!(
    ids(ItemQuery {
      label: Some("area".into()),
      ..Default::default()
    })
    .await,
    [parser.id]
  );
  assert_eq!(
    ids(ItemQuery {
      label: Some("area=parsing".into()),
      ..Default::default()
    })
    .await,
    [parser.id]
  );
  assert!(
    ids(ItemQuery {
      label: Some("area=docs".into()),
      ..Default::default()
    })
    .await
    .is_empty()
  );
  assert_eq!(
    ids(ItemQuery {
      ids: Some(format!("{},999", docs.id)),
      ..Default::default()
    })
    .await,
    [docs.id]
  );
  assert_eq!(
    ids(ItemQuery {
      q: Some("parser WRITE".into()),
      ..Default::default()
    })
    .await,
    [parser.id]
  );
  assert_eq!(
    ids(ItemQuery {
      q: Some("guide".into()),
      ..Default::default()
    })
    .await,
    [docs.id]
  );

  let refused = client
    .items(&ItemQuery {
      state: Some("lost".into()),
      ..Default::default()
    })
    .await;
  assert_eq!(refused.expect_err("no such state").code(), Some("bad-request"));
}

#[tokio::test]
async fn a_change_is_a_merge_patch() {
  let service = Service::start().await;
  let client = service.client();
  let mut new = raised("build", "one");
  new.assignee_name = Some("builder".to_owned());
  let item = client.create_item(&new).await.expect("raised");

  let cleared: ItemChange =
    serde_json::from_value(serde_json::json!({ "assigneeName": null, "brief": "new words" }))
      .expect("a change reads");
  let changed = client.change_item(item.id, &cleared).await.expect("changed");
  assert_eq!(
    (changed.assignee_name, changed.brief.as_str(), changed.title.as_str()),
    (None, "new words", "one")
  );

  let unknown = service
    .raw(reqwest::Method::PATCH, &format!("/items/{}", item.id))
    .json(&serde_json::json!({ "colour": "red" }))
    .send()
    .await
    .expect("answered");
  assert_eq!(unknown.status(), 400);
  let missing = client.item(9_999).await.expect_err("no such item");
  assert_eq!((missing.status(), missing.code()), (Some(404), Some("not-found")));
}
