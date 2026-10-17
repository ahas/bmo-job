//! A stand-in for a program the service starts — systemd-run, a runner's launcher, the platform's command
//! line — written once for the test binary before any test runs it, since a program written while another
//! thread starts a process cannot be run until that process has started its own. What it does is set by the
//! test: by a file `stand-in.env` beside the name it was started by (a link in the test's own folder), or in
//! the directory it runs in, or by the variables it is started with. The file is read as shell, with the
//! program's words as its arguments, so it may set a variable by them.
//!
//! - `STAND_IN_LINES`: a file its words are added to, one run a line;
//! - `STAND_IN_INPUT`: a file its standard input is added to;
//! - `STAND_IN_SAY`: a line it writes on its standard error;
//! - `STAND_IN_OUT`: a line it writes on its standard output;
//! - `STAND_IN_EXIT`: the code it ends with, 0 where none is set.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use tempfile::NamedTempFile;

const PROGRAM: &str = r#"#!/bin/sh
here=$(dirname "$0")
if [ -f "$here/stand-in.env" ]; then . "$here/stand-in.env"; elif [ -f ./stand-in.env ]; then . ./stand-in.env; fi
[ -n "$STAND_IN_LINES" ] && echo "$*" >> "$STAND_IN_LINES"
[ -n "$STAND_IN_INPUT" ] && cat >> "$STAND_IN_INPUT"
[ -n "$STAND_IN_OUT" ] && echo "$STAND_IN_OUT"
[ -n "$STAND_IN_SAY" ] && echo "$STAND_IN_SAY" >&2
exit "${STAND_IN_EXIT:-0}"
"#;

/// The stand-in program's path, written the first time it is asked for.
pub fn program() -> &'static Path {
  static WRITTEN: OnceLock<PathBuf> = OnceLock::new();

  WRITTEN.get_or_init(|| {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let mut file = NamedTempFile::new_in(dir).expect("a file for the stand-in program is made");

    file.write_all(PROGRAM.as_bytes()).expect("the stand-in program is written");
    file
      .as_file()
      .set_permissions(fs::Permissions::from_mode(0o755))
      .expect("the stand-in program is made runnable");

    let path = dir.join(format!("bmo-job-stand-in-{}", std::process::id()));

    file.persist(&path).expect("the stand-in program is put in place");
    path
  })
}

/// What the stand-in does when run in `dir`, or through a link there: each variable set as given.
pub fn set_in(dir: &Path, variables: &[(&str, &str)]) {
  let lines: Vec<String> = variables.iter().map(|(name, value)| format!("{name}='{value}'")).collect();

  set_shell(dir, &(lines.join("\n") + "\n"));
}

/// What the stand-in does when run in `dir`, or through a link there, as shell over its words.
pub fn set_shell(dir: &Path, shell: &str) {
  fs::write(dir.join("stand-in.env"), shell).expect("the stand-in's settings are written");
}

/// The stand-in linked into `dir` under `name`, so it reads that folder's settings.
pub fn linked(dir: &Path, name: &str) -> PathBuf {
  let link = dir.join(name);

  std::os::unix::fs::symlink(program(), &link).expect("the stand-in is linked");
  link
}
