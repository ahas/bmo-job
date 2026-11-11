//! What the service holds under a caller's scope, counted and let go (docs/api.md, "Scopes").

use crate::support;

use bmo_job_client::{
  EventQuery, LeaseAsk, LeaseWait, NewItem, NewNote, NewQuestion, NewSubscription, ScopeCounts,
  ScopeDeletion, WorkOrderBody,
};

use support::Service;

#[tokio::test]
async fn a_scope_is_counted_previewed_and_let_go() {
  let service = Service::start().await;
  let client = service.client();
  assert_eq!(client.scope("project-7").await.expect_err("nothing held").code(), Some("not-found"));

  let item = client
    .create_item(&NewItem {
      title: "scoped".into(),
      queue_name: "build".into(),
      scope: Some("project-7".into()),
      ..NewItem::default()
    })
    .await
    .expect("raised");
  let approval = bmo_job_client::ItemChange {
    approval: Some(bmo_job_client::ApprovalGiven::default()),
    ..Default::default()
  };
  service.approver().change_item(item.id, &approval).await.expect("opened");
  client
    .add_note(
      item.id,
      &NewNote {
        text: "a word".into(),
        actor_name: "asker".into(),
        ..NewNote::default()
      },
    )
    .await
    .expect("noted");
  client
    .put_work_order(
      item.id,
      &WorkOrderBody {
        purpose: "p".into(),
        directive: "d".into(),
        steps: vec!["one".into()],
        state: None,
      },
    )
    .await
    .expect("ordered");
  client
    .take_lease(
      &LeaseAsk {
        queue_name: "build".into(),
        holder_name: "taker".into(),
        ttl_ms: 60_000,
        ..Default::default()
      },
      &LeaseWait::default(),
    )
    .await
    .expect("answered")
    .expect("a lease");
  client
    .ask(&NewQuestion {
      item_id: Some(item.id),
      asker_name: "taker".into(),
      text: "how?".into(),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  client
    .subscribe(&NewSubscription {
      holder_name: "taker".into(),
      scope: Some("project-7".into()),
      ..NewSubscription::default()
    })
    .await
    .expect("subscribed");
  let unscoped = service.open_item("build", "unscoped").await;

  let counts = ScopeCounts {
    items: 1,
    notes: 1,
    reports: 0,
    reviews: 0,
    work_orders: 1,
    tasks: 1,
    leases: 1,
    questions: 1,
    replies: 0,
    subscriptions: 1,
  };
  let held = client.scope("project-7").await.expect("held");
  assert_eq!((held.counts.clone(), held.state), (counts.clone(), None));
  let preview = client
    .delete_scope(
      "project-7",
      &ScopeDeletion {
        dry_run: Some(true),
      },
    )
    .await
    .expect("previewed");
  assert_eq!((preview.counts.clone(), preview.state.as_deref()), (counts.clone(), Some("preview")));
  assert!(client.item(item.id).await.is_ok(), "a preview lets nothing go");

  let deleted = client.delete_scope("project-7", &ScopeDeletion::default()).await.expect("let go");
  assert_eq!((deleted.counts, deleted.state.as_deref()), (counts, Some("deleted")));
  assert_eq!(client.item(item.id).await.expect_err("gone").code(), Some("not-found"));
  assert!(client.item(unscoped.id).await.is_ok(), "another scope's member stays");
  assert_eq!(client.scope("project-7").await.expect_err("nothing held").code(), Some("not-found"));

  let told = EventQuery {
    kinds: Some("item.deleted,question.deleted,subscription.deleted,lease.deleted".into()),
    ..EventQuery::default()
  };
  let kinds: Vec<String> = client
    .events_page(&told)
    .await
    .expect("read")
    .items
    .into_iter()
    .map(|event| event.kind)
    .collect();
  assert_eq!(kinds, ["question.deleted", "lease.deleted", "item.deleted", "subscription.deleted"]);
}
