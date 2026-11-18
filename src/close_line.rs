//! The close lines a close stands in (docs/cli.md, 「`bmo-job job close`」 and 「Several checkouts in one
//! close」): bmo-testbed's `close-line` verb started for every checkout with commits to push, waited on until it
//! says every line is held, and kept running with its standard input open until the close lets the lines go.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

/// The variable a command run inside a close line is handed, so its seats stand next after the front
/// (bmo-testbed's `docs/testbed.md`, 「Next after the front」).
pub const NEXT_VARIABLE: &str = "BMO_TESTBED_NEXT";

/// How long the verb is given to end once its standard input is closed, before it is killed.
const LET_GO_WITHIN: Duration = Duration::from_secs(10);

/// The code bmo-testbed ends with where the harness it waited for went away before the turn came.
const HARNESS_GONE: i32 = 75;

/// The close lines held: the verb's process, which lets every line go once its standard input closes. Dropped, it
/// closes that input and waits for the verb to end, killing it where it has not ended within `LET_GO_WITHIN`.
pub struct HeldLines {
  child: Option<Child>,
  input: Option<ChildStdin>,
}

impl Drop for HeldLines {
  fn drop(&mut self) {
    drop(self.input.take());

    let Some(mut child) = self.child.take() else { return };
    let pid = child.id();
    let (ended, waited) = mpsc::channel();

    std::thread::spawn(move || {
      let _ = ended.send(child.wait());
    });

    if waited.recv_timeout(LET_GO_WITHIN).is_err() {
      // SAFETY: a signal to the verb this close started, still unwaited, so its pid is not yet reused.
      unsafe { libc::kill(pid as i32, libc::SIGKILL) };
      let _ = waited.recv();
    }
  }
}

// What the verb's two outputs tell while the close waits: a line it says of where it stands, that every line is
// held, or that an output has ended.
enum Heard {
  Said(String),
  Held,
  Ended,
}

/// Holds the close line of every checkout at `roots` through the testbed's `close-line` verb, run in `from`,
/// waiting however long the lines take and handing `say` every line the verb says of where it stands. The verb
/// orders the lines itself. Refused with what the verb said where it ended before every line was held.
pub fn hold_lines(
  testbed: &Path,
  from: &Path,
  roots: &[&PathBuf],
  say: &dyn Fn(&str),
) -> Result<HeldLines, String> {
  let mut command = Command::new(testbed);

  command.arg("close-line").current_dir(from);
  for root in roots {
    command.arg("--checkout").arg(root);
  }

  let mut child = command
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|why| format!("the close line could not be asked for: {why}"))?;
  let (heard, hearing) = mpsc::channel();

  if let Some(stdout) = child.stdout.take() {
    let heard = heard.clone();

    std::thread::spawn(move || {
      let mut lines = BufReader::new(stdout).lines();
      let first = lines.next();

      let _ = heard.send(if matches!(first, Some(Ok(_))) { Heard::Held } else { Heard::Ended });
      lines.for_each(drop);
    });
  }
  if let Some(stderr) = child.stderr.take() {
    let heard = heard.clone();

    std::thread::spawn(move || {
      for line in BufReader::new(stderr).lines().map_while(Result::ok) {
        let _ = heard.send(Heard::Said(line));
      }

      let _ = heard.send(Heard::Ended);
    });
  }
  drop(heard);

  let input = child.stdin.take();
  let mut last = String::new();

  // The wait has no clock: it ends once the lines are held, or once the verb has ended without holding them.
  for told in hearing.iter() {
    match told {
      Heard::Said(line) => {
        say(&line);
        last = line;
      }
      Heard::Held => {
        return Ok(HeldLines {
          child: Some(child),
          input,
        });
      }
      Heard::Ended => {}
    }
  }

  drop(input);
  let code = child.wait().ok().and_then(|status| status.code());

  Err(match code {
    Some(HARNESS_GONE) => {
      format!("the harness this close runs in is gone before its close line came: {last}")
    }
    Some(code) => format!("the close line could not be held, the testbed ending with {code}: {last}"),
    None => format!("the close line could not be held, the testbed ended by a signal: {last}"),
  })
}
