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
  /// Who a question is put to: a person who settles it, or the supervisor of the work under its scope, which
  /// settles it itself or puts it to a person.
  #[derive(Default)]
  QuestionAddressee {
    #[default]
    Person = "person",
    Supervisor = "supervisor",
  }
}

wire_enum! {
  /// Who gave a reply: a caller that answers for a person, a caller that supervises the work, or the question's
  /// recommendation, which the service settles it with once its due time passes.
  ReplySource {
    Person = "person",
    Supervisor = "supervisor",
    Recommendation = "recommendation",
  }
}

/// The answer an asker would take by itself, and the one line saying why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionRecommendation {
  /// One of the question's choices word for word, or a free answer where the choices do not cover it; never a
  /// choice with other words before or after it.
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
  /// Who it is put to now; a person where the ask named none.
  #[serde(default)]
  pub addressee: QuestionAddressee,
  pub text: String,
  /// The answers to choose from; empty for an open question.
  pub choices: Vec<String>,
  /// The answer the asker would take by itself, and why; null where it would not decide alone.
  pub recommendation: Option<QuestionRecommendation>,
  pub state: QuestionState,
  /// Epoch milliseconds the asker wants a person's answer by, when its recommendation settles it; null while
  /// paused, while the supervisor holds it, or where none was given.
  pub due_at: Option<i64>,
  /// Whether the asker puts it before whoever answers it itself, so no consumer of the events tells of it.
  pub told_by_asker: bool,
  /// The asker's own reference to the notice it put the question before whoever answers it with, opaque to the
  /// service; null where it keeps none.
  #[serde(default)]
  pub told_as: Option<String>,
  /// The last reply its asker has been handed; null where it has been handed none.
  pub heard_through: Option<i64>,
  /// The caller that put it to a person in the supervisor's place; null where nobody did.
  #[serde(default)]
  pub readdressed_by: Option<String>,
  /// Epoch milliseconds it was put to a person in the supervisor's place; null where it was not.
  #[serde(default)]
  pub readdressed_at: Option<i64>,
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
  /// Who it is put to; a person where left out. A caller that does not supervise the work asks the supervisor,
  /// whatever it names.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub addressee: Option<QuestionAddressee>,
  pub text: String,
  #[serde(default, skip_serializing_if = "Vec::is_empty")]
  pub choices: Vec<String>,
  /// The answer the asker would take by itself, and why; left out where it cannot make the case.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub recommendation: Option<QuestionRecommendation>,
  /// How long a question asked of a person goes unanswered by him before its due time passes, 1 000 to 86 400 000
  /// milliseconds, counted from the ask and held to the deployment's person wait, a minute unless its settings name
  /// another, which also stands where this is left out. Its recommendation settles it then; with none, it is told
  /// of as due and waits on. A question put to the supervisor takes the wait the supervisor's move names
  /// once it is put to a person, and none of this one.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub wait_ms: Option<i64>,
  /// Whether the asker puts it before whoever answers it itself; false where left out.
  #[serde(default, skip_serializing_if = "std::ops::Not::not")]
  pub told_by_asker: bool,
}

/// A question paused, opened again, withdrawn or closed unsent; put again in other words; told how far its asker
/// has been handed its replies; or put to a person by the supervisor that held it.
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
  /// `person` puts a question the supervisor holds to a person, and `supervisor` takes one put to a person back
  /// while nobody has settled it; only a supervising caller may.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub addressee: Option<QuestionAddressee>,
  /// With `addressee` `person` alone: how long the person has before its due time passes, 1 000 to 86 400 000
  /// milliseconds from the move, held to the deployment's person wait, a minute unless its settings name another,
  /// which also stands where this is left out. Its recommendation settles it then.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub wait_ms: Option<i64>,
  /// The asker's reference to the notice it put the question before whoever answers it with, set or replaced.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub told_as: Option<String>,
  /// Makes the whole change conditional on the question being put to this addressee; nothing is changed and the
  /// change is refused `conflict` otherwise.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub if_addressee: Option<QuestionAddressee>,
}

/// The filters of the question list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuestionQuery {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub scope: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub item_id: Option<i64>,
  /// One addressee, or several comma-separated.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub addressee: Option<String>,
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
  /// The voice it was given in: `supervisor` from a caller that supervises the work, `person` from any other; a
  /// reply kept in the store may read `recommendation`.
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
