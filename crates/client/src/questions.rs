//! Questions and their replies (docs/api.md, "Questions and replies").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

wire_enum! {
  /// Where a question stands.
  QuestionState {
    Open = "open",
    Paused = "paused",
    Answered = "answered",
    Withdrawn = "withdrawn",
  }
}

wire_enum! {
  /// Who gave a reply.
  ReplySource {
    Person = "person",
    Recommendation = "recommendation",
  }
}

/// The answer an asker would take by itself, and the one line saying why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionRecommendation {
  /// One of the question's choices, or a free answer where the choices do not cover it.
  pub answer: String,
  /// The one line saying why that is the answer.
  pub why: String,
}

/// A question, about an item or on its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Question {
  pub id: i64,
  pub item_id: Option<i64>,
  pub asker_name: String,
  pub scope: Option<String>,
  pub text: String,
  /// The answers to choose from; empty for an open question.
  pub choices: Vec<String>,
  /// The answer the asker would take by itself, and why; null where it would not decide alone.
  pub recommendation: Option<QuestionRecommendation>,
  pub state: QuestionState,
  /// Epoch milliseconds the asker wants an answer by; null while paused or where none was given.
  pub due_at: Option<i64>,
  /// Epoch milliseconds.
  pub created_at: i64,
}

/// A question asked.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewQuestion {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub item_id: Option<i64>,
  pub asker_name: String,
  /// The item's scope where left out.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub scope: Option<String>,
  pub text: String,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub choices: Vec<String>,
  /// The answer the asker would take by itself, and why; left out where it cannot make the case.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub recommendation: Option<QuestionRecommendation>,
  /// The wait before the due time, 1 000 to 86 400 000 milliseconds; 60 000 where left out.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub wait_ms: Option<i64>,
}

/// A question paused, opened again or withdrawn.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionChange {
  pub state: QuestionState,
}

/// The filters of the question list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionQuery {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub scope: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub item_id: Option<i64>,
  /// One state, or several comma-separated.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub state: Option<String>,
  /// The key of the last question read; the page continues past it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// How many questions at most, 50 where left out and at most 500.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
}

/// A reply to a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Reply {
  pub id: i64,
  pub question_id: i64,
  pub choice: Option<String>,
  pub text: String,
  /// The caller's name, or `recommendation` where the recommendation gave it.
  pub actor_name: String,
  /// Whether the reply settled the question, or is a word on it.
  pub settles: bool,
  /// Who gave it: a person, or the recommendation when the wait ran out.
  pub source: ReplySource,
  /// The recommendation's reason where the recommendation gave it; null for a person's reply.
  pub why: Option<String>,
  /// Epoch milliseconds.
  pub created_at: i64,
}

/// A reply given.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewReply {
  /// One of the question's choices.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub choice: Option<String>,
  pub text: String,
  pub actor_name: String,
  /// Whether the reply settles the question; left out, it settles where it carries a choice or the question
  /// has none.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub settles: Option<bool>,
}
