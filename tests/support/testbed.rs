//! The stand-in for bmo-testbed: a program taking the place of its binary (docs/api.md, "bmo-testbed"),
//! reached through a link in each stand's scratch directory. It records the words and the working
//! directory of every run beside its link, and answers from the fixture its verb is given there; a verb
//! with none ends with exit code 3 and says `no_fixture`.

use std::fs;
use std::io::Write;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use job_testbed::TestbedClient;
use tempfile::{NamedTempFile, TempDir};

/// The stand-in program. Each run takes the first free number under `calls` beside the link it was run
/// through, writes its words there separated by NUL and its working directory beside them, then writes
/// the fixture of its verb, keyed by the verb and, for `units` and `pending`, the word after it.
const PROGRAM: &str = r#"#!/bin/sh
here=$(dirname "$0")
[ -d "$here/calls" ] || exit 4
i=0
while ! mkdir "$here/calls/$i" 2>/dev/null; do i=$((i + 1)); done
printf '%s\0' "$@" > "$here/calls/$i/words"
pwd -P > "$here/calls/$i/dir"
key=$1
case "$1" in units | pending) key="$1-$2" ;; esac
answer="$here/answers/$key"
if [ ! -f "$answer.code" ]; then
  echo no_fixture >&2
  exit 3
fi
[ -f "$answer.out" ] && cat "$answer.out"
[ -f "$answer.err" ] && cat "$answer.err" >&2
exit "$(cat "$answer.code")"
"#;

/// The stand-in program, written once for the test binary before any stand runs it. A program written
/// while another thread starts a process cannot be run until that process has started its own program,
/// so no stand writes one of its own.
fn written() -> &'static Path {
  static PROGRAM_PATH: OnceLock<PathBuf> = OnceLock::new();
  PROGRAM_PATH.get_or_init(|| {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"));
    let mut file = NamedTempFile::new_in(dir).expect("a file for the stand-in program is made");
    file.write_all(PROGRAM.as_bytes()).expect("the stand-in program is written");
    file
      .as_file()
      .set_permissions(fs::Permissions::from_mode(0o755))
      .expect("the stand-in program is made runnable");
    let path = dir.join("bmo-testbed-stand-in");
    file.persist(&path).expect("the stand-in program is put in place");
    path
  })
}

/// One run of the stand-in: its words after the program, and the directory it ran in.
#[derive(Debug, PartialEq, Eq)]
pub struct Run {
  pub words: Vec<String>,
  pub dir: PathBuf,
}

/// The stand-in for bmo-testbed, its scratch directory removed when it is dropped.
pub struct Stand {
  dir: TempDir,
}

impl Stand {
  /// Links the stand-in program into a fresh scratch directory.
  pub fn new() -> Self {
    let dir = TempDir::new().expect("a scratch directory is made");
    fs::create_dir(dir.path().join("calls")).expect("the calls directory is made");
    fs::create_dir(dir.path().join("answers")).expect("the answers directory is made");
    symlink(written(), Self::program_in(dir.path())).expect("the stand-in program is linked");
    Self { dir }
  }

  fn program_in(dir: &Path) -> PathBuf {
    dir.join("bmo-testbed")
  }

  /// The stand-in program's path.
  pub fn program(&self) -> PathBuf {
    Self::program_in(self.dir.path())
  }

  /// The port's client, running the stand-in.
  pub fn client(&self) -> TestbedClient {
    TestbedClient::new(self.program()).expect("the stand-in's path is absolute")
  }

  /// A fresh directory in the scratch directory, standing for a checkout or holding logs.
  pub fn folder(&self, name: &str) -> PathBuf {
    let folder = self.dir.path().join(name);
    fs::create_dir_all(&folder).expect("a folder is made");
    folder
  }

  /// Gives a verb's fixture: what it writes to standard output and to standard error, and its exit
  /// code. The key is the verb, and for `units` and `pending` the verb and the next word joined by `-`.
  pub fn answer(&self, key: &str, out: &str, err: &str, code: i32) {
    let answer = self.dir.path().join("answers").join(key);
    fs::write(answer.with_extension("out"), out).expect("the fixture's output is written");
    fs::write(answer.with_extension("err"), err).expect("the fixture's error output is written");
    fs::write(answer.with_extension("code"), code.to_string()).expect("the fixture's exit code is written");
  }

  /// Every run so far, in the order they began.
  pub fn runs(&self) -> Vec<Run> {
    let calls = self.dir.path().join("calls");
    let mut runs = Vec::new();
    while calls.join(runs.len().to_string()).is_dir() {
      let run = calls.join(runs.len().to_string());
      let words = fs::read(run.join("words")).expect("a run's words are readable");
      let dir = fs::read_to_string(run.join("dir")).expect("a run's directory is readable");
      runs.push(Run {
        words: words
          .strip_suffix(b"\0")
          .expect("a run's words end with NUL")
          .split(|byte| *byte == 0)
          .map(|word| String::from_utf8(word.to_vec()).expect("a word is UTF-8"))
          .collect(),
        dir: PathBuf::from(dir.trim_end_matches('\n')),
      });
    }
    runs
  }

  /// The one run so far.
  pub fn run(&self) -> Run {
    let mut runs = self.runs();
    assert_eq!(runs.len(), 1, "one run of the stand-in, not {runs:?}");
    runs.remove(0)
  }
}
