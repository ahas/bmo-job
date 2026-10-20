//! The service's OpenAPI document whole: the client's routes and types, with the harness entry under
//! `x-harness` and the tools under `x-tools`. `docs/openapi.json` is this document as it is committed.

use schemars::generate::{SchemaGenerator, SchemaSettings};
use serde_json::{Value, json};

use crate::tools::tools;

/// A generator whose definitions stand at `definitions_path`.
pub(crate) fn generator(definitions_path: &str) -> SchemaGenerator {
  SchemaSettings::draft2020_12()
    .for_serialize()
    .with(|settings| {
      settings.definitions_path = definitions_path.to_owned().into();
      settings.meta_schema = None;
    })
    .into_generator()
}

/// The document as JSON.
pub fn openapi() -> Value {
  let mut generator = bmo_job_client::openapi::generator();
  let mut document = bmo_job_client::openapi::document(&mut generator);
  document.insert(
    "x-harness".into(),
    json!({
      "plugin": "bmo-job",
      "mcp": "bmo-job mcp",
      "entry": "A harness subscribes with job_subscribe to the queues it serves (or names them when it starts the \
        server), and is told of each item opened there by an MCP notification; it takes that item, or the next of a \
        queue, with job_lease_next, reads its brief, notes and work order, hears words appended while it works by \
        the same subscription or job_notes, reports with job_report and closes the item. The server renews the \
        leases and subscriptions it took while its harness runs, and lets them go when it stops.",
      "notifications": "The MCP server reads its subscriptions in the stream form and sends each event as an MCP \
        notification: notifications/resources/updated on the resource job://items/{itemId} or \
        job://queues/{queueName} the event names, and, where it was started with a channel, the event itself as \
        notifications/<channel> carrying content and meta, for a harness that takes messages pushed into its \
        session.",
    }),
  );
  let listed: Vec<Value> = tools()
    .iter()
    .map(|tool| {
      json!({
        "name": tool.name,
        "description": tool.description,
        "input": tool.input(&mut generator).to_value(),
        "output": tool.output(&mut generator).to_value(),
        "route": tool.route,
      })
    })
    .collect();
  document.insert("x-tools".into(), Value::Array(listed));
  bmo_job_client::openapi::with_definitions(&mut generator, &mut document);
  Value::Object(document)
}

/// The document as the committed file holds it: pretty-printed, ending in a newline.
pub fn openapi_json() -> String {
  let mut text = serde_json::to_string_pretty(&openapi()).expect("the document renders as JSON");
  text.push('\n');
  text
}
