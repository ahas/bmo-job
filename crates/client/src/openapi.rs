//! The OpenAPI 3.1 document, generated from the client's own types and the route table. The harness plugin
//! adds its tools and its entry to it, so the committed `docs/openapi.json` is made by the plugin's
//! `openapi_json` (docs/api.md).

use schemars::Schema;
use schemars::generate::{SchemaGenerator, SchemaSettings};
use serde_json::{Map, Value, json};

use crate::events::{Event, EventKind};
use crate::routes::{Route, path_parameter_schema, routes};

/// A generator whose definitions are the document's component schemas. It reads each type as it is written:
/// a field is required where it is always written, and an input's field the caller may leave out is one the
/// client leaves out when it is empty.
pub fn generator() -> SchemaGenerator {
  SchemaSettings::draft2020_12()
    .for_serialize()
    .with(|settings| {
      settings.definitions_path = "/components/schemas".into();
      settings.meta_schema = None;
    })
    .into_generator()
}

/// The document, short of its component schemas, which [`with_definitions`] adds once everything that
/// refers to them has been generated.
pub fn document(generator: &mut SchemaGenerator) -> Map<String, Value> {
  let mut paths = Map::new();
  for route in routes() {
    let operation = operation(&route, generator);
    let entry = paths.entry(route.path.to_owned()).or_insert_with(|| Value::Object(Map::new()));
    entry
      .as_object_mut()
      .expect("a path's entry is an object")
      .insert(route.method.key().to_owned(), operation);
  }

  let event = generator.subschema_for::<Event>().to_value();
  let mut document = Map::new();
  document.insert("openapi".into(), json!("3.1.0"));
  document.insert(
    "info".into(),
    json!({
      "title": "bmo-job",
      "version": env!("CARGO_PKG_VERSION"),
      "description": "A todo kept as middleware: items, notes, reports, work orders and their tasks, queues, leases \
        on items, questions and their replies, and subscriptions, stored and served through this API and nothing \
        else. The service never executes: it launches no run and calls no other service; the only calls it makes \
        are deliveries to the addresses its subscribers registered. Work reaches whoever does it two ways, side by \
        side: pushed, as the events of a subscription it registered, and pulled, by a lease. Who a member belongs \
        to is opaque to the service: scope and the <role>Name fields are the caller's strings, and labels the \
        caller's metadata.",
    }),
  );
  document.insert("servers".into(), json!([{ "url": "/", "description": "the job service" }]));
  document.insert("security".into(), json!([{ "bearer": [] }]));
  document.insert("tags".into(), tags());
  document.insert("paths".into(), Value::Object(paths));
  document.insert(
    "webhooks".into(),
    json!({
      "subscription-event": {
        "post": {
          "summary": "The service tells a subscriber of one event it subscribed to, at least once and in event order.",
          "description": "Posted to the url the subscriber registered, with the subscription's key in the \
            Job-Subscription header and, where a secret was given, sha256= and the lowercase hex HMAC-SHA256 of the \
            body under it in Job-Signature. A 2xx acknowledges it; anything else is tried again with backoff, and \
            ten failures in a row move the subscription to failing. The subscriber drops an event it has seen by \
            its id.",
          "requestBody": { "required": true, "content": { "application/json": { "schema": event } } },
          "responses": { "204": { "description": "acknowledged" } },
        }
      }
    }),
  );
  document.insert("x-calls".into(), json!([]));
  document.insert(
    "x-events".into(),
    json!(EventKind::ALL.iter().map(|kind| kind.as_str()).collect::<Vec<_>>()),
  );
  document
}

/// Adds the component schemas every part of the document refers to, the error shape and the bearer scheme.
pub fn with_definitions(generator: &mut SchemaGenerator, document: &mut Map<String, Value>) {
  let error = generator.subschema_for::<crate::codes::ErrorBody>().to_value();
  let schemas = generator.take_definitions(true);
  document.insert(
    "components".into(),
    json!({
      "securitySchemes": { "bearer": { "type": "http", "scheme": "bearer" } },
      "responses": {
        "Error": {
          "description": "a failure, in the common error shape",
          "content": { "application/json": { "schema": error } },
        }
      },
      "schemas": schemas,
    }),
  );
}

fn tags() -> Value {
  let mut tags: Vec<&str> = Vec::new();
  for route in routes() {
    if !tags.contains(&route.tag) {
      tags.push(route.tag);
    }
  }
  Value::Array(tags.into_iter().map(|tag| json!({ "name": tag })).collect())
}

fn operation(route: &Route, generator: &mut SchemaGenerator) -> Value {
  let mut parameters = Vec::new();
  for name in path_parameters(route.path) {
    parameters.push(json!({
      "name": name,
      "in": "path",
      "required": true,
      "schema": path_parameter_schema(&name),
    }));
  }
  if let Some(query) = route.query {
    parameters.extend(query_parameters(query(generator)));
  }

  let mut responses = Map::new();
  for answer in &route.answers {
    let mut response = Map::new();
    response.insert("description".into(), json!(answer.description));
    if !answer.content.is_empty() {
      let content: Map<String, Value> = answer
        .content
        .iter()
        .map(|(media, schema)| ((*media).to_owned(), json!({ "schema": schema(generator).to_value() })))
        .collect();
      response.insert("content".into(), Value::Object(content));
    }
    responses.insert(answer.status.to_string(), Value::Object(response));
  }
  responses.insert("default".into(), json!({ "$ref": "#/components/responses/Error" }));

  let mut operation = Map::new();
  operation.insert("operationId".into(), json!(route.operation_id));
  operation.insert("tags".into(), json!([route.tag]));
  operation.insert("summary".into(), json!(route.summary));
  if !parameters.is_empty() {
    operation.insert("parameters".into(), Value::Array(parameters));
  }
  if let Some(body) = route.body {
    operation.insert(
      "requestBody".into(),
      json!({ "required": true, "content": { "application/json": { "schema": body(generator).to_value() } } }),
    );
  }
  operation.insert("responses".into(), Value::Object(responses));
  operation.insert(
    "x-codes".into(),
    json!(route.codes.iter().map(|code| code.as_str()).collect::<Vec<_>>()),
  );
  Value::Object(operation)
}

/// The names a path template carries in braces, in order.
pub(crate) fn path_parameters(path: &str) -> Vec<String> {
  path
    .split('/')
    .filter_map(|segment| segment.strip_prefix('{').and_then(|rest| rest.strip_suffix('}')))
    .map(str::to_owned)
    .collect()
}

/// A query object's properties as query parameters, each taking its property's description, and a nullable
/// type written as the type alone, since a query has no null.
fn query_parameters(query: Schema) -> Vec<Value> {
  let query = query.to_value();
  let required: Vec<&str> = query
    .get("required")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .collect();
  let Some(properties) = query.get("properties").and_then(Value::as_object) else {
    return Vec::new();
  };
  properties
    .iter()
    .map(|(name, schema)| {
      let mut schema = schema.as_object().cloned().unwrap_or_default();
      let description = schema.remove("description");
      if let Some(Value::Array(types)) = schema.get("type") {
        let types: Vec<Value> =
          types.iter().filter(|kind| kind.as_str() != Some("null")).cloned().collect();
        let single = if types.len() == 1 { types[0].clone() } else { Value::Array(types) };
        schema.insert("type".into(), single);
      }
      let mut parameter = Map::new();
      parameter.insert("name".into(), json!(name));
      parameter.insert("in".into(), json!("query"));
      parameter.insert("required".into(), json!(required.contains(&name.as_str())));
      if let Some(description) = description {
        parameter.insert("description".into(), description);
      }
      parameter.insert("schema".into(), Value::Object(schema));
      Value::Object(parameter)
    })
    .collect()
}
