//! The job service's harness plugin, `bmo-job` (docs/api.md, "The harness plugin").
//!
//! `bmo-job mcp` starts the MCP server this crate serves: the draft's tools, each one route called through
//! bmo-job-client, and the item and queue resources its notifications name. The skill beside this crate, in
//! `skills/bmo-job`, tells a harness when and how to use it. The crate also makes the service's whole OpenAPI
//! document, which states the tools under `x-tools`.

mod harness;
mod openapi;
mod server;
mod tools;

use std::time::Duration;

use bmo_job_client::JobClient;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

pub use harness::{Harness, pushed_message, updated_resources};
pub use openapi::{openapi, openapi_json};
pub use server::JobServer;
pub use tools::{Answered, JobTool, LeaseTaken, ToolFailure, tools};

/// How the server is started: the service it reaches, and what it does first for its harness.
#[derive(Debug, Clone, Default)]
pub struct Start {
  /// The base address the service's routes stand at.
  pub service: String,
  /// The service's bearer token.
  pub token: String,
  /// The holder the queues named at the start are subscribed for.
  pub holder: Option<String>,
  /// Queues to be told of from the start.
  pub queues: Vec<String>,
  /// The experimental capability events are pushed on as messages.
  pub channel: Option<String>,
  /// Tools left out of what the server offers, by name.
  pub without: Vec<String>,
}

/// The names of `without` that are no tool of the plugin.
pub fn unknown_tools(without: &[String]) -> Vec<String> {
  let known: Vec<&str> = tools().iter().map(|tool| tool.name).collect();

  without.iter().filter(|name| !known.contains(&name.as_str())).cloned().collect()
}

/// Serves the MCP server on standard input and output until the harness ends the session, then gives back
/// what it kept.
pub async fn serve_stdio(start: Start) -> Result<(), String> {
  let unknown = unknown_tools(&start.without);
  if !unknown.is_empty() {
    return Err(format!("--without names no tool of the plugin: {}", unknown.join(", ")));
  }
  let client = JobClient::new(&start.service, start.token).map_err(|failure| failure.to_string())?;
  let harness = Harness::new(client, start.channel);
  if !start.queues.is_empty() {
    let holder = start.holder.ok_or("queues named at the start need a holder name")?;
    harness
      .subscribe_to_queues(&holder, &start.queues)
      .await
      .map_err(|failure| format!("the queues could not be subscribed to: {failure}"))?;
  }
  let renewing = tokio::spawn(std::sync::Arc::clone(&harness).renew());
  let served = JobServer::new(std::sync::Arc::clone(&harness))
    .without(&start.without)
    .serve(stdio())
    .await;
  let ended = match served {
    Ok(running) => running
      .waiting()
      .await
      .map(|_| ())
      .map_err(|error| format!("the MCP session failed: {error}")),
    Err(error) => Err(format!("the MCP session did not start: {error}")),
  };
  renewing.abort();
  // What the server kept is let go within a bound, so a service that no longer answers does not hold the exit.
  let _ = tokio::time::timeout(Duration::from_secs(10), harness.let_go()).await;
  ended
}
