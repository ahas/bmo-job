//! Reports: what the one doing an item tells of it (docs/api.md, "Reports").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::fields::Labels;

wire_enum! {
  /// What a report tells: progress, the final account, or why the item waits.
  ReportKind {
    Progress = "progress",
    Final = "final",
    Held = "held",
  }
}

/// A report on an item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Report {
  pub id: i64,
  pub item_id: i64,
  pub kind: ReportKind,
  pub body: String,
  pub actor_name: String,
  /// The caller's own metadata.
  pub labels: Labels,
  /// Epoch milliseconds.
  pub created_at: i64,
}

/// A report appended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NewReport {
  pub kind: ReportKind,
  pub body: String,
  pub actor_name: String,
  /// The caller's own metadata.
  #[serde(default, skip_serializing_if = "Labels::is_empty")]
  pub labels: Labels,
}
