//! What a checkout's graph names (docs/api.md, "What the graph names").

use serde::Deserialize;

/// Where the graph stands inside a checkout and every file its nodes name, each relative to the checkout.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
pub struct Named {
  pub directory: String,
  pub files: Vec<String>,
}
