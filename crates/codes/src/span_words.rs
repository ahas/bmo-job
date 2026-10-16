//! What a run's spans are named by and the tokens they count.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

/// What a span is named by.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum SpanName {
  Tool { name: String },
  Server { server: String, tool: String },
  Coworker { agent_type: Option<String> },
}

/// A sort of token a run counts.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TokenKind {
  Input,
  Output,
  CacheRead,
  CacheWrite,
}
