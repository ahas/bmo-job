//! The TODO tool's three filings (docs/tools.md): a row filed, words added to an item, and an approval given or
//! taken back, each run as the unit CLI's `todo` verb (docs/cli.md, "`todo`") with the turn's agent, run and
//! work-order step, so what it files stands under them. The room a filing is made in is the one the call names
//! or the turn's own. The message a call acts on is the host's to judge its grant by, and is not handed on.

use futures_util::future::BoxFuture;
use job_tool_call::turn::NO_ROOM;
use job_tool_call::{Answer, Service, Turn, input};
use serde::Deserialize;
use serde_json::Value;

/// A row to file.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddInput {
  pub chat: Option<i64>,
  pub title: String,
  pub what: Option<String>,
  #[serde(rename = "do")]
  pub to_do: Option<String>,
  pub conclusion: Option<String>,
  pub instruction: Option<Vec<i64>>,
  pub reply: Option<Vec<i64>>,
  pub reply_text: Option<String>,
  pub requester: Option<String>,
  pub model: Option<String>,
  pub status: Option<String>,
  #[serde(default)]
  pub cowork: bool,
  /// The grounds the co-work setting was weighed on, handed to the verb as the JSON they are.
  pub impact: Option<Value>,
  pub judgement: Option<String>,
  pub raised_by: Option<i64>,
  pub from_todo: Option<String>,
  pub message: Option<i64>,
}

/// Words to add to an item.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AppendInput {
  pub item: String,
  pub text: String,
  pub by: Option<String>,
  pub message: Option<i64>,
}

/// An approval to give or take back.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApproveInput {
  pub item: String,
  pub note: Option<String>,
  #[serde(default)]
  pub unapprove: bool,
  pub message: Option<i64>,
}

// The words of a flag that takes a value, where one is given.
fn flag(words: &mut Vec<String>, name: &str, value: Option<String>) {
  if let Some(value) = value {
    words.extend([format!("--{name}"), value]);
  }
}

/// The words a filing is run with: `todo --add` in the room, with every part given.
pub fn add_words(room: i64, asked: AddInput) -> Vec<String> {
  let mut words = vec!["todo".to_owned(), "--add".to_owned(), "--chat".to_owned(), room.to_string()];

  flag(&mut words, "title", Some(asked.title));
  flag(&mut words, "what", asked.what);
  flag(&mut words, "do", asked.to_do);
  flag(&mut words, "conclusion", asked.conclusion);

  for (name, ids) in [("instruction", asked.instruction), ("reply", asked.reply)] {
    for id in ids.into_iter().flatten() {
      flag(&mut words, name, Some(id.to_string()));
    }
  }

  flag(&mut words, "reply-text", asked.reply_text);
  flag(&mut words, "requester", asked.requester);
  flag(&mut words, "model", Some(asked.model.unwrap_or_else(|| "auto".to_owned())));
  flag(&mut words, "status", asked.status);

  if asked.cowork {
    words.push("--cowork".to_owned());
  }

  flag(&mut words, "impact", asked.impact.map(|grounds| grounds.to_string()));
  flag(&mut words, "judgement", asked.judgement);
  flag(&mut words, "raised-by", asked.raised_by.map(|id| id.to_string()));
  flag(&mut words, "from-todo", asked.from_todo);
  words
}

/// The words words added are run with.
pub fn append_words(asked: AppendInput) -> Vec<String> {
  let mut words = vec!["todo".to_owned()];

  flag(&mut words, "append", Some(asked.item));
  flag(&mut words, "text", Some(asked.text));
  flag(&mut words, "by", asked.by);
  words
}

/// The words an approval is run with, or its taking back.
pub fn approve_words(asked: ApproveInput) -> Vec<String> {
  let mut words = vec!["todo".to_owned()];

  flag(&mut words, if asked.unapprove { "unapprove" } else { "approve" }, Some(asked.item));
  flag(&mut words, "note", asked.note);
  words
}

// The verb run on its words under the turn: what it answered, its codes one a line, or why it refused.
async fn filed(turn: &Turn, words: Vec<String>) -> Answer {
  let service = match Service::of_process() {
    Ok(service) => service,
    Err(why) => return Answer::refusal(why),
  };

  match service.cli(&words, &turn.verb_variables()).await {
    Ok(lines) => Answer::text(lines.join("\n")),
    Err(why) => Answer::refusal(why),
  }
}

pub fn add<'a>(turn: &'a Turn, arguments: Value, entry: &'a str) -> BoxFuture<'a, Answer> {
  Box::pin(async move {
    let asked: AddInput = match input(entry, arguments) {
      Ok(asked) => asked,
      Err(refused) => return refused,
    };
    let Some(room) = turn.room(asked.chat) else {
      return Answer::refusal(NO_ROOM);
    };

    filed(turn, add_words(room, asked)).await
  })
}

pub fn append<'a>(turn: &'a Turn, arguments: Value, entry: &'a str) -> BoxFuture<'a, Answer> {
  Box::pin(async move {
    match input(entry, arguments) {
      Ok(asked) => filed(turn, append_words(asked)).await,
      Err(refused) => refused,
    }
  })
}

pub fn approve<'a>(turn: &'a Turn, arguments: Value, entry: &'a str) -> BoxFuture<'a, Answer> {
  Box::pin(async move {
    match input(entry, arguments) {
      Ok(asked) => filed(turn, approve_words(asked)).await,
      Err(refused) => refused,
    }
  })
}
