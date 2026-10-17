//! What a stand-in road's answer tells the cache siren (docs/api.md, "The cache siren"): the routing identity
//! and the token split the answering program reported, never a prompt, an answer or a credential. A Codex
//! answer is its turn's requests, one event each, since the siren judges a request against the one before
//! it; a turn whose program counted none is one event whose usage is unknown.

use std::time::Duration;

use job_agency::{ModelSpend, Reply, RequestUsage};
use serde::Serialize;
use url::Url;

// How long one event is given.
const WAIT: Duration = Duration::from_millis(1_500);

// What a scope names a worker's turns by, ahead of the worker's name.
const SCOPE_PREFIX: &str = "job-worker:";

/// One event, in the siren's own field names.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct Event {
  pub mode: String,
  pub model: String,
  pub key: String,
  pub agent: String,
  pub project: String,
  pub cwd: String,
  pub session: String,
  pub input_tokens: i64,
  pub cache_write_input_tokens: i64,
  pub cache_read_input_tokens: i64,
  pub output_tokens: i64,
  pub usage_known: bool,
}

/// The events a Codex turn's answer is told as: one to each request it counted, or one of unknown usage.
pub fn codex_events(key: &str, scope: &str, cwd: &str, reply: &Reply, model: &str) -> Vec<Event> {
  let requests: Vec<RequestUsage> = reply.request_usages.clone().unwrap_or_default();
  let event = |usage: Option<&RequestUsage>| Event {
    mode: reply.mode.clone(),
    model: model.to_owned(),
    key: key.to_owned(),
    agent: scope.strip_prefix(SCOPE_PREFIX).unwrap_or(scope).to_owned(),
    project: basename(cwd),
    cwd: basename(cwd),
    session: reply.session_id.clone().unwrap_or_default(),
    input_tokens: usage.map_or(0, |usage| usage.input_tokens),
    cache_write_input_tokens: usage.map_or(0, |usage| usage.cache_write_tokens),
    cache_read_input_tokens: usage.map_or(0, |usage| usage.cache_read_tokens),
    output_tokens: usage.map_or(0, |usage| usage.output_tokens),
    usage_known: usage.is_some(),
  };

  if requests.is_empty() {
    vec![event(None)]
  } else {
    requests.iter().map(|usage| event(Some(usage))).collect()
  }
}

/// The one event any other road's answer is told as: what it spent on its own road and model, the usage
/// unknown where it reported none.
pub fn spend_event(key: &str, scope: &str, cwd: &str, reply: &Reply, model: &str) -> Event {
  let rows: Vec<&ModelSpend> = reply
    .spend
    .iter()
    .filter(|row| {
      row.mode.as_deref().is_none_or(|mode| mode.is_empty() || mode == reply.mode) && row.model == model
    })
    .collect();
  let sum = |field: fn(&ModelSpend) -> i64| rows.iter().map(|row| field(row)).sum();

  Event {
    mode: reply.mode.clone(),
    model: model.to_owned(),
    key: key.to_owned(),
    agent: scope.strip_prefix(SCOPE_PREFIX).unwrap_or(scope).to_owned(),
    project: basename(cwd),
    cwd: basename(cwd),
    session: reply.session_id.clone().unwrap_or_default(),
    input_tokens: sum(|row| row.input_tokens),
    cache_write_input_tokens: sum(|row| row.cache_write_tokens),
    cache_read_input_tokens: sum(|row| row.cache_read_tokens),
    output_tokens: sum(|row| row.output_tokens),
    usage_known: !rows.is_empty(),
  }
}

fn basename(path: &str) -> String {
  std::path::Path::new(path)
    .file_name()
    .map_or_else(String::new, |name| name.to_string_lossy().into_owned())
}

/// The events told in order, one after another, to the siren at `url`; the first the siren does not take ends
/// the telling, which nothing waits on. With no siren named nothing is told.
pub async fn tell(url: Option<&Url>, events: &[Event]) {
  let Some(url) = url else { return };
  let Ok(events_url) = url.join("events") else { return };
  let Ok(client) = reqwest::Client::builder().timeout(WAIT).build() else {
    return;
  };

  for event in events {
    match client.post(events_url.clone()).json(event).send().await {
      Ok(response) if !response.status().is_success() => {
        eprintln!(
          "bmo-job: the cache siren refused {}/{}: {}",
          event.mode,
          event.model,
          response.status().as_u16()
        );
      }
      Ok(_) => {}
      Err(why) => {
        eprintln!("bmo-job: the cache siren did not record {}/{}: {why}", event.mode, event.model);
        return;
      }
    }
  }
}
