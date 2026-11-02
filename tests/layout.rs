//! The repository's layout: only Cargo manifests at the root, no directory named after its parent, and
//! one version.

use crate::support;

use std::fs;
use std::path::Path;

use support::{ROOT, is_skipped};

/// File names that declare a package or a project to some other build tool.
const OTHER_MANIFESTS: [&str; 12] = [
  "package.json",
  "bun.lock",
  "bun.lockb",
  "deno.json",
  "pnpm-workspace.yaml",
  "pyproject.toml",
  "setup.py",
  "go.mod",
  "Gemfile",
  "pom.xml",
  "build.gradle",
  "Makefile",
];

#[test]
fn only_cargo_manifests_at_the_root() {
  let found: Vec<String> = fs::read_dir(ROOT)
    .expect("the repository root is readable")
    .map(|entry| entry.expect("a root entry is readable").file_name().to_string_lossy().into_owned())
    .filter(|name| OTHER_MANIFESTS.contains(&name.as_str()))
    .collect();
  assert!(found.is_empty(), "the root holds {found:?}; only Cargo manifests belong there");
}

fn nested_twins(dir: &Path, twins: &mut Vec<String>) {
  let parent = dir.file_name().map(|name| name.to_string_lossy().into_owned());
  for entry in fs::read_dir(dir).expect("a directory is readable") {
    let entry = entry.expect("an entry is readable");
    let name = entry.file_name().to_string_lossy().into_owned();
    if !entry.file_type().expect("an entry's type is readable").is_dir() || is_skipped(&name) {
      continue;
    }
    if parent.as_deref() == Some(name.as_str()) {
      twins.push(entry.path().display().to_string());
    }
    nested_twins(&entry.path(), twins);
  }
}

#[test]
fn no_directory_is_named_after_its_parent() {
  let mut twins = Vec::new();
  nested_twins(Path::new(ROOT), &mut twins);
  assert!(twins.is_empty(), "these directories repeat their parent's name: {twins:?}");
}

#[test]
fn the_version_file_is_the_workspace_version() {
  let version = fs::read_to_string(Path::new(ROOT).join("VERSION")).expect("VERSION is readable");
  assert_eq!(version.trim(), env!("CARGO_PKG_VERSION"));
}
