//! Leases through the real server: the next item or the one named, raced for, held to their queue's state,
//! renewed, given back and run out (docs/api.md, "Leases").

mod support;

use bmo_job_client::{
  Hold, ItemChange, ItemState, LeaseAsk, LeaseQuery, LeaseRenewal, LeaseWait, NewItem, QueueChange,
  QueueState,
};

use support::{START, Service};

fn ask(queue: &str, holder: &str) -> LeaseAsk {
  LeaseAsk {
    queue_name: queue.to_owned(),
    holder_name: holder.to_owned(),
    ttl_ms: 60_000,
    ..LeaseAsk::default()
  }
}

const AT_ONCE: LeaseWait = LeaseWait { wait_ms: None };

#[tokio::test]
async fn the_next_open_item_is_taken_and_nothing_waiting_answers_none() {
  let service = Service::start().await;
  let client = service.client();
  assert!(client.take_lease(&ask("build", "taker"), &AT_ONCE).await.expect("answered").is_none());

  let first = service.open_item("build", "first").await;
  service.open_item("build", "second").await;
  let lease = client
    .take_lease(&ask("build", "taker"), &AT_ONCE)
    .await
    .expect("answered")
    .expect("a lease");
  assert_eq!((lease.item_id, lease.holder_name.as_str()), (first.id, "taker"));
  assert_eq!((lease.expires_at, lease.item.state), (START + 60_000, ItemState::Leased));
  assert_eq!(lease.item.lease_id, Some(lease.id));

  let standing = client.leases(&LeaseQuery::default()).await.expect("listed");
  assert_eq!(standing.items.len(), 1);
  let other = LeaseQuery {
    holder_name: Some("someone else".to_owned()),
    ..LeaseQuery::default()
  };
  assert!(client.leases(&other).await.expect("listed").items.is_empty());
}

#[tokio::test]
async fn harnesses_told_of_one_item_race_and_one_wins() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "raced").await;
  let named = |holder: &str| LeaseAsk {
    item_id: Some(item.id),
    ..ask("build", holder)
  };
  let first = client.take_lease(&named("first"), &AT_ONCE).await.expect("taken").expect("a lease");
  assert_eq!(first.item_id, item.id);
  let second = client.take_lease(&named("second"), &AT_ONCE).await.expect_err("taken first by another");
  assert_eq!((second.status(), second.code()), (Some(409), Some("conflict")));

  let elsewhere = client
    .take_lease(
      &LeaseAsk {
        item_id: Some(item.id),
        ..ask("docs", "x")
      },
      &AT_ONCE,
    )
    .await;
  assert_eq!(elsewhere.expect_err("not in that queue").code(), Some("bad-request"));
  let unknown = client
    .take_lease(
      &LeaseAsk {
        item_id: Some(9_999),
        ..ask("build", "x")
      },
      &AT_ONCE,
    )
    .await;
  assert_eq!(unknown.expect_err("no such item").code(), Some("unknown-item"));
}

#[tokio::test]
async fn an_assigned_item_goes_only_to_its_assignee() {
  let service = Service::start().await;
  let client = service.client();
  let mine = service.open_item("build", "mine").await;
  let assigned = ItemChange {
    assignee_name: Some(Some("builder".to_owned())),
    ..ItemChange::default()
  };
  client.change_item(mine.id, &assigned).await.expect("assigned");
  assert!(client.take_lease(&ask("build", "anyone"), &AT_ONCE).await.expect("answered").is_none());
  let as_builder = LeaseAsk {
    assignee_name: Some("builder".to_owned()),
    ..ask("build", "builder-1")
  };
  let lease = client.take_lease(&as_builder, &AT_ONCE).await.expect("answered").expect("a lease");
  assert_eq!(lease.item_id, mine.id);
}

#[tokio::test]
async fn a_paused_or_held_queue_gives_no_lease_until_it_opens() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "waiting").await;
  let paused = client
    .change_queue(
      "build",
      &QueueChange {
        state: Some(QueueState::Paused),
        ..QueueChange::default()
      },
    )
    .await
    .expect("paused");
  assert_eq!((paused.state, paused.open_count), (QueueState::Paused, 1));
  assert!(client.take_lease(&ask("build", "taker"), &AT_ONCE).await.expect("answered").is_none());
  let named = client
    .take_lease(
      &LeaseAsk {
        item_id: Some(item.id),
        ..ask("build", "taker")
      },
      &AT_ONCE,
    )
    .await;
  assert_eq!(named.expect_err("the queue is paused").code(), Some("queue-not-open"));

  let held = QueueChange {
    state: Some(QueueState::Open),
    holds: Some(vec![Hold {
      reason: "a release is going out".to_owned(),
      actor_name: "operator".to_owned(),
      until: Some(START + 1_000),
    }]),
  };
  client.change_queue("build", &held).await.expect("held");
  assert!(client.take_lease(&ask("build", "taker"), &AT_ONCE).await.expect("answered").is_none());

  service.pass(1_000).await;
  assert!(client.queue("build").await.expect("read").holds.is_empty(), "the hold ran out");
  let lease = client
    .take_lease(&ask("build", "taker"), &AT_ONCE)
    .await
    .expect("answered")
    .expect("a lease");
  assert_eq!(lease.item_id, item.id);
}

#[tokio::test]
async fn a_wait_ends_when_an_item_opens() {
  let service = Service::start().await;
  let client = service.client();
  let waiting = {
    let client = client.clone();
    tokio::spawn(async move {
      client
        .take_lease(
          &ask("build", "taker"),
          &LeaseWait {
            wait_ms: Some(60_000),
          },
        )
        .await
    })
  };
  let item = service.open_item("build", "arrives").await;
  let lease = waiting.await.expect("the wait ends").expect("answered").expect("a lease");
  assert_eq!(lease.item_id, item.id);
}

#[tokio::test]
async fn a_lease_is_renewed_given_back_and_run_out() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "held for a while").await;
  let lease = client
    .take_lease(&ask("build", "taker"), &AT_ONCE)
    .await
    .expect("answered")
    .expect("a lease");

  service.pass(30_000).await;
  let renewed = client.renew_lease(lease.id, &LeaseRenewal { ttl_ms: 60_000 }).await.expect("renewed");
  assert_eq!(renewed.expires_at, START + 90_000);
  let refused = client.renew_lease(lease.id, &LeaseRenewal { ttl_ms: 10 }).await.expect_err("too short");
  assert_eq!(refused.code(), Some("bad-request"));

  client.release_lease(lease.id).await.expect("given back");
  assert_eq!(client.item(item.id).await.expect("read").state, ItemState::Open);
  assert_eq!(client.release_lease(lease.id).await.expect_err("ended").code(), Some("gone"));

  let again = client
    .take_lease(&ask("build", "taker"), &AT_ONCE)
    .await
    .expect("answered")
    .expect("a lease");
  service.pass(59_999).await;
  assert_eq!(client.item(item.id).await.expect("read").state, ItemState::Leased);
  service.pass(1).await;
  let lapsed = client.item(item.id).await.expect("read");
  assert_eq!((lapsed.state, lapsed.lease_id), (ItemState::Open, None));
  assert_eq!(client.lease(again.id).await.expect_err("ran out").code(), Some("gone"));
  let renew = client.renew_lease(again.id, &LeaseRenewal { ttl_ms: 60_000 }).await;
  assert_eq!(renew.expect_err("ran out").status(), Some(410));
}

#[tokio::test]
async fn holding_an_item_ends_its_lease_and_a_leased_item_stays_in_its_queue() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "paused work").await;
  let lease = client
    .take_lease(&ask("build", "taker"), &AT_ONCE)
    .await
    .expect("answered")
    .expect("a lease");
  let elsewhere = ItemChange {
    queue_name: Some("docs".to_owned()),
    ..ItemChange::default()
  };
  assert_eq!(
    client.change_item(item.id, &elsewhere).await.expect_err("leased").code(),
    Some("conflict")
  );
  let held = ItemChange {
    state: Some(ItemState::Held),
    ..ItemChange::default()
  };
  let held = client.change_item(item.id, &held).await.expect("held");
  assert_eq!((held.state, held.lease_id), (ItemState::Held, None));
  assert_eq!(client.lease(lease.id).await.expect_err("ended").code(), Some("gone"));
  let refused = client
    .create_item(&NewItem {
      title: "x".into(),
      queue_name: "no spaces allowed".into(),
      ..NewItem::default()
    })
    .await;
  assert_eq!(refused.expect_err("a queue name").code(), Some("bad-request"));
}
