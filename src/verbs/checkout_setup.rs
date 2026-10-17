//! `checkout setup`: the checkout it is run in set up the way its repository sets one up (docs/verbs.md, "A
//! checkout set up"): the submodules its commit pins brought to their pins, then the repository's own setup
//! script where it names one.

use std::path::{Path, PathBuf};

use super::stand::{Answer, VerbError};
use crate::job_run::checkout::Checkout;
use crate::order_run::hold::SETUP_RUNTIME;

/// The checkout a directory stands in: the nearest directory above it holding a `.git`, or the directory
/// itself where none does.
pub fn checkout_of(from: &Path) -> PathBuf {
  from
    .ancestors()
    .find(|dir| dir.join(".git").exists())
    .map_or_else(|| from.to_path_buf(), Path::to_path_buf)
}

/// The setup of the checkout `here` stands in; the reason it stopped where it could not finish.
pub fn checkout_setup(here: &Path) -> Result<Answer, VerbError> {
  let root = checkout_of(here);
  let checkout = Checkout::local(root.to_string_lossy());
  let failed = |why: String| VerbError::Ended {
    code: 1,
    said: format!("checkout setup: {why}"),
  };

  checkout.run_setup(SETUP_RUNTIME).map_err(failed)?;

  Ok(vec![format!("checkout setup: {} is set up", root.display())].into())
}
