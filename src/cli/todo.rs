//! `todo` from a shell (docs/cli.md, "`todo`"): the rows' ground of whoever runs it — the job's own process,
//! the agent whose turn runs it, or the person at a shell — the shell's hands, and the providers a job's model
//! may name; each answer printed as its code's JSON on a line.

use job_codes::todo_verbs::TodoAnswer;
use queue_rows::Filer;

use crate::verbs::stand::{Answer, JobProcess, Stand, VerbError};
use crate::verbs::todo::{Beside, ShellHands, TodoArgs, providers, rows_ground, rows_units, todo_verb};

/// The variable naming the agent whose session runs a verb (docs/api.md, "Sessions").
pub const TURN_AGENT_VARIABLE: &str = "BMO_TURN_AGENT";

/// The variable naming the person at a shell.
const PERSON_VARIABLE: &str = "USER";

fn set(name: &str) -> Option<String> {
  std::env::var(name)
    .ok()
    .map(|value| value.trim().to_owned())
    .filter(|value| !value.is_empty())
}

/// One call of the TODO's verb on the stand, its answers printed one a line.
pub async fn todo(stand: &Stand, args: &TodoArgs) -> Result<Answer, VerbError> {
  let process = JobProcess::of_process();
  let agent = set(TURN_AGENT_VARIABLE);
  let person = set(PERSON_VARIABLE);
  let filer = Filer {
    agent: agent.as_deref(),
    label: None,
  };
  let units = rows_units(stand);
  let hands = ShellHands { stand };
  let providers = providers(stand).await?;
  let ground = rows_ground(stand, &process, filer, person.as_deref(), &units, &hands, &providers);
  let beside = Beside {
    source: &stand.ground.source,
    testbed: &stand.ground.testbed,
  };
  let answers: Vec<TodoAnswer> = todo_verb(&ground, &beside, args, process.work_order_step).await?;

  Ok(
    answers
      .iter()
      .map(|answer| Answer::coded(answer).lines)
      .collect::<Vec<_>>()
      .concat()
      .into(),
  )
}
