//! The job's prompt as the runner composes it before the work starts (docs/runs.md, "The prompt"): the
//! worker's preamble as its row stands, the job's text as it was handed through the sanitiser and to no
//! translator, and the trailer the job is held to; the worker's bench made ready before the turn.

use std::path::{Path, PathBuf};

use bmo_job::job_run::{WORK_SESSION_RESUME, WORK_STEP_PHASE};

use crate::support::job::{Bench, Given, WORKER};
use crate::support::turns::{FINISHED, Played, reply};

const TASK: &str = "Fix the one thing in this repository, then report.";

// What the run asked: the prompt its first turn was sent, and its upkeep calls so far.
struct Side {
  prompt: String,
  sanitised: Vec<String>,
  upkeep: Vec<String>,
}

// A job on the worker the bench was given, in `cwd`, on the words `given` makes, its first prompt read back.
async fn side(bench: &Bench, cwd: &Path, given: impl FnOnce(Given) -> Given) -> Side {
  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  bench.agent.play([Played::answered(reply("s1", FINISHED))]);
  bench
    .run(&given(Given {
      task: TASK.into(),
      ..bench.given(job, cwd)
    }))
    .await;

  Side {
    prompt: bench.agent.prompts().first().cloned().unwrap_or_default(),
    sanitised: bench.agent.sanitised(),
    upkeep: bench.agent.upkeep(),
  }
}

async fn worded(bench: &Bench, name: &str, preamble: Option<&str>, dev: Option<&str>) {
  bench
    .scene
    .run(
      r#"UPDATE "workers" SET "preamble" = $2, "dev_command" = $3 WHERE "name" = $1"#,
      vec![name.into(), preamble.map(str::to_owned).into(), dev.map(str::to_owned).into()],
    )
    .await;
}

async fn checkout() -> (Bench, PathBuf) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  (bench, dir)
}

#[tokio::test]
async fn a_worker_s_preamble_and_dev_server_on_a_base_checkout_working_on_its_item_s_branch() {
  let (bench, dir) = checkout().await;

  bench.worker(WORKER, &dir, 1).await;
  worded(&bench, WORKER, Some("Keep the console in mind."), Some("bun run dev")).await;

  let seen = side(&bench, &dir, |given| given).await;

  assert!(seen.prompt.contains("[Worker preamble"), "{}", seen.prompt);
  assert!(seen.prompt.contains("in this worker's own checkout"), "{}", seen.prompt);
  assert!(seen.prompt.contains("running bun run dev."), "{}", seen.prompt);
  assert!(seen.prompt.contains("Keep the console in mind."), "{}", seen.prompt);
  assert!(seen.prompt.contains(TASK), "{}", seen.prompt);
  // The bench, and what a job before this one left of a dev server there, is bmo-agency's to make ready before
  // the work starts.
  assert!(seen.upkeep.contains(&"make_bench".to_owned()), "{:?}", seen.upkeep);
}

#[tokio::test]
async fn a_support_worker_is_told_its_base_and_the_branch_its_worktree_is_on() {
  let (bench, dir) = checkout().await;
  let cwd = bench.at("support-1");

  bench.worker("base", &dir, 1).await;
  worded(&bench, "base", None, Some("bun run dev")).await;
  bench.support_worker(WORKER, &cwd, "base", 1).await;

  let seen = side(&bench, &cwd, |given| given).await;

  assert!(seen.prompt.contains("This worker is support worker 1 of base"), "{}", seen.prompt);
  assert!(seen.prompt.contains("with BMO_WORKER_SLOT=1"), "{}", seen.prompt);
}

#[tokio::test]
async fn a_worker_whose_directory_is_no_repository_is_told_so() {
  let bench = Bench::new().await;
  let cwd = bench.at("plain");

  std::fs::create_dir_all(&cwd).unwrap();
  bench.worker(WORKER, &cwd, 0).await;

  let seen = side(&bench, &cwd, |given| given).await;

  assert!(seen.prompt.contains("is not a git repository"), "{}", seen.prompt);
}

// Words in a script the worker does not read, of so many syllables, made from their code points.
fn foreign(syllables: u32) -> String {
  (0..syllables).map(|at| char::from_u32(0xac00 + at * 29).unwrap()).collect()
}

#[tokio::test]
async fn the_job_s_own_text_with_the_owner_s_quote_goes_to_the_worker_as_handed_and_to_no_translator() {
  let (bench, dir) = checkout().await;
  let task = [
    "Item #7 Fix the gate, as the owner asked:".to_owned(),
    format!("```text\n{} {}\n```", foreign(6), foreign(9)),
    format!("Bring the gate back, and have the {} screen say when it does not answer.", foreign(3)),
    format!("{} {} {}", foreign(12), foreign(20), foreign(8)),
  ]
  .join("\n\n");

  bench.worker(WORKER, &dir, 0).await;

  let seen = side(&bench, &dir, |given| Given {
    task: task.clone(),
    ..given
  })
  .await;

  assert!(seen.prompt.contains(&task), "{}", seen.prompt);
  // The sanitiser was asked once, at the first asking, with the text as it stood.
  assert_eq!(seen.sanitised, [task]);
  assert!(!seen.prompt.contains("Translate into English."), "{}", seen.prompt);
}

#[tokio::test]
async fn a_work_order_s_middle_step_is_told_what_a_step_is() {
  let (bench, dir) = checkout().await;

  bench.worker(WORKER, &dir, 0).await;

  let seen = side(&bench, &dir, |given| Given {
    phase: WORK_STEP_PHASE.into(),
    word: WORK_SESSION_RESUME.into(),
    ..given
  })
  .await;

  assert!(seen.prompt.contains("This is one bot-managed co-work step."), "{}", seen.prompt);
  assert!(!seen.prompt.contains("[Worker preamble"), "{}", seen.prompt);
}
