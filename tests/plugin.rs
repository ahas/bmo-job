//! The harness plugin as a harness meets it: `bmo-job mcp --service <url>` starts an MCP server on standard
//! input and output that lists exactly the draft's tools, calls the service through the client, tells the
//! harness of each event of its subscriptions, and lets go of what it kept when the session ends (docs/api.md,
//! "The harness plugin"). An approval it gives is its token's caller's and carries no name the harness typed.
//! The command runs as its own process, spoken to over its pipes, and is taken down before each test ends.

use crate::support;

use std::collections::HashMap;
use std::process::Stdio;
use std::time::Duration;

use axum::Router;
use axum::body::Bytes;
use axum::routing::patch;
use bmo_job_client::{Item, ItemState, SubscriptionQuery};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::net::TcpListener;
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::mpsc;

use support::{APPROVER, APPROVER_TOKEN, CALLER, CALLER_TOKEN, Service, TOKEN};

/// How long a test lets the server take to answer before it fails; a guard, never a wait.
const PATIENCE: Duration = Duration::from_secs(30);

/// The command, started with these arguments, spoken to line by line.
struct Server {
  child: Child,
  input: Option<ChildStdin>,
  output: Lines<BufReader<ChildStdout>>,
  next: u64,
  /// Notifications read while waiting for an answer.
  heard: Vec<Value>,
  /// Each tool's output schema as the server listed it, by the tool's name.
  outputs: HashMap<String, Value>,
}

impl Server {
  async fn initialized(service: &Service, extra: &[&str]) -> Self {
    Self::started(&service.url, TOKEN, extra).await
  }

  /// The command reaching the service at `url` with `token`.
  async fn started(url: &str, token: &str, extra: &[&str]) -> Self {
    let mut command = Command::new(env!("CARGO_BIN_EXE_bmo-job"));
    command
      .args(["mcp", "--service", url])
      .args(extra)
      .env("BMO_JOB_TOKEN", token)
      .stdin(Stdio::piped())
      .stdout(Stdio::piped())
      .stderr(Stdio::null())
      .kill_on_drop(true);
    let mut child = command.spawn().expect("the command starts");
    let input = child.stdin.take().expect("the command's input is piped");
    let output = BufReader::new(child.stdout.take().expect("the command's output is piped")).lines();
    let mut server = Self {
      child,
      input: Some(input),
      output,
      next: 1,
      heard: Vec::new(),
      outputs: HashMap::new(),
    };
    let info = server
      .ask(
        "initialize",
        json!({
          "protocolVersion": "2025-11-25",
          "capabilities": {},
          "clientInfo": { "name": "tester", "version": "1.0.0" },
        }),
      )
      .await;
    assert_eq!(info["serverInfo"]["name"], "bmo-job");
    assert_eq!(info["serverInfo"]["version"], env!("CARGO_PKG_VERSION"));
    assert!(info["capabilities"]["tools"].is_object(), "the server serves tools: {info}");
    server.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" })).await;
    let listed = server.ask("tools/list", json!({})).await;
    for tool in listed["tools"].as_array().expect("a list of tools") {
      let name = tool["name"].as_str().expect("a name").to_owned();
      server.outputs.insert(name, tool["outputSchema"].clone());
    }
    server
  }

  async fn send(&mut self, message: Value) {
    let mut line = message.to_string();
    line.push('\n');
    let input = self.input.as_mut().expect("the session is open");
    input.write_all(line.as_bytes()).await.expect("the server reads its input");
    input.flush().await.expect("the server reads its input");
  }

  async fn line(&mut self) -> Value {
    let line = tokio::time::timeout(PATIENCE, self.output.next_line())
      .await
      .expect("the server writes in time")
      .expect("the server's output reads")
      .expect("the server writes before it closes its output");
    serde_json::from_str(&line).expect("the server writes JSON lines")
  }

  /// One request, answered with the result of the answer bearing its id.
  async fn ask(&mut self, method: &str, params: Value) -> Value {
    let id = self.next;
    self.next += 1;
    self
      .send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
      .await;
    loop {
      let message = self.line().await;
      if message["id"] == id {
        assert!(message["error"].is_null(), "{method} is answered without a protocol error: {message}");
        return message["result"].clone();
      }
      if message["id"].is_null() {
        self.heard.push(message);
      }
    }
  }

  /// One tool's call; an answer that is no tool error is held to the output schema the tool was listed with, as
  /// a harness holds it.
  async fn call(&mut self, tool: &str, arguments: Value) -> Value {
    let result = self.ask("tools/call", json!({ "name": tool, "arguments": arguments })).await;
    if result["isError"] != true {
      let schema = self.outputs.get(tool).expect("the tool was listed");
      let validator = jsonschema::validator_for(schema).expect("the output schema compiles");
      let answer = &result["structuredContent"];
      let faults: Vec<String> = validator.iter_errors(answer).map(|fault| fault.to_string()).collect();
      assert!(faults.is_empty(), "{tool} answers outside its output schema: {faults:?}\n{answer}");
    }
    result
  }

  /// The next notification of `method` matching `wanted`, read from what was heard and what comes.
  async fn heard(&mut self, method: &str, wanted: impl Fn(&Value) -> bool) -> Value {
    if let Some(place) = self
      .heard
      .iter()
      .position(|message| message["method"] == method && wanted(&message["params"]))
    {
      return self.heard.remove(place);
    }
    loop {
      let message = self.line().await;
      if message["method"] == method && wanted(&message["params"]) {
        return message;
      }
    }
  }

  /// Ends the session as a harness does, by closing the server's input, and waits for the server to exit.
  async fn end(mut self) {
    drop(self.input.take());
    let exited = tokio::time::timeout(PATIENCE, self.child.wait()).await;
    if exited.is_err() {
      self.child.kill().await.ok();
      panic!("the server did not exit when its session ended");
    }
  }
}

#[tokio::test]
async fn the_server_lists_exactly_the_drafts_tools() {
  let service = Service::start().await;
  let mut server = Server::initialized(&service, &[]).await;
  let listed = server.ask("tools/list", json!({})).await;
  let tools = listed["tools"].as_array().expect("a list of tools");
  let names: Vec<&str> = tools.iter().map(|tool| tool["name"].as_str().expect("a name")).collect();
  let documented: Vec<String> = bmo_job_plugin::openapi()["x-tools"]
    .as_array()
    .expect("the document's tools")
    .iter()
    .map(|tool| tool["name"].as_str().expect("a name").to_owned())
    .collect();
  assert_eq!(names, documented);
  assert_eq!(names.len(), 19);
  for tool in tools {
    assert_eq!(tool["inputSchema"]["type"], "object", "{} takes an object", tool["name"]);
    assert_eq!(tool["outputSchema"]["type"], "object", "{} answers an object", tool["name"]);
  }
  let templates = server.ask("resources/templates/list", json!({})).await;
  let uris: Vec<&str> = templates["resourceTemplates"]
    .as_array()
    .expect("templates")
    .iter()
    .map(|t| t["uriTemplate"].as_str().expect("a uri"))
    .collect();
  assert_eq!(uris, ["job://items/{itemId}", "job://queues/{queueName}"]);
  server.end().await;
}

#[tokio::test]
async fn a_tool_calls_its_route_and_a_refusal_is_a_tool_error() {
  let service = Service::start().await;
  let mut server = Server::started(&service.url, APPROVER_TOKEN, &[]).await;
  let created = server
    .call(
      "job_item_create",
      json!({ "title": "parser", "brief": "write it", "queueName": "build" }),
    )
    .await;
  let item_id = created["structuredContent"]["id"].as_i64().expect("the item's key");
  assert_eq!(created["structuredContent"]["state"], "draft");
  let approved = server.call("job_item_update", json!({ "itemId": item_id, "approval": {} })).await;
  assert_eq!(approved["structuredContent"]["state"], "open");
  assert_eq!(approved["structuredContent"]["approvals"][0]["callerName"], APPROVER);

  let taken = server
    .call(
      "job_lease_next",
      json!({ "queueName": "build", "holderName": "harness", "ttlMs": 60000 }),
    )
    .await;
  assert_eq!(taken["structuredContent"]["lease"]["itemId"], item_id);
  let none = server
    .call(
      "job_lease_next",
      json!({ "queueName": "build", "holderName": "harness", "ttlMs": 60000 }),
    )
    .await;
  assert_eq!(none["structuredContent"], json!({ "lease": null }));

  let missing = server.call("job_item", json!({ "itemId": 9999 })).await;
  assert_eq!(missing["isError"], true);
  let content: Value =
    serde_json::from_str(missing["content"][0]["text"].as_str().expect("a text")).expect("JSON");
  assert_eq!((content["status"].as_i64(), content["code"].as_str()), (Some(404), Some("not-found")));
  let malformed = server.call("job_item", json!({ "itemId": "seven" })).await;
  assert_eq!(malformed["isError"], true);

  let read = server.ask("resources/read", json!({ "uri": format!("job://items/{item_id}") })).await;
  let item: Value =
    serde_json::from_str(read["contents"][0]["text"].as_str().expect("a text")).expect("JSON");
  assert_eq!(item["state"], "leased");
  server.end().await;
}

#[tokio::test]
async fn the_server_tells_the_harness_of_each_event_of_its_queues() {
  let service = Service::start().await;
  let mut server =
    Server::initialized(&service, &["--holder", "harness", "--queue", "build", "--channel", "test/push"])
      .await;
  let item = service.open_item("build", "told").await;
  let uri = format!("job://items/{}", item.id);
  server
    .heard("notifications/resources/updated", |params| params["uri"] == uri.as_str())
    .await;
  server
    .heard("notifications/resources/updated", |params| params["uri"] == "job://queues/build")
    .await;
  let pushed = server
    .heard("notifications/test/push", |params| params["meta"]["kind"] == "item.changed")
    .await;
  assert_eq!(pushed["params"]["meta"]["itemId"], item.id.to_string());
  let event: Value =
    serde_json::from_str(pushed["params"]["content"].as_str().expect("a text")).expect("JSON");
  assert_eq!(event["data"]["state"], "open");
  server.end().await;
}

#[tokio::test]
async fn the_server_lets_go_of_what_it_kept_when_the_session_ends() {
  let service = Service::start().await;
  let client = service.client();
  let item = service.open_item("build", "kept").await;
  let mut server = Server::initialized(&service, &[]).await;
  server
    .call(
      "job_lease_next",
      json!({ "queueName": "build", "holderName": "harness", "ttlMs": 60000 }),
    )
    .await;
  let subscribed = server
    .call("job_subscribe", json!({ "holderName": "harness", "queueNames": ["build"] }))
    .await;
  assert!(subscribed["structuredContent"]["id"].is_i64(), "subscribed: {subscribed}");
  assert_eq!(client.item(item.id).await.expect("read").state, ItemState::Leased);

  server.end().await;
  assert_eq!(client.item(item.id).await.expect("read").state, ItemState::Open);
  assert!(
    client
      .subscriptions(&SubscriptionQuery::default())
      .await
      .expect("listed")
      .items
      .is_empty()
  );
}

#[tokio::test]
async fn the_update_tool_takes_no_name_of_who_approves() {
  let service = Service::start().await;
  let mut server = Server::initialized(&service, &[]).await;
  let listed = server.ask("tools/list", json!({})).await;
  let update = listed["tools"]
    .as_array()
    .expect("a list of tools")
    .iter()
    .find(|tool| tool["name"] == "job_item_update")
    .expect("the update tool is listed")
    .clone();
  let input = update["inputSchema"].to_string();
  assert!(input.contains("\"approval\""), "the tool takes an approval: {input}");
  assert!(!input.contains("actorName"), "the tool takes no actor's name: {input}");
  let description = update["description"].as_str().expect("a description");
  assert!(description.contains("recommendation"), "{description}");
  let documented = bmo_job_plugin::openapi()["x-tools"]
    .as_array()
    .expect("the document's tools")
    .iter()
    .find(|tool| tool["name"] == "job_item_update")
    .expect("the update tool is documented")["input"]
    .to_string();
  assert!(
    !documented.contains("actorName"),
    "the documented tool takes no actor's name: {documented}"
  );
  server.end().await;
}

#[tokio::test]
async fn an_approval_through_the_tool_is_the_callers_recommendation_whatever_name_is_typed() {
  let service = Service::start().await;
  let item = service
    .client()
    .create_item(&bmo_job_client::NewItem {
      title: "recommended".to_owned(),
      queue_name: "build".to_owned(),
      ..bmo_job_client::NewItem::default()
    })
    .await
    .expect("raised");
  let mut server = Server::started(&service.url, CALLER_TOKEN, &[]).await;
  let answered = server
    .call("job_item_update", json!({ "itemId": item.id, "approval": { "actorName": APPROVER } }))
    .await;
  let recommended = &answered["structuredContent"];
  assert_eq!(recommended["state"], "draft", "{answered}");
  assert_eq!(recommended["approvals"][0]["kind"], "recommendation");
  assert_eq!(recommended["approvals"][0]["callerName"], CALLER);
  assert_eq!(service.client().item(item.id).await.expect("read").state, ItemState::Draft);
  server.end().await;
}

/// A service that records each item change it is sent and answers a draft.
async fn recorder() -> (String, mpsc::UnboundedReceiver<Value>, tokio::task::JoinHandle<()>) {
  let (sent, bodies) = mpsc::unbounded_channel();
  let app = Router::new().route(
    "/items/{itemId}",
    patch(move |body: Bytes| {
      let sent = sent.clone();
      async move {
        let _ = sent.send(serde_json::from_slice::<Value>(&body).expect("a JSON body"));
        let draft = Item {
          id: 7,
          title: "recorded".to_owned(),
          brief: String::new(),
          state: ItemState::Draft,
          queue_name: "build".to_owned(),
          position: 1,
          priority: 0,
          assignee_name: None,
          requester_name: None,
          scope: None,
          labels: Default::default(),
          approvals: Vec::new(),
          rejections: 0,
          merged_into: None,
          raised_by: None,
          lease_id: None,
          created_at: 0,
          changed_at: 0,
          ended_at: None,
        };
        axum::Json(draft)
      }
    }),
  );
  let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
  let url = format!("http://{}", listener.local_addr().expect("an address"));
  let task = tokio::spawn(async move {
    let _ = axum::serve(listener, app).await;
  });
  (url, bodies, task)
}

#[tokio::test]
async fn the_update_tool_sends_no_name_of_who_approves() {
  let (url, mut bodies, task) = recorder().await;
  let mut server = Server::started(&url, TOKEN, &[]).await;
  let answered = server
    .call(
      "job_item_update",
      json!({ "itemId": 7, "approval": { "actorName": "somebody", "at": 5 } }),
    )
    .await;
  assert_ne!(answered["isError"], true, "{answered}");
  let sent = tokio::time::timeout(PATIENCE, bodies.recv())
    .await
    .expect("the change is sent in time")
    .expect("a body");
  assert_eq!(sent, json!({ "approval": { "at": 5 } }));
  server.end().await;
  task.abort();
}

#[tokio::test]
async fn a_tool_left_out_at_the_start_is_neither_listed_nor_called() {
  let service = Service::start().await;
  let mut server = Server::initialized(&service, &["--without", "job_item_create,job_queues"]).await;
  let listed = server.ask("tools/list", json!({})).await;
  let names: Vec<&str> = listed["tools"]
    .as_array()
    .expect("a list of tools")
    .iter()
    .map(|tool| tool["name"].as_str().unwrap())
    .collect();

  assert_eq!(names.len(), 17);
  assert!(!names.contains(&"job_item_create") && !names.contains(&"job_queues"));
  assert!(names.contains(&"job_item_update"));

  let id = server.next;

  server.next += 1;
  server
    .send(json!({
      "jsonrpc": "2.0",
      "id": id,
      "method": "tools/call",
      "params": { "name": "job_item_create", "arguments": { "title": "t", "brief": "b", "queueName": "build" } },
    }))
    .await;

  let called = loop {
    let message = server.line().await;

    if message["id"] == id {
      break message;
    }
  };

  assert_eq!(called["error"]["message"], "There is no tool job_item_create.", "{called}");
  assert!(
    service.client().items(&Default::default()).await.expect("the items").items.is_empty(),
    "nothing raised"
  );
  server.end().await;
}

#[tokio::test]
async fn a_name_that_is_no_tool_refuses_the_start() {
  let service = Service::start().await;
  let ran = Command::new(env!("CARGO_BIN_EXE_bmo-job"))
    .args(["mcp", "--service", &service.url, "--without", "job_item_create,job_nothing"])
    .env("BMO_JOB_TOKEN", TOKEN)
    .stdin(Stdio::null())
    .output()
    .await
    .expect("the command runs");

  assert!(!ran.status.success());
  assert!(
    String::from_utf8_lossy(&ran.stderr).contains("job_nothing"),
    "{}",
    String::from_utf8_lossy(&ran.stderr)
  );
}
