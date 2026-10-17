//! A landing on a work node (docs/verbs.md, "Landing a branch"): which landed paths call for the checkout's
//! setup to be run again where main moved.

use bmo_job::verbs::node_land::setup_inputs_moved;

#[test]
fn a_package_s_manifest_a_lockfile_or_the_submodules_moving_calls_for_the_setup_again() {
  for moved in ["crates/x/Cargo.toml", "Cargo.lock", "web/package.json", "web/bun.lock", ".gitmodules"] {
    assert!(setup_inputs_moved(&[moved.into()]), "{moved}");
  }

  for still in ["src/main.rs", "web/client/vendor/x/a.js", "docs/Cargo.toml.md"] {
    assert!(!setup_inputs_moved(&[still.into()]), "{still}");
  }

  assert!(setup_inputs_moved(&["src/main.rs".into(), "Cargo.lock".into()]));
}
