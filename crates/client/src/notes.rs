//! Notes: words added to an item after it was raised (docs/api.md, "Notes").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::Labels;

/// A note on an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Note {
  pub id: i64,
  pub item_id: i64,
  pub text: String,
  pub actor_name: String,
  /// The caller's own metadata.
  pub labels: Labels,
  /// Epoch milliseconds.
  pub created_at: i64,
}

/// A note appended.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewNote {
  pub text: String,
  pub actor_name: String,
  /// The caller's own metadata.
  #[serde(default, skip_serializing_if = "Labels::is_empty")]
  pub labels: Labels,
}

/// The cursor of a list that can wait for its next member.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WaitQuery {
  /// How long to hold the answer open where nothing stands past the cursor, at most 60000.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub wait_ms: Option<i64>,
  /// The key of the last member read; the page continues past it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub after: Option<i64>,
  /// How many members at most, 50 where left out and at most 500.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
}
