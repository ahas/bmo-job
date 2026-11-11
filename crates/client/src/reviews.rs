//! Reviews: the record of how the work done on an item was examined (docs/api.md, "Reviews").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::Labels;

wire_enum! {
  /// What one entry of a review is.
  ReviewKind {
    Opened = "opened",
    Report = "report",
    Question = "question",
    Answer = "answer",
    Rejection = "rejection",
    Pass = "pass",
    Escalation = "escalation",
    Ruling = "ruling",
    Accepted = "accepted",
  }
}

/// One entry of an item's review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Review {
  pub id: i64,
  pub item_id: i64,
  /// The caller's reference of the run whose work is examined, or null.
  pub r#ref: Option<String>,
  /// The stage of the item's work order the entry is about, from 1, or null for the work whole.
  pub stage: Option<i64>,
  /// The turn of the examination the entry belongs to, from 1.
  pub round: i64,
  pub kind: ReviewKind,
  pub body: String,
  pub actor_name: String,
  /// The caller's own metadata.
  pub labels: Labels,
  /// Epoch milliseconds.
  pub created_at: i64,
}

/// An entry appended to an item's review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewReview {
  pub kind: ReviewKind,
  pub body: String,
  pub actor_name: String,
  /// The caller's reference of the run whose work is examined.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub r#ref: Option<String>,
  /// The stage of the item's work order, from 1; left out for the work whole.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub stage: Option<i64>,
  /// The turn of the examination, from 1; 1 where left out.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub round: Option<i64>,
  /// The caller's own metadata.
  #[serde(default, skip_serializing_if = "Labels::is_empty")]
  pub labels: Labels,
}

/// What an item's review is read by.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewQuery {
  /// One run's entries alone.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub r#ref: Option<String>,
  /// The key of the last entry read; the page continues past it.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// How many entries at most, 50 where left out and at most 500.
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
}
