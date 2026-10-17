//! A job that answered with its own work still open (docs/runs.md, "How a job ends"): a Claude Workflow walked
//! away from or never heard from again, a room's silence marker for an answer, a change left in the checkout,
//! a scratch file, an aside ahead of the report, a graph left behind, and the one turn the session is asked to
//! close it in. Each scenario is one run, and each case reads one side of it.

use std::path::PathBuf;

use bmo_job::queue::outcomes::{JOB_DONE, JOB_FAILED, JOB_UNCLOSED, counts_as_attempt};

use super::frame::asked_to_close;
use super::on_checkout;
use crate::support::job::{Bench, Ran, git};
use crate::support::turns::{FINISHED, Played, commit_work, reply, with_workflow};

const MID_FLIGHT: &str = "Owner, two branches are running. I will report once the build branch is done.";

const CHANGE_OPEN: &str = "Your answer was not sent. Your own change is still open";

async fn scenario(plays: impl FnOnce(&std::path::Path) -> Vec<Played>) -> (Bench, PathBuf, Ran) {
  on_checkout(|_| {}, plays).await
}

async fn walked_away() -> (Bench, PathBuf, Ran) {
  scenario(|_| vec![Played::answered(with_workflow(reply("s1", MID_FLIGHT), Some("stopped")))]).await
}

#[tokio::test]
async fn a_workflow_walked_away_from_is_no_done_job_and_counts_as_an_attempt_at_the_item() {
  let (_, _, ran) = walked_away().await;

  assert!(ran.ended(JOB_UNCLOSED, false), "{ran:?}");
  assert!(counts_as_attempt(ran.outcome.as_deref()));
}

#[tokio::test]
async fn the_report_names_the_workflow_left_with_the_job_s_own_words_under_it() {
  let (bench, _, ran) = walked_away().await;

  for said in [ran.report.clone(), bench.told()] {
    assert!(said.contains("Workflows not finished: two-answers"), "{said}");
    assert!(said.contains("What the job left:"), "{said}");
    assert!(said.contains(MID_FLIGHT), "{said}");
  }
}

// The change-closing question is worth a turn only where the thing asked for is still the session's to do. A
// Claude Workflow died with the run, so no turn can bring it back and none is spent.
#[tokio::test]
async fn no_turn_is_asked_of_a_job_whose_workflow_is_gone_with_the_run() {
  let (bench, _, _) = walked_away().await;

  assert_eq!(bench.agent.prompts().len(), 1);
  assert_eq!(asked_to_close(&bench.agent), 0);
}

// The run was killed at its own result frame before anything could be said about what it launched.
#[tokio::test]
async fn silence_about_a_workflow_is_no_finished_workflow() {
  let (_, _, ran) =
    scenario(|_| vec![Played::answered(with_workflow(reply("s1", MID_FLIGHT), None))]).await;

  assert!(ran.ended(JOB_UNCLOSED, false), "{ran:?}");
  assert!(ran.report.contains("Workflows not finished: two-answers"), "{}", ran.report);
}

// The marker a room's session answers with when it has nothing to say satisfies every gate that asks what the
// run left behind; the reply itself is the only thing that says the work never happened.
async fn silent() -> (Bench, PathBuf, Ran) {
  scenario(|_| vec![Played::answered(reply("quiet-session", "<<SKIP>>"))]).await
}

#[tokio::test]
async fn an_answer_of_nothing_but_the_room_s_silence_marker_is_a_failure() {
  let (_, _, ran) = silent().await;

  assert!(ran.ended(JOB_FAILED, false), "{ran:?}");
}

#[tokio::test]
async fn the_silent_job_s_item_stays_runnable_since_the_ending_counts_as_an_attempt() {
  let (_, _, ran) = silent().await;

  assert!(counts_as_attempt(ran.outcome.as_deref()));
}

#[tokio::test]
async fn the_silent_job_s_report_quotes_what_the_run_answered_with() {
  let (bench, _, ran) = silent().await;

  for said in [ran.report.clone(), bench.told()] {
    assert!(said.contains("`<<SKIP>>`"), "{said}");
    assert!(said.contains("The job ended without doing any work."), "{said}");
  }
}

async fn change_left() -> (Bench, PathBuf, Ran) {
  scenario(|dir| {
    vec![
      Played::answered(with_workflow(reply("s1", FINISHED), Some("completed")))
        .running(dir, "printf 'export const kept = 2;\\n' > kept.ts"),
      Played::answered(with_workflow(reply("s1", FINISHED), Some("completed"))),
    ]
  })
  .await
}

#[tokio::test]
async fn a_change_to_a_file_git_tracks_left_open_refuses_the_job_however_well_it_reported() {
  let (_, _, ran) = change_left().await;

  assert!(ran.ended(JOB_UNCLOSED, false), "{ran:?}");
}

#[tokio::test]
async fn the_report_says_which_file_was_left_and_nothing_of_a_workflow() {
  let (_, _, ran) = change_left().await;

  assert!(ran.report.contains("Uncommitted changes are left: kept.ts"), "{}", ran.report);
  assert!(!ran.report.contains("Workflows not finished"), "{}", ran.report);
}

// Refusing a job that really finished is the worse of the two mistakes, and a file git has never seen is a
// scratch file as often as it is work.
#[tokio::test]
async fn a_file_git_never_saw_is_named_in_the_report_and_refuses_nothing() {
  let (_, _, ran) = scenario(|dir| {
    vec![
      Played::answered(with_workflow(reply("s1", FINISHED), Some("completed")))
        .running(dir, "printf 'a passing note\\n' > scratch.txt"),
    ]
  })
  .await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(ran.report.contains("Uncommitted changes are left: scratch.txt"), "{}", ran.report);
}

#[tokio::test]
async fn a_workflow_that_ran_to_the_end_over_a_clean_tree_is_a_job_that_worked() {
  let (bench, _, ran) =
    scenario(|_| vec![Played::answered(with_workflow(reply("s1", FINISHED), Some("completed")))]).await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(bench.told().contains("Owner, the work is done."), "{}", bench.told());
  assert!(!ran.report.contains("without closing its work"), "{}", ran.report);
}

// The agent is handed the whole answer and tells the owner in its own words, so nothing is cut from it on the
// way.
#[tokio::test]
async fn the_agent_is_handed_the_whole_answer_with_the_aside_ahead_of_the_report() {
  let aside = "Item #807 is done, both commits landed on main. Here's the report:";
  let (bench, _, ran) = scenario(|_| {
    vec![Played::answered(with_workflow(
      reply("s1", &format!("{aside}\n\n---\n\n{FINISHED}")),
      Some("completed"),
    ))]
  })
  .await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(ran.report.contains("Here's the report"), "{}", ran.report);
  assert!(bench.told().contains("Here's the report"), "{}", bench.told());
  assert!(bench.told().contains("Owner, the work is done."), "{}", bench.told());
}

// What the runner measures is the job's own commits, after the point the checkout stood at when the run began,
// so nothing anybody committed before it is counted against it.
async fn graph_left() -> (Bench, PathBuf, Ran) {
  scenario(|dir| {
    vec![
      Played::answered(with_workflow(reply("s1", FINISHED), Some("completed")))
        .running(dir, &commit_work(&["printf 'export const kept = 2;\\n' > kept.ts"])),
    ]
  })
  .await
}

#[tokio::test]
async fn a_commit_to_code_the_graph_names_with_the_graph_left_alone_is_refused_however_clean_the_checkout()
{
  let (_bench, dir, ran) = graph_left().await;

  assert!(ran.ended(JOB_UNCLOSED, false), "{ran:?}");
  assert!(counts_as_attempt(ran.outcome.as_deref()));
  assert_eq!(git(&dir, &["status", "--porcelain"]), "");
}

#[tokio::test]
async fn the_report_names_the_file_the_graph_had_spoken_for() {
  let (_, _, ran) = graph_left().await;

  assert!(
    ran
      .report
      .contains("It changed files the graph names and left the graph as it was: kept.ts"),
    "{}",
    ran.report
  );
  assert!(ran.report.contains("What the job left:"), "{}", ran.report);
  assert!(!ran.report.contains("Uncommitted changes are left"), "{}", ran.report);
}

// Asked once, and refused where the asking changed nothing: the judgement after it reads the graph again rather
// than the answer.
#[tokio::test]
async fn the_job_that_left_the_graph_was_asked_once_naming_what_it_left() {
  let (bench, _, _) = graph_left().await;
  let prompts = bench.agent.prompts();

  assert_eq!(asked_to_close(&bench.agent), 1);
  assert!(prompts.iter().any(|prompt| prompt.contains(CHANGE_OPEN)), "{prompts:?}");
  assert!(prompts.iter().any(|prompt| prompt.contains("dependency graph names")), "{prompts:?}");
}

// Nothing of a change left open died with the turn, so the session is asked once, and what it does then is
// what the judgement reads.
async fn closed_when_asked() -> (Bench, PathBuf, Ran) {
  scenario(|dir| {
    vec![
      Played::answered(with_workflow(reply("s1", FINISHED), Some("completed")))
        .running(dir, "printf 'export const kept = 2;\\n' > kept.ts"),
      // The turn the runner asks for commits it and brings the graph along.
      Played::answered(with_workflow(reply("s1", FINISHED), Some("completed")))
        .running(dir, &commit_work(&["sed -i 's/bot\\/kept/bot\\/kept-file/' graph/bot.json"])),
    ]
  })
  .await
}

#[tokio::test]
async fn the_session_is_told_its_answer_was_not_sent_and_named_what_it_left() {
  let (bench, _, _) = closed_when_asked().await;
  let asking = bench.agent.prompts()[1].clone();

  assert!(asking.contains(CHANGE_OPEN), "{asking}");
  assert!(asking.contains("kept.ts"), "{asking}");
}

#[tokio::test]
async fn the_judgement_reads_the_checkout_again_so_a_job_that_closed_its_change_worked() {
  let (_bench, dir, ran) = closed_when_asked().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]), "3");
  assert!(!ran.report.contains("without closing its work"), "{}", ran.report);
}
