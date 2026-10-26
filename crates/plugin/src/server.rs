//! The MCP server: the harness tools over one job service, each call made through the client, and the item
//! and queue resources its notifications name.

use std::sync::Arc;

use rmcp::model::{
  CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ExperimentalCapabilities,
  Implementation, InitializeResult, JsonObject, ListResourceTemplatesResult, ListToolsResult,
  PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
  ResourceContents, ResourceTemplate, ServerCapabilities, Tool,
};
use rmcp::service::{NotificationContext, RequestContext};
use rmcp::{ErrorData, RoleServer, ServerHandler};
use schemars::Schema;
use serde_json::{Map, Value};

use crate::harness::Harness;
use crate::openapi::generator;
use crate::tools::{JobTool, ToolFailure, tools};

/// What the server tells a harness of itself when it starts.
const INSTRUCTIONS: &str = "Keeps work as items in queues, and hands it out by leases. Subscribe to the queues you \
  serve with job_subscribe and you are told of each item opened there; take it, or the next of a queue, with \
  job_lease_next. Read the item, its notes and its work order with job_item, job_notes and job_work_order, report \
  with job_report, ask with job_question_ask and wait for the answer with job_replies, and close the item with \
  job_item_update setting its state to done. The server renews the leases and subscriptions you took while this \
  session lasts. A refusal comes back as a tool error carrying the service's status, code and sentence.";

/// The server over the job service the harness's client reaches.
pub struct JobServer {
  harness: Arc<Harness>,
  tools: Arc<Vec<JobTool>>,
}

impl JobServer {
  pub fn new(harness: Arc<Harness>) -> Self {
    Self {
      harness,
      tools: Arc::new(tools()),
    }
  }

  /// The server with the tools named left out: neither listed nor called.
  pub fn without(self, names: &[String]) -> Self {
    let tools = tools().into_iter().filter(|tool| !names.iter().any(|name| name == tool.name)).collect();

    Self {
      tools: Arc::new(tools),
      ..self
    }
  }

  /// The tools as MCP lists them, each input and output a schema of its own with its definitions inside.
  pub fn listed(&self) -> Vec<Tool> {
    self.tools.iter().map(listed).collect()
  }
}

fn listed(tool: &JobTool) -> Tool {
  let mut generator = generator("/$defs");
  let input = tool.input(&mut generator);
  let output = tool.output(&mut generator);
  let definitions = generator.take_definitions(true);
  let mut listed = Tool::new(tool.name, tool.description, standing_alone(input, &definitions));
  listed.output_schema = Some(Arc::new(standing_alone(output, &definitions)));
  listed
}

/// A schema made to stand on its own: an object at its root, with every definition it refers to beside it.
fn standing_alone(schema: Schema, definitions: &Map<String, Value>) -> JsonObject {
  let mut object = match schema.to_value() {
    Value::Object(object) if object.contains_key("$ref") => {
      let mut wrapped = Map::new();
      wrapped.insert("allOf".into(), Value::Array(vec![Value::Object(object)]));
      wrapped.insert("type".into(), "object".into());
      wrapped
    }
    Value::Object(object) => object,
    _ => unreachable!("every tool's input and output is an object"),
  };
  if !definitions.is_empty() {
    object.insert("$defs".into(), Value::Object(definitions.clone()));
  }
  object
}

/// A resource the notifications name, as its template's kind and key.
enum Named<'a> {
  Item(i64),
  Queue(&'a str),
}

fn named(uri: &str) -> Option<Named<'_>> {
  if let Some(key) = uri.strip_prefix("job://items/") {
    return key.parse().ok().map(Named::Item);
  }
  uri.strip_prefix("job://queues/").filter(|name| !name.is_empty()).map(Named::Queue)
}

impl ServerHandler for JobServer {
  fn get_info(&self) -> InitializeResult {
    let mut capabilities = ServerCapabilities::builder().enable_tools().enable_resources().build();
    if let Some(channel) = self.harness.channel() {
      let mut experimental = ExperimentalCapabilities::new();
      experimental.insert(channel.to_owned(), JsonObject::new());
      capabilities.experimental = Some(experimental);
    }
    let mut info = InitializeResult::new(capabilities);
    let mut server = Implementation::default();
    server.name = "bmo-job".to_owned();
    server.version = env!("CARGO_PKG_VERSION").to_owned();
    info.server_info = server;
    info.instructions = Some(INSTRUCTIONS.to_owned());
    info
  }

  async fn on_initialized(&self, context: NotificationContext<RoleServer>) {
    self.harness.reached(context.peer);
  }

  async fn list_tools(
    &self,
    _: Option<PaginatedRequestParams>,
    _: RequestContext<RoleServer>,
  ) -> Result<ListToolsResult, ErrorData> {
    Ok(ListToolsResult::with_all_items(self.listed()))
  }

  async fn call_tool(
    &self,
    request: CallToolRequestParams,
    _: RequestContext<RoleServer>,
  ) -> Result<CallToolResponse, ErrorData> {
    let Some(tool) = self.tools.iter().find(|tool| tool.name == request.name) else {
      return Err(ErrorData::invalid_params(format!("There is no tool {}.", request.name), None));
    };
    let arguments = Value::Object(request.arguments.unwrap_or_default());
    let result = match tool.call(Arc::clone(&self.harness), arguments).await {
      Ok(answer) => CallToolResult::structured(answer),
      Err(failure) => failed(&failure),
    };
    Ok(CallToolResponse::Complete(result))
  }

  async fn list_resource_templates(
    &self,
    _: Option<PaginatedRequestParams>,
    _: RequestContext<RoleServer>,
  ) -> Result<ListResourceTemplatesResult, ErrorData> {
    Ok(ListResourceTemplatesResult::with_all_items(vec![
      ResourceTemplate::new("job://items/{itemId}", "item")
        .with_description("One item, as job_item answers it.")
        .with_mime_type("application/json"),
      ResourceTemplate::new("job://queues/{queueName}", "queue")
        .with_description("One queue, its state, holds and counts.")
        .with_mime_type("application/json"),
    ]))
  }

  async fn read_resource(
    &self,
    request: ReadResourceRequestParams,
    _: RequestContext<RoleServer>,
  ) -> Result<ReadResourceResponse, ErrorData> {
    let client = &self.harness.client;
    let read = match named(&request.uri) {
      Some(Named::Item(id)) => client.item(id).await.map(|item| serde_json::to_string(&item)),
      Some(Named::Queue(name)) => client.queue(name).await.map(|queue| serde_json::to_string(&queue)),
      None => {
        return Err(ErrorData::resource_not_found(format!("There is no resource {}.", request.uri), None));
      }
    };
    match read {
      Ok(Ok(text)) => {
        let contents = ResourceContents::text(text, request.uri).with_mime_type("application/json");
        Ok(ReadResourceResult::new(vec![contents]).into())
      }
      Ok(Err(error)) => Err(ErrorData::internal_error(error.to_string(), None)),
      Err(failure) => Err(ErrorData::resource_not_found(failure.to_string(), None)),
    }
  }
}

/// A failed call as a tool error whose text is the failure's content. It carries no structured content, which
/// would be read against the tool's output schema.
fn failed(failure: &ToolFailure) -> CallToolResult {
  CallToolResult::error(vec![ContentBlock::text(failure.content().to_string())])
}
