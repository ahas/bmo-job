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
    Unsent = "unsent",
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
  /// Whether the asker puts it before whoever answers it itself, so no consumer of the events tells of it.
  pub told_by_asker: bool,
  /// The last reply its asker has been handed; null where it has been handed none.
  pub heard_through: Option<i64>,
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
  /// How long the question goes unanswered before it is told of as due, 1 000 to 86 400 000 milliseconds;
  /// 60 000 where left out. It paces that notice alone: the question waits for its answer.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub wait_ms: Option<i64>,
  /// Whether the asker puts it before whoever answers it itself; false where left out.
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub told_by_asker: bool,
}

/// A question paused, opened again, withdrawn or closed unsent; put again in other words; or told how far its
/// asker has been handed its replies.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionChange {
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub state: Option<QuestionState>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub text: Option<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub choices: Option<Vec<String>>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub recommendation: Option<QuestionRecommendation>,
  /// The last reply the asker was handed.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub heard_through: Option<i64>,
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
  /// The caller's name.
  pub actor_name: String,
  /// Whether the reply settled the question, or is a word on it.
  pub settles: bool,
  /// Who gave it: a person for every reply written; a reply kept in the store may read `recommendation`.
  pub source: ReplySource,
  /// The recommendation's reason on a reply whose source is the recommendation; null for a person's reply.
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
