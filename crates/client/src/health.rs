//! The service's readiness.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether the service is ready to answer, why not where it is not, and its version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct Health {
  pub ready: bool,
  pub reason: Option<String>,
  pub version: String,
}
