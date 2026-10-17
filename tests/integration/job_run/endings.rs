//! How a job's last answer is judged (docs/runs.md, "The judgement"): a reply that reported nothing, a
//! Claude Workflow walked away from, a task left running with the session asked once to settle it, and a
//! change left open — the graph left behind or a file left uncommitted — with the session asked once to
//! close it.

use bmo_job::queue::outcomes::{JOB_FAILED, JOB_LIVE_TASKS, JOB_UNCLOSED};

use super::frame::asked_to_close;
use crate::support::job::{Bench, WORKER, git};
use crate::support::turns::{FINISHED, Played, commit_work, reply, with_live_task, with_workflow};

const MID_FLIGHT: &str = "Owner, two branches are running. I will report once the build branch is done.";

async fn bench() -> (Bench, std::path::PathBuf, i64) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;

  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  (bench, dir, job)
}

#[tokio::test]
async fn a_job_that_answered_blank_failed() {
  let (bench, dir, job) = bench().await;

  bench.agent.play([Played::answered(reply("s1", ""))]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  assert!(ran.ended(JOB_FAILED, false), "{ran:?}");
}

// The workflow is still going as the turn ends, so its program owes no turn for its end.
#[tokio::test]
async fn a_job_that_walked_away_from_a_claude_workflow_is_unclosed() {
  let (bench, dir, job) = bench().await;

  bench.agent.play([Played::answered(with_workflow(reply("s1", MID_FLIGHT), None))]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  assert!(ran.ended(JOB_UNCLOSED, false), "{ran:?}");
}

#[tokio::test]
async fn a_job_that_left_a_background_task_running_is_asked_once_to_settle_it_and_ends_with_it_still_live()
{
  let (bench, dir, job) = bench().await;

  bench.agent.play([
    Played::answered(with_live_task(reply("s1", FINISHED))),
    Played::answered(with_live_task(reply("s1", FINISHED))),
  ]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  assert!(ran.ended(JOB_LIVE_TASKS, false), "{ran:?}");
  assert_eq!(bench.agent.prompts().len(), 2);
  assert!(bench.agent.prompts()[1].contains("seed the repository"), "{:?}", bench.agent.prompts());
}

#[tokio::test]
async fn a_job_that_changed_a_file_the_graph_names_and_left_the_graph_behind_is_unclosed() {
  let (bench, dir, job) = bench().await;

  bench.agent.play([
    Played::answered(reply("s1", FINISHED))
      .running(&dir, &commit_work(&["printf 'export const kept = 2;\\n' > kept.ts"])),
    Played::answered(reply("s1", FINISHED)),
  ]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  assert!(ran.ended(JOB_UNCLOSED, false), "{ran:?}");
  assert_eq!(asked_to_close(&bench.agent), 1);
}

#[tokio::test]
async fn a_job_that_left_its_own_change_uncommitted_is_unclosed_with_the_change_still_open() {
  let (bench, dir, job) = bench().await;

  bench.agent.play([
    Played::answered(reply("s1", FINISHED))
      .running(&dir, "printf 'export const kept = 3;\\n' > kept.ts\nprintf 'scratch\\n' > notes.txt"),
    Played::answered(reply("s1", FINISHED)),
  ]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  assert!(ran.ended(JOB_UNCLOSED, false), "{ran:?}");
  assert!(git(&dir, &["status", "--porcelain"]).contains("kept.ts"));
}
