//! A call of the service's tools (docs/tools.md, "A call"): what the tool host hands a tool's executable on
//! standard input, the turn its session carries, the answer the executable writes on standard output, the
//! check that holds a tool's manifest to its entries, and the two roads to the service: its API and its unit
//! CLI.

pub mod manifest;
pub mod service;
pub mod turn;

use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub use service::Service;
pub use turn::Turn;

/// The protocol a tool answers.
pub const PROTOCOL: u32 = 1;

/// What the host hands a tool's executable: the protocol, what is asked, and the session it is asked under.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct Handed {
  pub protocol: u32,
  pub kind: String,
  #[serde(default)]
  pub entry: String,
  #[serde(default)]
  pub arguments: Value,
  #[serde(default)]
  pub session: Value,
}

/// One piece of an answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Content {
  Text { text: String },
}

/// What a tool answers: its text, whether it is an error, and the object of an entry with an output schema.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Answer {
  pub content: Vec<Content>,
  #[serde(rename = "isError", default)]
  pub is_error: bool,
  #[serde(rename = "structuredContent", default, skip_serializing_if = "Option::is_none")]
  pub structured: Option<Value>,
}

impl Answer {
  pub fn text(text: impl Into<String>) -> Self {
    Self {
      content: vec![Content::Text { text: text.into() }],
      is_error: false,
      structured: None,
    }
  }

  /// A refusal, which the model reads as it reads any other answer.
  pub fn refusal(text: impl Into<String>) -> Self {
    Self {
      is_error: true,
      ..Self::text(text)
    }
  }

  /// A reading under the reading schema: its lines, and the same lines as its text.
  pub fn reading(lines: Vec<String>) -> Self {
    Self {
      structured: Some(serde_json::json!({ "lines": lines })),
      ..Self::text(lines.join("\n"))
    }
  }

  /// The text of the answer, its pieces joined.
  pub fn plain(&self) -> String {
    self
      .content
      .iter()
      .map(|Content::Text { text }| text.as_str())
      .collect::<Vec<_>>()
      .join("\n")
  }
}

/// One entry of a tool: its full name and its call.
pub trait Entry: Sync {
  fn namespace(&self) -> &'static str;

  fn name(&self) -> &'static str;

  /// The entry called with its arguments under the turn its session carries.
  fn call<'a>(&'a self, turn: &'a Turn, arguments: Value) -> BoxFuture<'a, Answer>;

  /// `<namespace>/<name>`, as the host names the entry.
  fn full_name(&self) -> String {
    format!("{}/{}", self.namespace(), self.name())
  }
}

/// The arguments read into an entry's input, or the answer saying they do not fit it.
pub fn input<T: serde::de::DeserializeOwned>(entry: &str, arguments: Value) -> Result<T, Answer> {
  serde_json::from_value(arguments)
    .map_err(|why| Answer::refusal(format!("invalid arguments: {entry}: {why}")))
}

/// One request read and answered by the entries of an executable: the answer it writes, or why the request
/// does not read.
pub async fn answer(entries: &[&dyn Entry], handed: &str) -> Result<Answer, String> {
  let handed: Handed =
    serde_json::from_str(handed).map_err(|why| format!("the call does not read: {why}"))?;

  if handed.protocol != PROTOCOL {
    return Ok(Answer::refusal(format!(
      "this executable answers protocol {PROTOCOL}, not {}",
      handed.protocol
    )));
  }

  if handed.kind != "call" {
    return Ok(Answer::refusal("this executable answers calls alone"));
  }

  let Some(entry) = entries.iter().find(|entry| entry.full_name() == handed.entry) else {
    return Ok(Answer::refusal(format!("this executable holds no entry {}", handed.entry)));
  };
  let turn = Turn::of(&handed.session);

  Ok(entry.call(&turn, handed.arguments).await)
}

/// A tool's whole `main`: its call read off standard input and answered on standard output, a request that
/// does not read said on standard error and ended unclean.
pub async fn answer_standard_io(entries: &[&dyn Entry]) -> std::process::ExitCode {
  let read = std::io::read_to_string(std::io::stdin()).map_err(|why| why.to_string());
  let answered = match read {
    Ok(handed) => answer(entries, &handed).await,
    Err(why) => Err(why),
  };

  match answered.and_then(|answer| serde_json::to_string(&answer).map_err(|why| why.to_string())) {
    Ok(written) => {
      println!("{written}");
      std::process::ExitCode::SUCCESS
    }
    Err(why) => {
      eprintln!("{why}");
      std::process::ExitCode::FAILURE
    }
  }
}
