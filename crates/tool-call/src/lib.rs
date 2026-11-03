//! What the tool host says to a placed tool and what it answers (bmo-pocket's `docs/mcp-host.md`, 「A call」),
//! and the way this repository's tools reach the job service, the agency and the receiver.
//!
//! One call is handed on standard input as one JSON object and answered on standard output as the host's tool
//! result. A record carries the holder, the namespaces, the capabilities, the grants and the variables, and a
//! field this crate does not read is ignored rather than refused, so a newer host neither breaks a call nor has
//! to change every tool.

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// The protocol version every manifest and every handed object names.
pub const PROTOCOL: u32 = 1;

/// The name of the turn's agent, handed in the record's variables.
pub const AGENT_VARIABLE: &str = "BMO_TURN_AGENT";
/// The name of the room the turn answers, handed in the record's variables.
pub const CHAT_VARIABLE: &str = "BMO_TURN_CHAT";
/// The data root every call reads, handed in the record's variables.
pub const DATA_ROOT_VARIABLE: &str = "BMO_DATA_DIR";

/// One session's record, as much of it as these tools read. Fields the record carries and this does not name
/// are ignored.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
  #[serde(default)]
  pub holder_name: String,
  #[serde(default)]
  pub namespaces: BTreeSet<String>,
  #[serde(default)]
  pub capabilities: BTreeSet<String>,
  #[serde(default)]
  pub variables: BTreeMap<String, String>,
}

/// One call handed by the host.
#[derive(Clone, Debug, Deserialize)]
pub struct Handed {
  pub protocol: u32,
  #[serde(default)]
  pub kind: String,
  #[serde(default)]
  pub entry: String,
  #[serde(default)]
  pub arguments: Value,
  #[serde(default)]
  pub session: Session,
  /// The call's socket back into the host, where it was handed one.
  #[serde(default)]
  pub callback: Option<PathBuf>,
}

/// The host's tool result.
#[derive(Clone, Debug, Serialize)]
pub struct Answer {
  pub content: Vec<Value>,
  #[serde(rename = "isError")]
  pub is_error: bool,
  #[serde(rename = "structuredContent", skip_serializing_if = "Option::is_none")]
  pub structured: Option<Value>,
}

impl Answer {
  /// A plain text answer.
  pub fn text(text: impl Into<String>) -> Self {
    Self {
      content: vec![json!({ "type": "text", "text": text.into() })],
      is_error: false,
      structured: None,
    }
  }

  /// A reading answered as one line each, and as the object `reading.json` declares.
  pub fn lines(lines: Vec<String>) -> Self {
    let text = if lines.is_empty() { "Nothing stands there now.".to_owned() } else { lines.join("\n") };
    let structured = json!({ "lines": lines });

    Self {
      content: vec![json!({ "type": "text", "text": text })],
      is_error: false,
      structured: Some(structured),
    }
  }

  /// An answer carrying a value a program reads beside its text.
  pub fn structured(value: Value) -> Self {
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();

    Self {
      content: vec![json!({ "type": "text", "text": text })],
      is_error: false,
      structured: Some(value),
    }
  }

  /// A failure saying why in English.
  pub fn refusal(why: impl Into<String>) -> Self {
    Self {
      content: vec![json!({ "type": "text", "text": why.into() })],
      is_error: true,
      structured: None,
    }
  }
}

/// The last part of a full name, `<namespace>/<name>`: the entry a manifest declares.
pub fn entry_name(entry: &str) -> &str {
  entry.rsplit('/').next().unwrap_or(entry)
}

/// The one call this process was handed, read off standard input.
pub fn read_handed() -> Result<Handed, String> {
  let mut text = String::new();
  std::io::stdin()
    .lock()
    .read_to_string(&mut text)
    .map_err(|why| format!("the call could not be read: {why}"))?;

  serde_json::from_str(&text).map_err(|why| format!("the call does not read: {why}"))
}

/// One answer written on standard output.
pub fn write_answer(answer: &Answer) -> std::process::ExitCode {
  let text = match serde_json::to_string(answer) {
    Ok(text) => text,
    Err(why) => {
      eprintln!("the answer could not be written: {why}");
      return std::process::ExitCode::FAILURE;
    }
  };

  match std::io::stdout().lock().write_all(text.as_bytes()) {
    Ok(()) => std::process::ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("the answer could not be written: {why}");
      std::process::ExitCode::FAILURE
    }
  }
}

/// A variable this process's environment holds, or one an env file's `KEY=value` lines hold.
pub fn variable(name: &str, file: &BTreeMap<String, String>) -> Option<String> {
  std::env::var(name)
    .ok()
    .or_else(|| file.get(name).cloned())
    .map(|value| value.trim().to_owned())
    .filter(|value| !value.is_empty())
}

/// The `KEY=value` lines of an env file, none where it does not stand.
pub fn env_file(path: &Path) -> BTreeMap<String, String> {
  std::fs::read_to_string(path)
    .map(|text| {
      text
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_owned(), value.trim().to_owned()))
        .collect()
    })
    .unwrap_or_default()
}

/// The text a token file holds, trimmed, or none where it does not stand.
pub fn token_file(path: &Path) -> Option<String> {
  std::fs::read_to_string(path).ok().map(|text| text.trim().to_owned()).filter(|text| !text.is_empty())
}

/// The data root: the record's, else this process's, else the deployment's conventional one under the home the
/// host passes through.
pub fn data_root(session: &Session) -> PathBuf {
  session
    .variables
    .get(DATA_ROOT_VARIABLE)
    .map(PathBuf::from)
    .or_else(|| std::env::var(DATA_ROOT_VARIABLE).ok().map(PathBuf::from))
    .or_else(|| std::env::var("HOME").ok().map(|home| PathBuf::from(home).join(".local/share/bmo/data")))
    .unwrap_or_else(|| PathBuf::from("/nonexistent"))
}

/// An address under a listen setting, `127.0.0.1:13103` read as `http://127.0.0.1:13103`.
pub fn address(listen: &str) -> String {
  if listen.contains("://") {
    listen.trim_end_matches('/').to_owned()
  } else {
    format!("http://{}", listen.trim_end_matches('/'))
  }
}

/// The agent the call acts for: the turn's, else the record's holder.
pub fn turn_agent(session: &Session) -> Option<String> {
  session
    .variables
    .get(AGENT_VARIABLE)
    .cloned()
    .filter(|agent| !agent.is_empty())
    .or_else(|| Some(session.holder_name.clone()).filter(|holder| !holder.is_empty()))
}

/// The room the turn answers, where it answers one.
pub fn turn_room(session: &Session) -> Option<i64> {
  session.variables.get(CHAT_VARIABLE).and_then(|room| room.trim().parse().ok())
}

/// The queue a filed item waits in: the first the deployment names, `todo` otherwise.
pub fn queue_name(session: &Session) -> String {
  queue_name_of(&data_root(session))
}

fn queue_name_of(root: &Path) -> String {
  let file = env_file(&root.join("env/agency.env"));
  variable("BMO_JOB_QUEUES", &file)
    .and_then(|queues| queues.split(',').map(str::trim).find(|one| !one.is_empty()).map(str::to_owned))
    .unwrap_or_else(|| "todo".to_owned())
}

/// One service a tool reaches: its base address and its bearer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Service {
  pub url: String,
  pub token: String,
}

/// The job service: its settings under the data root, and the chat caller's token where the deployment keeps
/// one, since filing and approving are the chat agent's acts.
pub fn job_service(session: &Session) -> Result<Service, String> {
  let root = data_root(session);
  let settings: toml::Table = std::fs::read_to_string(root.join("env/bmo-job.toml"))
    .ok()
    .and_then(|text| text.parse().ok())
    .unwrap_or_default();
  let listen = settings.get("server").and_then(|server| server.get("listen")).and_then(toml::Value::as_str);
  let url = std::env::var("BMO_JOB_URL")
    .ok()
    .filter(|url| !url.is_empty())
    .or_else(|| listen.map(address))
    .ok_or_else(|| "the job service's address is set nowhere: no BMO_JOB_URL and no server.listen in env/bmo-job.toml".to_owned())?;
  let token = std::env::var("BMO_JOB_TOKEN")
    .ok()
    .filter(|token| !token.is_empty())
    .or_else(|| token_file(&root.join("env/job-chat.token")))
    .or_else(|| token_file(&root.join("env/job.token")))
    .ok_or_else(|| "the job service's token is set nowhere".to_owned())?;

  Ok(Service { url, token })
}

/// The agency's control port, from its own environment file.
pub fn agency_service(session: &Session) -> Result<Service, String> {
  let root = data_root(session);
  let file = env_file(&root.join("env/agency.env"));
  let url = variable("BMO_AGENCY_URL", &file).or_else(|| {
    variable("AGENCY_CONTROL_PORT", &file).map(|port| address(&format!("127.0.0.1:{port}")))
  });
  let url = url.ok_or_else(|| "the agency's address is set nowhere".to_owned())?;
  let token = variable("AGENCY_CONTROL_TOKEN", &file)
    .or_else(|| {
      variable("AGENCY_CONTROL_TOKEN_FILE", &file).and_then(|path| token_file(Path::new(&path)))
    })
    .ok_or_else(|| "the agency's token is set nowhere".to_owned())?;

  Ok(Service { url, token })
}

/// The receiver, from its own environment file.
pub fn receiver_service(session: &Session) -> Result<Service, String> {
  let root = data_root(session);
  let agency = env_file(&root.join("env/agency.env"));
  let file = env_file(&root.join("env/receiver.env"));
  let url = std::env::var("BMO_RECEIVER_URL")
    .ok()
    .filter(|url| !url.is_empty())
    .or_else(|| variable("RECEIVER_ADDRESS", &file))
    .ok_or_else(|| "the receiver's address is set nowhere".to_owned())?;
  let token = std::env::var("RECEIVER_TOKEN")
    .ok()
    .filter(|token| !token.is_empty())
    .or_else(|| variable("RECEIVER_TOKEN", &file))
    .or_else(|| {
      variable("BMO_RECEIVER_TOKEN_FILE", &agency).and_then(|path| token_file(Path::new(&path)))
    })
    .or_else(|| token_file(&root.join("env/receiver.token")))
    .ok_or_else(|| "the receiver's token is set nowhere".to_owned())?;

  Ok(Service { url, token })
}

fn client() -> reqwest::Client {
  reqwest::Client::new()
}

fn failure(status: reqwest::StatusCode, body: &str) -> String {
  let said: Value = serde_json::from_str(body).unwrap_or(Value::Null);
  let code = said.get("code").and_then(Value::as_str).unwrap_or("error");
  let error = said.get("error").and_then(Value::as_str).unwrap_or(body).trim();

  if error.is_empty() {
    format!("{status} ({code})")
  } else {
    format!("{error} ({code})")
  }
}

async fn answered(response: reqwest::Response) -> Result<Value, String> {
  let status = response.status();
  let text = response.text().await.map_err(|why| format!("the answer could not be read: {why}"))?;

  if !status.is_success() {
    return Err(failure(status, &text));
  }

  if text.trim().is_empty() {
    return Ok(Value::Null);
  }

  serde_json::from_str(&text).map_err(|why| format!("the answer does not read: {why}"))
}

/// A read of one service route.
pub async fn get(service: &Service, path: &str, query: &[(&str, String)]) -> Result<Value, String> {
  let mut request = client().get(format!("{}{}", service.url, path)).bearer_auth(&service.token);

  for (key, value) in query {
    request = request.query(&[(key, value)]);
  }

  let response = request.send().await.map_err(|why| format!("{} could not be reached: {why}", service.url))?;

  answered(response).await
}

/// A POST of one JSON body.
pub async fn post(service: &Service, path: &str, body: &Value) -> Result<Value, String> {
  let response = client()
    .post(format!("{}{}", service.url, path))
    .bearer_auth(&service.token)
    .json(body)
    .send()
    .await
    .map_err(|why| format!("{} could not be reached: {why}", service.url))?;

  answered(response).await
}

/// A PATCH of one JSON body.
pub async fn patch(service: &Service, path: &str, body: &Value) -> Result<Value, String> {
  let response = client()
    .patch(format!("{}{}", service.url, path))
    .bearer_auth(&service.token)
    .json(body)
    .send()
    .await
    .map_err(|why| format!("{} could not be reached: {why}", service.url))?;

  answered(response).await
}

/// One entry called on the call's own socket back into the host, answered as the host answers a call.
pub async fn call_back(socket: &Path, entry: &str, arguments: Value) -> Result<Value, String> {
  let at = socket.display();
  let stream = UnixStream::connect(socket)
    .await
    .map_err(|why| format!("{at} could not be reached: {why}"))?;
  let (reading, mut writing) = stream.into_split();
  let mut line = serde_json::to_string(&json!({ "kind": "call", "entry": entry, "arguments": arguments }))
    .map_err(|why| format!("what was asked could not be written: {why}"))?;

  line.push('\n');
  writing
    .write_all(line.as_bytes())
    .await
    .map_err(|why| format!("{at} could not be asked: {why}"))?;

  let mut answer = String::new();
  BufReader::new(reading)
    .read_line(&mut answer)
    .await
    .map_err(|why| format!("{at} could not be read: {why}"))?;

  if answer.trim().is_empty() {
    return Err(format!("{at} answered nothing"));
  }

  serde_json::from_str(&answer).map_err(|why| format!("the answer of {at} does not read: {why}"))
}

/// The text a tool result carries, joined from its content pieces.
pub fn result_text(result: &Value) -> String {
  result
    .get("content")
    .and_then(Value::as_array)
    .map(|pieces| {
      pieces
        .iter()
        .filter_map(|piece| piece.get("text").and_then(Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
    })
    .unwrap_or_default()
}

/// Whether a tool result marks itself failed.
pub fn result_failed(result: &Value) -> bool {
  result.get("isError").and_then(Value::as_bool).unwrap_or(false)
}

/// The value a tool result carries structured, where it carries one.
pub fn result_structured(result: &Value) -> Option<Value> {
  result.get("structuredContent").cloned().filter(|value| !value.is_null())
}

/// One line's words, cut to `most` characters with an ellipsis.
pub fn shortened(text: &str, most: usize) -> String {
  let text = text.trim();

  match text.char_indices().nth(most) {
    Some((at, _)) => format!("{}…", &text[..at]),
    None => text.to_owned(),
  }
}
