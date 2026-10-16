//! The rows (docs/rows.md, "The rows"): finishing with the items merged in, opening again, merging and its
//! refusals, a wind-down and a hold let go, throwing a request out, and the same statements on PostgreSQL.

mod support;

use queue_rows::store::{self, MergeRefusal};
use queue_rows::todos::{self, Raised, WIND_DOWN_NOTE_MARK};
use sea_orm::DatabaseConnection;
use support::{NOW, Scene, in_postgres};

#[tokio::test]
async fn an_item_finishes_with_the_items_merged_into_it_and_opens_again_with_them() {
  let scene = Scene::new().await;

  for (id, title) in [(1, "Target"), (2, "Merged"), (3, "Also merged")] {
    scene.item(id, title).await;
  }

  assert_eq!(store::merge(&scene.db, &[2, 3], 1, NOW).await.unwrap(), None);

  let carried = store::finish(&scene.db, 1, NOW, Some("Report."), Some("1. Agent share: a"), "Carried.")
    .await
    .unwrap();

  assert_eq!(carried, [2, 3]);
  assert_eq!(scene.column::<String>(2, "status").await, "done");
  assert_eq!(scene.column::<Option<String>>(2, "report").await.as_deref(), Some("Carried."));
  assert_eq!(
    scene.column::<Option<String>>(1, "unfinished").await.as_deref(),
    Some("1. Agent share: a")
  );
  assert_eq!(store::reopen(&scene.db, 1).await.unwrap(), [2, 3]);
  assert_eq!(scene.column::<String>(1, "status").await, "waiting");
  assert_eq!(scene.column::<Option<i64>>(2, "merged_into").await, Some(1));
  assert_eq!(scene.column::<i64>(3, "done").await, 0);
}

#[tokio::test]
async fn a_merge_is_refused_whole_for_any_item_it_cannot_take() {
  let scene = Scene::new().await;

  for id in 1..=4 {
    scene.item(id, "Item").await;
  }

  scene.run(r#"UPDATE "todos" SET "done" = 1 WHERE "id" = 4"#, vec![]).await;
  scene
    .run(r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "todo_id") VALUES (1, 'u', 1, 'p', 3)"#, vec![])
    .await;

  let refusals = [
    (vec![2], 9, MergeRefusal::Missing(9)),
    (vec![2], 4, MergeRefusal::Done(4)),
    (vec![1], 1, MergeRefusal::Itself(1)),
    (vec![2, 3], 1, MergeRefusal::Running(3)),
  ];

  for (ids, into, wanted) in refusals {
    assert_eq!(store::merge(&scene.db, &ids, into, NOW).await.unwrap(), Some(wanted));
  }

  assert_eq!(
    scene.column::<Option<i64>>(2, "merged_into").await,
    None,
    "a refused merge writes nothing"
  );
  assert!(!store::unmerge(&scene.db, 2).await.unwrap());
}

#[tokio::test]
async fn a_wind_down_holds_an_item_and_letting_go_withdraws_its_line() {
  let scene = Scene::new().await;

  scene.item(1, "Wound").await;
  scene.item(2, "Stopped").await;
  store::add_note(
    &scene.db,
    1,
    store::Author {
      name: "Agent",
      sender: None,
    },
    &format!("{WIND_DOWN_NOTE_MARK} Stop here."),
    None,
    NOW,
  )
  .await
  .unwrap();

  assert!(store::wind_down(&scene.db, 1, "Wait for #2.", Some(2), NOW).await.unwrap());
  assert_eq!(scene.column::<Option<String>>(1, "held_kind").await.as_deref(), Some("pause"));
  assert_eq!(store::resuming_after(&scene.db, 2).await.unwrap().len(), 1);
  assert!(todos::stop_hold(&scene.db, 2, "Enough.", NOW).await.unwrap());
  assert!(
    !store::wind_down(&scene.db, 2, "Later.", None, NOW).await.unwrap(),
    "a stopped item stays stopped"
  );
  assert!(todos::release(&scene.db, 1, NOW + 1).await.unwrap());
  assert!(!todos::release(&scene.db, 1, NOW + 2).await.unwrap());

  let notes = store::every_note(&scene.db, 1).await.unwrap();

  assert_eq!((notes[0].removed_at, notes[0].removed_by.as_deref()), (Some(NOW + 1), Some("resumed")));
}

#[tokio::test]
async fn a_request_thrown_out_leaves_its_merged_items_standing_on_their_own() {
  let scene = Scene::new().await;

  scene.item(1, "Thrown").await;
  scene.item(2, "Merged").await;
  store::merge(&scene.db, &[2], 1, NOW).await.unwrap();
  queue_rows::work_orders::save(&scene.db, &mut serde_json::json!({ "todoId": 1 }))
    .await
    .unwrap();
  todos::drop_item(&scene.db, 1).await.unwrap();

  assert!(store::row(&scene.db, 1).await.unwrap().is_none());
  assert!(queue_rows::work_orders::read(&scene.db, 1).await.unwrap().is_none());
  assert_eq!(scene.column::<Option<i64>>(2, "merged_into").await, None);
}

// The statements every backend must take alike: a filing with its sources, a finishing that compares the
// note, a hold let go that reads its line's head, and words taken back.
async fn every_backend_takes(scene: &Scene) {
  // Every row is numbered by the database here, as a filing numbers it.
  let filed = |title: &'static str, sources: &'static [i64]| Raised {
    title,
    instruction: "Why.",
    requester: "Agent",
    requester_id: None,
    chat_id: 5,
    created_at: NOW,
    sources,
  };
  let source = todos::add_raised(&scene.db, &filed("Source", &[])).await.unwrap();
  let raised = Raised {
    sources: &[source],
    ..filed("Raised", &[])
  };
  let id = todos::add_raised(&scene.db, &raised).await.unwrap();

  assert_eq!(todos::sources(&scene.db, id).await.unwrap(), [source]);
  assert_eq!(store::next_priority(&scene.db).await.unwrap(), 3);
  assert!(store::finish(&scene.db, source, NOW, None, None, "Carried.").await.unwrap().is_empty());
  assert_eq!(store::reopen(&scene.db, source).await.unwrap(), Vec::<i64>::new());
  assert!(todos::hold(&scene.db, id, Some("Wait."), NOW).await.unwrap());

  let note = store::add_note(
    &scene.db,
    id,
    store::Author {
      name: "Agent",
      sender: None,
    },
    &format!("{WIND_DOWN_NOTE_MARK} x"),
    None,
    NOW,
  )
  .await
  .unwrap();

  assert!(todos::release(&scene.db, id, NOW).await.unwrap());
  assert_eq!(
    store::remove_note(&scene.db, id, note, "Owner", NOW).await.unwrap(),
    None,
    "the release took it back"
  );
  assert!(todos::set_brief(&scene.db, id, "Brief.").await.unwrap());
  assert!(!todos::set_brief(&scene.db, id, "Brief.").await.unwrap());
}

#[tokio::test]
async fn a_sqlite_database_takes_the_rows_statements() {
  every_backend_takes(&Scene::new().await).await;
}

#[tokio::test]
async fn a_postgres_database_takes_the_rows_statements() {
  in_postgres("rows", |db: DatabaseConnection| async move {
    every_backend_takes(&Scene::on(db, None)).await
  })
  .await;
}
