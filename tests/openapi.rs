//! The contract: the committed OpenAPI document is what the code generates, its tools are the draft's, every
//! route it states is served, and it names nothing of the units that call it (docs/api.md).

use crate::support;

use std::fs;
use std::path::Path;

use bmo_job_client::{Code, ErrorBody, routes};
use regex::RegexBuilder;
use serde_json::Value;

use support::{ROOT, Service};

/// The draft's tools, in its order.
const DRAFT_TOOLS: [&str; 22] = [
  "job_items",
  "job_item_create",
  "job_item",
  "job_item_update",
  "job_items_relabel",
  "job_notes",
  "job_note_add",
  "job_report",
  "job_reviews",
  "job_work_order",
  "job_tasks",
  "job_queues",
  "job_lease_next",
  "job_lease_renew",
  "job_lease_release",
  "job_question_ask",
  "job_question",
  "job_replies",
  "job_reply",
  "job_question_change",
  "job_subscribe",
  "job_unsubscribe",
];

fn document() -> Value {
  bmo_job_plugin::openapi()
}

#[test]
fn the_committed_document_is_the_generated_one() {
  let committed =
    fs::read_to_string(Path::new(ROOT).join("docs/openapi.json")).expect("docs/openapi.json is readable");
  assert!(
    committed == bmo_job_plugin::openapi_json(),
    "docs/openapi.json differs from what the code generates; write it again with `bmo-job openapi`"
  );
}

#[test]
fn the_document_carries_the_placeholder_version() {
  assert_eq!(document()["info"]["version"], env!("CARGO_PKG_VERSION"));
}

#[test]
fn the_tools_are_the_drafts_each_calling_a_stated_route() {
  let document = document();
  let tools = document["x-tools"].as_array().expect("the tools are a list");
  let names: Vec<&str> = tools.iter().map(|tool| tool["name"].as_str().expect("a tool's name")).collect();
  assert_eq!(names, DRAFT_TOOLS);
  let stated: Vec<String> = routes().iter().map(|route| route.named()).collect();
  for tool in tools {
    let route = tool["route"].as_str().expect("a tool's route");
    assert!(stated.iter().any(|named| named == route), "{route} is not a stated route");
    assert_eq!(tool["input"]["type"], "object", "{} takes an object", tool["name"]);
  }
  assert_eq!(document["x-harness"]["plugin"], "bmo-job");
  assert_eq!(document["x-harness"]["mcp"], "bmo-job mcp");
}

#[test]
fn every_operation_lists_its_codes_and_answers_refusals_in_the_error_shape() {
  let document = document();
  for route in routes() {
    let operation = &document["paths"][route.path][route.method.key()];
    assert_eq!(operation["operationId"], route.operation_id);
    assert_eq!(operation["x-codes"][0], "unauthorized", "{} lists unauthorized first", route.named());
    assert_eq!(operation["responses"]["default"]["$ref"], "#/components/responses/Error");
  }
  assert_eq!(
    document["components"]["schemas"]["Error"]["required"],
    serde_json::json!(["code", "error"])
  );
}

#[tokio::test]
async fn every_stated_route_is_served() {
  let service = Service::start().await;
  let key = regex::Regex::new(r"\{\w+\}").expect("the key pattern builds");
  for route in routes() {
    let path = route.path.replace("{queueName}", "inbox").replace("{scope}", "somewhere");
    let path = key.replace_all(&path, "1").into_owned();
    let method = reqwest::Method::from_bytes(route.method.as_str().as_bytes()).expect("a method");
    let mut request = service.raw(method, &path).header("Accept", "application/json");
    if route.body.is_some() {
      request = request.json(&serde_json::json!({}));
    }
    let answer = request.send().await.expect("the service answers");
    if answer.status() == 404 {
      let body: ErrorBody = answer.json().await.expect("a refusal in the error shape");
      assert_ne!(body.code, Code::NoRoute.as_str(), "{} is not served", route.named());
    }
  }
}

#[test]
fn the_contract_names_nothing_of_its_callers() {
  let words = ["agent", "agency", "worker", "room", "owner", "persona", "receiver", "console"];
  let mut text = serde_json::to_string(&document()).expect("the document renders");
  text.push_str(
    &fs::read_to_string(Path::new(ROOT).join("crates/plugin/skills/bmo-job/SKILL.md"))
      .expect("the skill is readable"),
  );
  for word in words {
    let pattern = RegexBuilder::new(&format!(r"\b{word}s?\b"))
      .case_insensitive(true)
      .build()
      .expect("a word's pattern");
    assert!(!pattern.is_match(&text), "the contract names {word:?}");
  }
}
