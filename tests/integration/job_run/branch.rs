//! A base worker whose repository has support workers runs its job on the job's branch in its own checkout and
//! puts the checkout back on main as the job ends (docs/runs.md, "The checkout before and after").

use bmo_job::queue::outcomes::JOB_DONE;

use crate::support::job::{Bench, Ran, WORKER, git};
use crate::support::turns::{FINISHED, Played, commit_work, reply};

// A job on a base worker beside one support worker, which notes the branch it stood on and commits a file of
// its own; main as it stood before, and the job's number.
async fn base_beside_support() -> (Bench, String, i64, Ran) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");
  let main = git(&dir, &["rev-parse", "main"]);
  let seen = bench.at("branch-seen.txt");

  bench.worker(WORKER, &dir, 1).await;

  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  bench.agent.play([Played::answered(reply("s1", FINISHED)).running(
    &dir,
    &commit_work(&[
      &format!("git symbolic-ref --short HEAD > '{}'", seen.display()),
      "printf 'export const made = 1;\\n' > made.ts",
    ]),
  )]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, main, job, ran)
}

#[tokio::test]
async fn a_base_job_works_on_its_own_branch_while_main_is_checked_out_nowhere_and_ends_as_done() {
  let (bench, _, job, ran) = base_beside_support().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(
    std::fs::read_to_string(bench.at("branch-seen.txt")).unwrap().trim(),
    format!("work/job-{job}")
  );
}

#[tokio::test]
async fn it_goes_back_on_main_with_main_never_committed_on_and_its_commit_left_on_its_branch() {
  let (bench, main, job, _) = base_beside_support().await;
  let dir = bench.at("checkout");

  assert_eq!(git(&dir, &["symbolic-ref", "--short", "HEAD"]), "main");
  assert_eq!(git(&dir, &["rev-parse", "main"]), main);
  assert_eq!(git(&dir, &["show", &format!("work/job-{job}:made.ts")]), "export const made = 1;");
}
