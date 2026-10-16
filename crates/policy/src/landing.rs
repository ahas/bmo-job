//! What a landing says, each line with the `{name}` marks its file names.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LandingRefusals {
  pub open: String,
  pub origin: String,
  pub conflict: String,
  pub no_file: String,
  pub base: String,
  pub overwrite: String,
  pub check: String,
  pub moving: String,
  pub locked: String,
  pub push: String,
  pub submodule: String,
  pub language: String,
  pub commit: String,
  pub joiner: String,
  pub running: String,
  pub unlinked: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnitLinkWords {
  pub relinked: String,
  pub dangling: String,
  pub failed: String,
  pub unmatched: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HomeLandingWords {
  pub not_taken_in: String,
  pub unrecorded: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LandVerbWords {
  pub not_work_branch: String,
  pub no_branch: String,
  pub refused: String,
  pub said: String,
  pub landed: String,
  pub nothing: String,
  pub one: String,
  pub many: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Landing {
  pub refusal: LandingRefusals,
  pub units: UnitLinkWords,
  pub home: HomeLandingWords,
  pub verb: LandVerbWords,
}

impl crate::Part for Landing {
  const NAME: &'static str = "landing";
  const TEXT: &'static str = crate::policy_file!("landing");
}
