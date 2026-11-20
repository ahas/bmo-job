//! Label values through the real server: a key's list published, read, paged and removed, and an item's labels
//! held to it as it is raised and as it changes (docs/api.md, "Label values").

use crate::support;

use std::collections::BTreeMap;

use bmo_job_client::{
  ItemChange, ItemQuery, JobClient, LabelValues, LabelValuesBody, LabelValuesQuery, NewItem,
};

use support::{START, Service};

fn listing(values: &[&str]) -> LabelValuesBody {
  LabelValuesBody {
    values: values.iter().map(|value| (*value).to_owned()).collect(),
  }
}

fn labels(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
  pairs.iter().map(|(key, value)| ((*key).to_owned(), (*value).to_owned())).collect()
}

fn raised(title: &str, pairs: &[(&str, &str)]) -> NewItem {
  NewItem {
    title: title.to_owned(),
    brief: String::new(),
    queue_name: "build".to_owned(),
    labels: labels(pairs),
    ..NewItem::default()
  }
}

fn relabelled(pairs: &[(&str, &str)]) -> ItemChange {
  ItemChange {
    labels: Some(labels(pairs)),
    ..ItemChange::default()
  }
}

/// The sentence a refusal names, where it is `label_value_unknown`.
fn unknown_value(failure: bmo_job_client::ClientFailure) -> String {
  assert_eq!(
    (failure.status(), failure.code()),
    (Some(422), Some("label_value_unknown")),
    "{failure}"
  );
  match failure {
    bmo_job_client::ClientFailure::Answered {
      answer: Some(answer),
      ..
    } => answer.error,
    other => panic!("no sentence: {other}"),
  }
}

async fn published(client: &JobClient, key: &str, values: &[&str]) -> LabelValues {
  client.put_label_values(key, &listing(values)).await.expect("published")
}

#[tokio::test]
async fn a_list_is_kept_tidied_read_paged_and_removed() {
  let service = Service::start().await;
  let client = service.client();

  let kept = published(&client, "model", &[" b", "a", "a", "", "c "]).await;
  let model = LabelValues {
    key: "model".to_owned(),
    values: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
    changed_at: START,
  };
  assert_eq!(kept, model);
  assert_eq!(client.label_values("model").await.expect("read"), model);

  service.clock.advance(3);
  published(&client, "area", &["parsing"]).await;
  let first = client
    .label_values_list(&LabelValuesQuery {
      limit: Some(1),
      ..LabelValuesQuery::default()
    })
    .await
    .expect("a page");
  assert_eq!((first.items[0].key.as_str(), first.more), ("area", true));
  let rest = client
    .label_values_list(&LabelValuesQuery {
      after: Some("area".to_owned()),
      ..LabelValuesQuery::default()
    })
    .await
    .expect("a page");
  assert_eq!((rest.items, rest.more), (vec![model], false));

  let unknown_field = service
    .raw(reqwest::Method::PUT, "/label-values/model")
    .json(&serde_json::json!({ "values": ["a"], "colour": "red" }))
    .send()
    .await
    .expect("answered");
  assert_eq!(unknown_field.status(), 400);

  client.delete_label_values("model").await.expect("removed");
  for missing in [client.label_values("model").await.map(drop), client.delete_label_values("model").await] {
    let missing = missing.expect_err("none for the key");
    assert_eq!((missing.status(), missing.code()), (Some(404), Some("not-found")));
  }
}

#[tokio::test]
async fn a_raise_takes_only_a_set_value_on_its_keys_list() {
  let service = Service::start().await;
  let client = service.client();
  published(&client, "model", &["a", "b", "c"]).await;
  published(&client, "area", &["parsing"]).await;

  let refused = client.create_item(&raised("off", &[("model", "x")])).await.expect_err("off the list");
  assert_eq!(unknown_value(refused), r#"The label model names "x", which is none of: a, b, c."#);
  let refused = client
    .create_item(&raised("both off", &[("model", "x"), ("area", "y")]))
    .await
    .expect_err("off the list");
  assert!(unknown_value(refused).starts_with("The label area "), "the first in key order");
  assert!(
    client.items(&ItemQuery::default()).await.expect("the list").items.is_empty(),
    "nothing raised"
  );

  for pairs in [
    &[("model", "a"), ("lane", "anything")][..],
    &[("model", " b ")][..],
    &[("model", "  ")][..],
  ] {
    client.create_item(&raised("on the list", pairs)).await.expect("taken");
  }
}

#[tokio::test]
async fn a_change_checks_only_a_value_new_or_changed_and_a_removed_list_checks_nothing() {
  let service = Service::start().await;
  let client = service.client();
  let item = client.create_item(&raised("older", &[("model", "old")])).await.expect("raised");
  published(&client, "model", &["a", "b"]).await;

  let retitled = ItemChange {
    title: Some("still older".to_owned()),
    ..ItemChange::default()
  };
  client.change_item(item.id, &retitled).await.expect("an unchanged value is not checked");
  client
    .change_item(item.id, &relabelled(&[("model", "old"), ("area", "parsing")]))
    .await
    .expect("an unchanged value is not checked");

  let refused = client
    .change_item(item.id, &relabelled(&[("model", "x")]))
    .await
    .expect_err("off the list");
  assert_eq!(unknown_value(refused), r#"The label model names "x", which is none of: a, b."#);
  assert_eq!(client.item(item.id).await.expect("read").labels["model"], "old", "nothing written");

  client.change_item(item.id, &relabelled(&[("model", "b")])).await.expect("on the list");

  client.delete_label_values("model").await.expect("removed");
  client
    .change_item(item.id, &relabelled(&[("model", "x")]))
    .await
    .expect("no list, no check");
}
