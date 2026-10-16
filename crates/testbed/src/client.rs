//! The port's client: each call runs bmo-testbed's binary with the words the API document gives it,
//! in the checkout the call names, and reads its exit code and its output as that document says.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{ExitStatus, Stdio};

use serde::de::DeserializeOwned;
use tokio::process::Command;

use crate::{
  Answer, Check, Classified, Close, CloseTurn, Count, Deployed, Outcome, Pending, PendingDeploy, PendingOf,
  Relinked, Suite, SuiteLog, Testbed, TestbedError, UnitPlan,
};

/// The exit code of a refusal.
const REFUSED: i32 = 2;
/// The exit code of a turn that did not come.
const NOT_RUN: i32 = 75;

/// The calls on bmo-testbed, made by running its binary at the path given.
#[derive(Clone, Debug)]
pub struct TestbedClient {
  program: PathBuf,
}

impl TestbedClient {
  /// A client running the binary at `program`, which is an absolute path.
  pub fn new(program: impl Into<PathBuf>) -> Answer<Self> {
    let program = program.into();
    if !program.is_absolute() {
      return Err(TestbedError::NotAProgram {
        program: program.display().to_string(),
      });
    }
    Ok(Self { program })
  }

  fn command(&self, checkout: Option<&Path>, words: Vec<OsString>) -> Command {
    let mut command = Command::new(&self.program);
    command.args(words).stdin(Stdio::null());
    if let Some(checkout) = checkout {
      command.current_dir(checkout);
    }
    command
  }

  /// Runs a call answering a structure: its standard output read as `T` where the exit code is one
  /// of `answers`.
  async fn json<T: DeserializeOwned>(
    &self,
    call: &'static str,
    checkout: Option<&Path>,
    words: Vec<OsString>,
    answers: &[i32],
  ) -> Answer<T> {
    let output = self
      .command(checkout, words)
      .stdout(Stdio::piped())
      .stderr(Stdio::piped())
      .output()
      .await
      .map_err(unstarted(call))?;

    let said = || last_line(&output.stderr).or_else(|| last_line(&output.stdout)).unwrap_or_default();
    match output.status.code() {
      Some(code) if answers.contains(&code) => {
        serde_json::from_slice(&output.stdout).map_err(|error| TestbedError::Malformed {
          call,
          error: error.to_string(),
        })
      }
      Some(REFUSED) => Err(TestbedError::Refused { call, said: said() }),
      code => Err(TestbedError::Broke {
        call,
        code,
        said: said(),
      }),
    }
  }

  /// Runs a call that runs something, its standard output and standard error written together to
  /// `log`, and reads its exit code as an outcome.
  async fn logged(
    &self,
    call: &'static str,
    checkout: &Path,
    words: Vec<OsString>,
    log: &Path,
  ) -> Answer<Outcome> {
    let file = tokio::fs::File::create(log).await.map_err(unstarted(call))?.into_std().await;
    let twin = file.try_clone().map_err(unstarted(call))?;
    let status = self
      .command(Some(checkout), words)
      .stdout(file)
      .stderr(twin)
      .status()
      .await
      .map_err(unstarted(call))?;

    match status.code() {
      Some(0) => Ok(Outcome::Passed),
      Some(1) => Ok(Outcome::Failed),
      Some(NOT_RUN) => Ok(Outcome::NotRun),
      code => {
        let said = tokio::fs::read(log)
          .await
          .ok()
          .and_then(|written| last_line(&written))
          .unwrap_or_default();
        Err(match code {
          Some(REFUSED) => TestbedError::Refused { call, said },
          code => TestbedError::Broke { call, code, said },
        })
      }
    }
  }
}

fn unstarted(call: &'static str) -> impl Fn(std::io::Error) -> TestbedError {
  move |error| TestbedError::Unstarted {
    call,
    error: error.to_string(),
  }
}

/// The last line holding anything in what a verb wrote, as it stands.
fn last_line(written: &[u8]) -> Option<String> {
  String::from_utf8_lossy(written)
    .lines()
    .rev()
    .find(|line| !line.trim().is_empty())
    .map(str::to_owned)
}

fn words<const N: usize>(fixed: [&str; N]) -> Vec<OsString> {
  fixed.into_iter().map(OsString::from).collect()
}

/// Adds `--<flag> <value>` where the value is given.
fn optional(words: &mut Vec<OsString>, flag: &str, value: Option<impl ToString>) {
  if let Some(value) = value {
    words.push(flag.into());
    words.push(value.to_string().into());
  }
}

fn closed(status: ExitStatus) -> Answer<CloseTurn> {
  match status.code() {
    Some(NOT_RUN) => Ok(CloseTurn::NotRun),
    Some(code) => Ok(CloseTurn::Ran(code)),
    None => Err(TestbedError::Broke {
      call: "close",
      code: None,
      said: String::new(),
    }),
  }
}

impl Testbed for TestbedClient {
  async fn check(&self, check: &Check) -> Answer<Outcome> {
    let mut said = words(["check"]);
    optional(&mut said, "--since", check.since.as_ref());
    self.logged("check", &check.checkout, said, &check.log).await
  }

  async fn count(&self, count: &Count) -> Answer<Outcome> {
    let mut said = words(["count"]);
    optional(&mut said, "--since", count.since.as_ref());
    self.logged("count", &count.checkout, said, &count.log).await
  }

  async fn suite(&self, suite: &Suite) -> Answer<Outcome> {
    let mut said = words(["suite"]);
    optional(&mut said, "--since", suite.since.as_ref());
    if suite.again {
      said.push("--again".into());
    }
    optional(&mut said, "--guard", suite.guard);
    optional(&mut said, "--file-guard", suite.file_guard);
    optional(&mut said, "--turn-wait", suite.turn_wait);
    said.extend(suite.words.iter().map(OsString::from));
    self.logged("suite", &suite.checkout, said, &suite.log).await
  }

  async fn read_log(&self, log: &Path) -> Answer<SuiteLog> {
    let said = vec!["log".into(), log.into(), "--json".into()];
    self.json("read_log", None, said, &[0, 1]).await
  }

  async fn close(&self, close: &Close) -> Answer<CloseTurn> {
    let mut said = words(["close", "--"]);
    said.extend(close.command.iter().map(OsString::from));
    let status = self.command(Some(&close.checkout), said).status().await.map_err(unstarted("close"))?;
    closed(status)
  }

  async fn classify(&self, line: &str) -> Answer<Classified> {
    self.json("classify", None, words(["classify", "--json", line]), &[0, 1]).await
  }

  async fn plan_units(&self, checkout: &Path, commit: &str, hold: bool) -> Answer<UnitPlan> {
    let mut said = words(["units", "plan", "--to", commit]);
    if hold {
      said.push("--hold".into());
    }
    said.push("--json".into());
    self.json("plan_units", Some(checkout), said, &[0, 1]).await
  }

  async fn relink_units(&self, checkout: &Path) -> Answer<Relinked> {
    self
      .json("relink_units", Some(checkout), words(["units", "relink", "--json"]), &[0, 1])
      .await
  }

  async fn record_pending(&self, pending: &PendingDeploy) -> Answer<Pending> {
    let mut said = words(["pending", "add", "--commit", &pending.commit]);
    optional(&mut said, "--item", pending.item);
    optional(&mut said, "--worker", pending.worker.as_ref());
    optional(&mut said, "--job", pending.job);
    said.extend(words(["--reason", &pending.reason, "--json"]));
    self.json("record_pending", Some(&pending.checkout), said, &[0]).await
  }

  async fn pending(&self, of: PendingOf) -> Answer<Vec<Pending>> {
    let mut said = words(["pending", "list"]);
    match of {
      PendingOf::All => {}
      PendingOf::Job(job) => optional(&mut said, "--job", Some(job)),
      PendingOf::Item(item) => optional(&mut said, "--item", Some(item)),
    }
    said.push("--json".into());
    self.json("pending", None, said, &[0]).await
  }

  async fn deployed(&self, checkout: &Path) -> Answer<Option<Deployed>> {
    self.json("deployed", Some(checkout), words(["deployed", "--json"]), &[0]).await
  }
}
