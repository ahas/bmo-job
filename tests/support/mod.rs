//! What the root package's tests share: the root and the walk over the files the repository checks read,
//! and the stand-ins for bmo-agency, bmo-gate, bmo-receiver, bmo-testbed and bmo-web the ports' contract
//! tests call.

// Each test file is its own crate and uses only part of this module.
#![allow(dead_code)]

pub mod agency;
pub mod gate;
pub mod receiver;
pub mod stub;
pub mod testbed;
pub mod web;

use std::fs;
use std::path::{Path, PathBuf};

pub const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// Directories no check reads: build output and git's own.
const SKIPPED: [&str; 2] = ["target", ".git"];

pub fn is_skipped(name: &str) -> bool {
  SKIPPED.contains(&name)
}

/// Every file under `dir`, at any depth, skipping build output and git's directory. A directory that
/// does not exist holds no files.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
  let mut files = Vec::new();
  if dir.is_dir() {
    collect(dir, &mut files);
  }
  files.sort();
  files
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
  for entry in fs::read_dir(dir).expect("a directory is readable") {
    let entry = entry.expect("an entry is readable");
    let kind = entry.file_type().expect("an entry's type is readable");
    if kind.is_dir() {
      if !is_skipped(&entry.file_name().to_string_lossy()) {
        collect(&entry.path(), files);
      }
    } else if kind.is_file() {
      files.push(entry.path());
    }
  }
}

/// A path as it reads relative to the root, for a failure's message.
pub fn shown(path: &Path) -> String {
  path.strip_prefix(ROOT).unwrap_or(path).display().to_string()
}
