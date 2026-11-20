//! Label values: the values a label key may take, as a caller published them (docs/api.md, "Label values").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The values one label key may take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LabelValues {
  pub key: String,
  /// Each trimmed, kept once, in sorted order.
  pub values: Vec<String>,
  /// Epoch milliseconds.
  pub changed_at: i64,
}

/// A key's list published whole, replacing the one that stood.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabelValuesBody {
  /// The values the key may take; each is trimmed, a blank one dropped, and each kept once.
  pub values: Vec<String>,
}

/// The cursor of the list of label values, which is in key order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LabelValuesQuery {
  /// The key of the last list read; the page continues past it.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub after: Option<String>,
  /// How many lists at most, 50 where left out and at most 500.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub limit: Option<i64>,
}
