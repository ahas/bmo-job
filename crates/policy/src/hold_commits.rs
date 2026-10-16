//! The commit messages a work order holding its checkout writes its change under, each line with the
//! `{name}` marks its file names.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldCommits {
  pub passed: String,
  pub kept_off: String,
  pub unfinished: String,
  pub failed: String,
  pub released: String,
  pub stopped: String,
  pub taken: String,
}

impl crate::Part for HoldCommits {
  const NAME: &'static str = "hold-commits";
  const TEXT: &'static str = crate::policy_file!("hold-commits");
}
