//! Heavy commands: whether a shell line starts one outside the testbed, and the line that runs it there.

use serde::Deserialize;

/// What a shell line is to the testbed.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
pub struct Classified {
  /// True where the line starts a heavy command outside the testbed.
  pub heavy: bool,
  /// The language that claims the heavy command.
  pub language: Option<String>,
  /// The heavy command's words.
  pub command: Option<Vec<String>>,
  /// The line that runs it in the testbed.
  pub instead: Option<String>,
}
