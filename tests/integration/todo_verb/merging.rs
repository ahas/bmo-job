//! Items merged into one from a shell, read from both sides, closed and reopened with the item carrying them,
//! and left alone by the verbs a merged item takes none of.

use serde_json::json;

use crate::support::todo::Bench;

// As many approved items as the test wants, each with an instruction of its own.
async fn rows(count: usize) -> (Bench, Vec<i64>) {
  let bench = Bench::new().await;
  let mut ids = vec![];

  for at in 1..=count {
    let title = format!("item {at}");

    ids.push(bench.item(&title, &format!("{title} instruction")).await);
  }

  (bench, ids)
}

async fn merged_into(bench: &Bench, id: i64) -> Option<i64> {
  bench.field_number(id, "merged_into").await
}

fn ids(listing: &serde_json::Value) -> Vec<i64> {
  listing.as_array().unwrap().iter().map(|line| line["id"].as_i64().unwrap()).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_merge_moves_the_items_into_the_target_and_the_listing_keeps_them_out_of_the_way_until_asked() {
  let (bench, ids_made) = rows(3).await;
  let [a, b, target] = ids_made[..] else { unreachable!() };
  let ran = bench.todo(&["--merge", &format!("{a},{b}"), "--into", &target.to_string()]).await;

  assert_eq!(
    ran.answer(),
    &json!({ "code": "items_merged", "ids": [a, b], "into": target, "title": "item 3" })
  );
  assert_eq!(merged_into(&bench, a).await, Some(target));

  let listed = ids(&bench.listed("").await);

  assert!(listed.contains(&target) && !listed.contains(&a), "{listed:?}");
  assert!(ids(&bench.listed("?merged").await).contains(&a));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reading_shows_the_merge_from_both_sides() {
  let (bench, ids_made) = rows(2).await;
  let [a, target] = ids_made[..] else { unreachable!() };

  bench.todo(&["--merge", &a.to_string(), "--into", &target.to_string()]).await.answer();

  let source = bench.shown(a).await;

  assert_eq!(source["merged_into"]["id"], target);
  assert_eq!(source["merged_into"]["title"], "item 2");

  let carrying = bench.shown(target).await;

  assert_eq!(carrying["merged_here"][0]["id"], a);
  // The merged row's own work is read with the carrying row, since one job does both.
  assert_eq!(carrying["merged_work"][0]["id"], a);
  assert!(
    carrying["merged_work"][0]["instruction"]
      .as_str()
      .unwrap()
      .contains("item 1 instruction")
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_merge_says_why_and_writes_nothing() {
  let (bench, ids_made) = rows(1).await;
  let a = ids_made[0];
  let ran = bench.todo(&["--merge", &a.to_string(), "--into", &a.to_string()]).await;

  assert_eq!((ran.code, ran.refused.as_str()), (1, "merge_itself"));
  assert_eq!(merged_into(&bench, a).await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn done_on_the_target_closes_what_is_merged_into_it_and_reopen_brings_it_back_merged() {
  let (bench, ids_made) = rows(2).await;
  let [a, target] = ids_made[..] else { unreachable!() };

  bench.todo(&["--merge", &a.to_string(), "--into", &target.to_string()]).await.answer();

  let done = bench.todo(&["--done", &target.to_string(), "--report", "The end."]).await;

  assert_eq!(done.answer()["code"], "item_closed");
  assert_eq!(done.answer()["carried"], json!([a]));
  assert!(bench.done(a).await);

  let reopened = bench.todo(&["--reopen", &target.to_string()]).await;

  assert_eq!(reopened.answer()["code"], "item_reopened");
  assert_eq!(reopened.answer()["merged_again"], json!([a]));
  assert!(!bench.done(a).await);
  assert_eq!(merged_into(&bench, a).await, Some(target));
}

#[tokio::test(flavor = "multi_thread")]
async fn hold_unhold_and_append_leave_a_merged_item_alone_and_unmerge_brings_it_back() {
  let (bench, ids_made) = rows(2).await;
  let [a, target] = ids_made[..] else { unreachable!() };
  let id = a.to_string();

  bench.todo(&["--merge", &id, "--into", &target.to_string()]).await.answer();

  assert_eq!(bench.todo(&["--hold", &id]).await.code, 1);
  assert_eq!(bench.todo(&["--unhold", &id]).await.code, 1);

  let appended = bench.todo(&["--append", &id, "--text", "one more"]).await;

  assert_eq!(appended.refused, "item_merged");
  assert!(appended.said.contains(&format!("merged into item {target}")), "{}", appended.said);

  let ran = bench.todo(&["--unmerge", &id]).await;

  assert_eq!(ran.answer()["code"], "item_unmerged");
  assert_eq!(merged_into(&bench, a).await, None);
  assert_eq!(bench.todo(&["--unmerge", &id]).await.refused, "not_merged");
}
