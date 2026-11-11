//! What the service holds under a caller's scope (docs/api.md, "Scopes").

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What the service holds under one scope, counted by resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopeHolding {
  pub scope: String,
  pub counts: ScopeCounts,
  /// Null for a reading, `preview` for a deletion previewed, `deleted` for one made.
  pub state: Option<String>,
}

/// The members held under a scope, by resource.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ScopeCounts {
  pub items: i64,
  pub notes: i64,
  pub reports: i64,
  #[serde(default)]
  pub reviews: i64,
  pub work_orders: i64,
  pub tasks: i64,
  pub leases: i64,
  pub questions: i64,
  pub replies: i64,
  pub subscriptions: i64,
}

impl ScopeCounts {
  /// Whether nothing is held.
  pub fn is_empty(&self) -> bool {
    *self == Self::default()
  }
}

/// A deletion of a scope, or its preview.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScopeDeletion {
  /// Lets nothing go and answers what would go.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub dry_run: Option<bool>,
}
