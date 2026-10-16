//! A research run's own settings and the checks each of its fields must pass.

use std::collections::BTreeMap;

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Research {
  /// Every research key, in the order the screen lists them.
  pub keys: Vec<String>,
  /// The keys held to a ceiling and a research close.
  pub clocked: Vec<String>,
  /// The keys that open no branches at all.
  pub branchless: Vec<String>,
  pub search_strategies: Vec<String>,
  pub browse_strategies: Vec<String>,
  /// The most places one pool bucket may hold.
  pub pool_capacity_most: i64,
  /// The capabilities a run's research settings open, each with the run variable that opens it.
  pub opens: BTreeMap<String, String>,
  pub limits: Limits,
  pub search: Search,
}

/// What a search run's standing instructions carry after its task: the rulebooks it is bound by in order,
/// the ones given by their deciding section alone, and that section's heading.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
  pub rulebooks: Vec<String>,
  pub on_request: Vec<String>,
  pub applies_section: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
  pub min: Option<i64>,
  pub max: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
  pub cap_ms: Range,
  pub synthesis_reserve_ms: Range,
  pub research_ms: Range,
  pub max_children: Range,
  pub wait_limit_ms: Range,
  pub interval_ms: Range,
}

impl Research {
  pub fn holds(&self, key: &str) -> bool {
    self.keys.iter().any(|one| one == key)
  }

  /// Whether a kind of run is held to a ceiling and a research close at all.
  pub fn clocked(&self, key: &str) -> bool {
    self.clocked.iter().any(|one| one == key)
  }

  /// Whether a kind of run opens no branches.
  pub fn branchless(&self, key: &str) -> bool {
    self.branchless.iter().any(|one| one == key)
  }

  /// The ways a page is fetched for a key: the browse runner's, or a search's.
  pub fn strategies(&self, key: &str) -> &[String] {
    if key == "browse" { &self.browse_strategies } else { &self.search_strategies }
  }

  /// The capabilities a run's settings leave closed, read through `variable`: every one the table names for
  /// a run carrying no research setting at all, and otherwise those whose variable names nothing.
  pub fn withheld<'v>(
    &self,
    carries_settings: bool,
    variable: impl Fn(&str) -> Option<&'v str>,
  ) -> Vec<&str> {
    self
      .opens
      .iter()
      .filter(|(_, opener)| !carries_settings || !variable(opener).is_some_and(opens))
      .map(|(capability, _)| capability.as_str())
      .collect()
  }
}

/// Whether a run variable's value opens the capability it is tied to: it names at least one comma-separated
/// item other than `0`.
pub fn opens(value: &str) -> bool {
  value.split(',').map(str::trim).any(|item| !item.is_empty() && item != "0")
}

impl crate::Part for Research {
  const NAME: &'static str = "research";
  const TEXT: &'static str = crate::policy_file!("research");
}
