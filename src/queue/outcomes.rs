//! How a job can end, in the words every record of a run uses, and which of those endings count against
//! the item (docs/queue.md, "How a job ends"): one list, which a job's run writes with and the queue reads
//! back, so the set cannot drift into two.

use job_codes::job_ended::JobEnding;

/// The job worked: the only ending that hands the queue on as a success.
pub const JOB_DONE: &str = "done";

/// A work order's step finished.
pub const JOB_WORK_STEP: &str = "work-step";

/// The agent answered with nothing where a report was due.
pub const JOB_FAILED: &str = "failed";

/// The agent answered and left its own background work running.
pub const JOB_LIVE_TASKS: &str = "live-tasks";

/// The agent answered while its own work was still open.
pub const JOB_UNCLOSED: &str = "unclosed";

/// The usage limit was reached.
pub const JOB_LIMIT: &str = "limit";

/// The CLI could not be read: the run never reached the work.
pub const JOB_ERROR: &str = "error";

/// The run went down with its row still open.
pub const JOB_DIED: &str = "died";

/// A person stopped the run.
pub const RUN_STOPPED: &str = "stopped";

/// The agent reported it could not answer everything.
pub const JOB_UNANSWERED: &str = "unanswered";

/// The run stopped waiting for a person's answer.
pub const JOB_BLOCKED: &str = "blocked";

/// How many times one item's work may fail before the queue stops launching it.
pub const MAX_TODO_ATTEMPTS: i64 = 3;

/// How many failures to start, one after another, before the item leaves the queue.
pub const MAX_EXECUTION_FAILURES: i64 = 3;

/// Whether an ending is one attempt at the item: the agent's own answer, and a failure.
pub fn counts_as_attempt(outcome: Option<&str>) -> bool {
  matches!(outcome, Some(JOB_FAILED | JOB_LIVE_TASKS | JOB_UNCLOSED))
}

/// Whether an ending is the execution not running at all.
pub fn counts_as_execution_failure(outcome: Option<&str>) -> bool {
  matches!(outcome, Some(JOB_ERROR | JOB_DIED))
}

/// Whether one run was a failed try at the item's own work: an attempt that ended while the work was
/// still open, which an item nobody has closed always is.
pub fn tried_the_work(outcome: Option<&str>, closed_at: Option<i64>, ended_at: Option<i64>) -> bool {
  if !counts_as_attempt(outcome) {
    return false;
  }

  match (closed_at, ended_at) {
    (Some(closed), Some(ended)) => ended < closed,
    _ => true,
  }
}

/// The code an ending is shown by, none for a word of no ending the records know.
pub fn ending(outcome: &str) -> Option<JobEnding> {
  Some(match outcome {
    JOB_DONE => JobEnding::Done,
    JOB_WORK_STEP => JobEnding::WorkStep,
    JOB_FAILED => JobEnding::Failed,
    JOB_LIVE_TASKS => JobEnding::LiveTasks,
    JOB_UNCLOSED => JobEnding::Unclosed,
    JOB_LIMIT => JobEnding::Limit,
    JOB_ERROR => JobEnding::Error,
    JOB_DIED => JobEnding::Died,
    RUN_STOPPED => JobEnding::Stopped,
    JOB_UNANSWERED => JobEnding::Unanswered,
    JOB_BLOCKED => JobEnding::Blocked,
    _ => return None,
  })
}

/// How many of an item's runs, counting back from the last that ended, failed to start: any other ending
/// breaks the run. Each run is its ending moment and its outcome, oldest first.
pub fn execution_failure_streak(runs: &[(Option<i64>, Option<String>)]) -> i64 {
  let streak = runs
    .iter()
    .filter(|(finished, outcome)| finished.is_some() && outcome.is_some())
    .rev()
    .take_while(|(_, outcome)| counts_as_execution_failure(outcome.as_deref()))
    .count();

  i64::try_from(streak).unwrap_or(i64::MAX)
}
