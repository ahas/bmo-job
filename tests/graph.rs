//! The graph port's contract: its call is made on the method and path the API document gives it, and its
//! answer is read as the call's shape (docs/api.md, "bmo-graph").

mod support;

use job_graph::{Graph, GraphClient, GraphError, Named};
use serde_json::json;
use support::graph::Stub;
use url::Url;

#[tokio::test]
async fn named_reads_where_the_graph_stands_and_every_file_it_names() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /graph/named",
    200,
    json!({ "directory": ".graph", "files": ["src/lib.rs", "crates/base/src/settings.rs"] }),
  );

  let named = stub.client().named("/work/checkout").await.unwrap();

  assert_eq!(
    named,
    Named {
      directory: ".graph".into(),
      files: vec!["src/lib.rs".into(), "crates/base/src/settings.rs".into()],
    }
  );
  stub.took("GET /graph/named", "/graph/named?checkout=%2Fwork%2Fcheckout", None);
}

#[tokio::test]
async fn a_checkout_that_keeps_no_graph_names_no_file() {
  let stub = Stub::start().await;

  stub.answer("GET /graph/named", 200, json!({ "directory": ".graph", "files": [] }));

  assert!(stub.client().named("/work/checkout").await.unwrap().files.is_empty());
}

#[tokio::test]
async fn a_checkout_path_is_encoded_in_the_query() {
  let stub = Stub::start().await;

  stub.answer("GET /graph/named", 200, json!({ "directory": ".graph", "files": [] }));

  stub.client().named("/work/a b&c=d").await.unwrap();

  assert_eq!(stub.calls()[0].target, "/graph/named?checkout=%2Fwork%2Fa+b%26c%3Dd");
}

#[tokio::test]
async fn a_refusal_is_handed_on_with_its_status_code_and_sentence() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /graph/named",
    422,
    json!({ "error": "The graph's area server.json does not read.", "code": "graph_unreadable" }),
  );

  let refused = stub.client().named("/work/checkout").await.unwrap_err();

  assert!(
    matches!(
      &refused,
      GraphError::Refused { call: "named", status: 422, code, error }
        if code == "graph_unreadable" && error == "The graph's area server.json does not read."
    ),
    "{refused}"
  );
}

#[tokio::test]
async fn a_checkout_that_is_not_found_is_refused_not_read_as_nothing() {
  let stub = Stub::start().await;

  stub.answer(
    "GET /graph/named",
    404,
    json!({ "error": "No directory stands at /work/gone.", "code": "checkout_missing" }),
  );

  let refused = stub.client().named("/work/gone").await.unwrap_err();

  assert!(
    matches!(&refused, GraphError::Refused { status: 404, code, .. } if code == "checkout_missing"),
    "{refused}"
  );
}

#[tokio::test]
async fn an_answer_that_does_not_read_is_malformed() {
  for (status, body) in [
    (200, json!({ "files": ["src/lib.rs"] })),
    (200, json!({ "directory": ".graph", "files": "src/lib.rs" })),
    (502, json!("bad gateway")),
  ] {
    let stub = Stub::start().await;

    stub.answer("GET /graph/named", status, body.clone());

    let failed = stub.client().named("/work/checkout").await.unwrap_err();

    assert!(
      matches!(failed, GraphError::Malformed { call: "named", status: given, .. } if given == status),
      "{body}: {failed}"
    );
  }
}

#[tokio::test]
async fn a_call_that_reaches_nothing_is_unreachable() {
  let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
  let address = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();

  drop(listener);

  let failed = GraphClient::new(address).unwrap().named("/work/checkout").await.unwrap_err();

  assert!(matches!(failed, GraphError::Unreachable { call: "named", .. }), "{failed}");
}

#[test]
fn an_address_that_is_not_http_is_refused() {
  for address in ["https://graph.invalid/", "mailto:graph@graph.invalid", "file:///graph"] {
    let refused = GraphClient::new(Url::parse(address).unwrap()).unwrap_err();

    assert!(matches!(refused, GraphError::NotAnAddress { .. }), "{address}: {refused}");
  }
}
