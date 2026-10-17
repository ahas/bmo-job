//! A runner started on the owner's own words (docs/api.md, "Runners"): its launcher, the platform's
//! command line run with the runner's name as its verb, with the owner's words on its standard input and
//! never in its arguments, which stand in the process list. Only the launcher is waited for — it exits once
//! its unit is up — and never the run, which reports to the room by itself.
//!
//! Which runners there are, and the depths a search takes, are the policy's research keys: each key is a
//! runner, with a depth after its colon, the first its default.

use std::path::Path;
use std::time::Duration;

use job_codes::{JobError, TaskRefusal};
use job_policy::research::Research;
use tokio::io::AsyncWriteExt;

use crate::Refusal;

// How long the launcher itself is given. Its unit is up long before it is done, so a launcher ended here
// has already started the run.
const LAUNCH_WAIT: Duration = Duration::from_secs(30);

// What a run launched from here records as its launcher: the one launcher that is not a room.
const CONSOLE_LAUNCHER: &str = "console";

/// One runner, with the depths it is launched at, the first its default; none for one with no depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runner {
  pub name: String,
  pub depths: Vec<String>,
}

fn refused(refusal: TaskRefusal) -> Refusal {
  JobError::from(refusal).into()
}

/// Every runner the research keys name, in their order.
pub fn runners(research: &Research) -> Vec<Runner> {
  let mut found: Vec<Runner> = vec![];

  for key in &research.keys {
    let (name, depth) =
      key.split_once(':').map_or((key.as_str(), None), |(name, depth)| (name, Some(depth)));
    let at = match found.iter().position(|runner| runner.name == name) {
      Some(at) => at,
      None => {
        found.push(Runner {
          name: name.to_owned(),
          depths: vec![],
        });
        found.len() - 1
      }
    };

    if let Some(depth) = depth {
      found[at].depths.push(depth.to_owned());
    }
  }

  found
}

/// What the launcher of a runner is told after its own name, or the refusal of what it was asked.
pub fn command(
  research: &Research,
  runner_name: &str,
  depth: Option<&str>,
  chat: i64,
) -> Result<Vec<String>, Refusal> {
  let known = runners(research);
  let Some(runner) = known.iter().find(|runner| runner.name == runner_name) else {
    return Err(refused(TaskRefusal::RunnerUnknown {
      runner: runner_name.to_owned(),
      runners: known.into_iter().map(|runner| runner.name).collect(),
    }));
  };
  let mut line = vec!["--chat".to_owned(), chat.to_string()];

  // A depth not one of the runner's is refused rather than quietly run at the shallowest: the depth is
  // what somebody asked for. One the owner picked and pressed was insisted on, so it waits for the pool.
  if let Some(default) = runner.depths.first() {
    let depth = match depth {
      None => default.as_str(),
      Some(depth) if runner.depths.iter().any(|one| one == depth) => depth,
      Some(_) => {
        return Err(refused(TaskRefusal::RunnerDepthWrong {
          depths: runner.depths.clone(),
        }));
      }
    };

    line.extend(["--depth".into(), depth.to_owned(), "--demand".into(), "explicit".into()]);
  }

  line.extend(["--by".into(), CONSOLE_LAUNCHER.into()]);

  Ok(line)
}

/// One runner started on the owner's words in the owner's chat: `launcher` run in `cwd` with the runner's
/// name and its line, and waited for no longer than its while.
pub async fn launch(
  research: &Research,
  launcher: &Path,
  cwd: &Path,
  owner_chat: i64,
  runner: &str,
  depth: Option<&str>,
  text: &str,
) -> Result<(), Refusal> {
  let line = command(research, runner, depth, owner_chat)?;
  let failed = |why: String| {
    refused(TaskRefusal::RunnerNotLaunched {
      runner: runner.to_owned(),
      why,
    })
  };
  let mut child = tokio::process::Command::new(launcher)
    .arg(runner)
    .args(&line)
    .current_dir(cwd)
    .stdin(std::process::Stdio::piped())
    .kill_on_drop(true)
    .spawn()
    .map_err(|why| failed(format!("it could not start: {why}")))?;

  if let Some(mut input) = child.stdin.take() {
    input
      .write_all(text.trim().as_bytes())
      .await
      .map_err(|why| failed(format!("it did not take the words: {why}")))?;
  }

  match tokio::time::timeout(LAUNCH_WAIT, child.wait()).await {
    Ok(Ok(status)) if status.success() => Ok(()),
    Ok(Ok(status)) => Err(failed(format!("it exited with code {}", status.code().unwrap_or(-1)))),
    Ok(Err(why)) => Err(failed(format!("it could not be waited on: {why}"))),
    Err(_) => {
      let _ = child.kill().await;

      Err(failed(format!("it ran past {} seconds", LAUNCH_WAIT.as_secs())))
    }
  }
}
