//! What a job's own commits are measured against (docs/runs.md, "How a job ends"): the graph, which the job is
//! asked once to bring along and to answer for node by node; the design rules a changed screen is swept
//! against; and the commits somebody else made and the change somebody else left open, which are never held
//! against the job.

use std::path::{Path, PathBuf};

use bmo_job::queue::outcomes::{JOB_DONE, JOB_UNCLOSED};

use super::on_checkout;
use crate::support::job::{Bench, Ran, git};
use crate::support::turns::{FINISHED, Played, commit_work, reply, with_workflow};

const GRAPH_LEFT: &str = "It changed files the graph names and left the graph as it was";
const IMPACT_ASKED: &str = "Your change reaches nodes of the code dependency graph";
const UNSWEPT: &str = "sweeping them against the design rules";

// A turn that reported its work done over a Claude Workflow that ran to its end.
fn finished() -> Played {
  Played::answered(with_workflow(reply("s1", FINISHED), Some("completed")))
}

// A run over the turns given on a clean checkout.
async fn worked(plays: impl FnOnce(&Path) -> Vec<Played>) -> (Bench, PathBuf, Ran) {
  on_checkout(|_| {}, plays).await
}

fn commits(dir: &Path) -> String {
  git(dir, &["rev-list", "--count", "HEAD"])
}

#[tokio::test]
async fn a_graph_brought_up_to_date_in_the_turn_asked_for_is_a_job_that_worked() {
  let (bench, dir, ran) = worked(|dir| {
    vec![
      finished().running(dir, &commit_work(&["printf 'export const kept = 2;\\n' > kept.ts"])),
      finished().running(dir, &commit_work(&["sed -i 's/bot\\/kept/bot\\/kept-file/' graph/bot.json"])),
    ]
  })
  .await;

  assert!(
    bench.agent.prompts()[1].contains("dependency graph names"),
    "{:?}",
    bench.agent.prompts()
  );
  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(commits(&dir), "4");
  assert!(!ran.report.contains(GRAPH_LEFT), "{}", ran.report);
}

async fn graphed_in_one_commit() -> (Bench, PathBuf, Ran) {
  worked(|dir| {
    vec![finished().running(
      dir,
      &commit_work(&[
        "printf 'export const kept = 2;\\n' > kept.ts",
        "sed -i 's/bot\\/kept/bot\\/kept-file/' graph/bot.json",
      ]),
    )]
  })
  .await
}

#[tokio::test]
async fn a_job_that_brought_the_graph_along_in_the_same_commit_closed_its_change_and_worked() {
  let (_bench, dir, ran) = graphed_in_one_commit().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(!ran.report.contains(GRAPH_LEFT), "{}", ran.report);
  // The job really committed: the checkout carries its own commit on top of the two it began with.
  assert_eq!(commits(&dir), "3");
}

#[tokio::test]
async fn it_is_asked_once_to_answer_for_the_nodes_its_change_reached_and_what_it_left_is_named_under_its_report()
 {
  let (bench, _, ran) = graphed_in_one_commit().await;

  assert_eq!(bench.agent.prompts().iter().filter(|prompt| prompt.contains(IMPACT_ASKED)).count(), 1);
  assert!(ran.report.contains("its report does not answer for it"), "{}", ran.report);
  assert!(
    ran.report.contains(
      "no `graph_impact` call is recorded for this item, while your commits changed files the graph names: kept.ts"
    ),
    "{}",
    ran.report
  );
  assert!(ran.report.contains("the report has no Impact part"), "{}", ran.report);
}

// The runner reads the claim, not the screen: it names the omission and stops nothing.
#[tokio::test]
async fn a_job_that_committed_a_screen_and_said_nothing_of_the_design_rules_worked_with_the_screen_named() {
  let (_, _, ran) = worked(|dir| {
    vec![finished().running(dir, &commit_work(&["printf '<template><div /></template>\\n' > Screen.vue"]))]
  })
  .await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(
    ran
      .report
      .contains("its report says nothing of sweeping them against the design rules: Screen.vue"),
    "{}",
    ran.report
  );
}

#[tokio::test]
async fn a_job_whose_report_says_it_swept_the_screen_it_changed_is_asked_nothing() {
  let swept = "Owner, the work is done.\n\nWhat changed\nThe file was fixed.\n\nChecked\nI swept that screen against the design rules and gave its icon buttons tooltips.\n\n1 done";
  let (_, _, ran) = worked(|dir| {
    vec![
      Played::answered(with_workflow(reply("s1", swept), Some("completed")))
        .running(dir, &commit_work(&["printf '<template><div /></template>\\n' > Screen.vue"])),
    ]
  })
  .await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(!ran.report.contains(UNSWEPT), "{}", ran.report);
}

async fn elsewhere() -> (Bench, PathBuf, Ran) {
  worked(|dir| {
    vec![finished().running(dir, &commit_work(&["printf 'export const other = 1;\\n' > other.ts"]))]
  })
  .await
}

#[tokio::test]
async fn a_job_that_committed_no_screen_is_never_asked_about_a_sweep() {
  let (_, _, ran) = elsewhere().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(!ran.report.contains(UNSWEPT), "{}", ran.report);
}

#[tokio::test]
async fn a_job_that_committed_nothing_a_node_names_is_asked_nothing_of_the_graph() {
  let (bench, dir, ran) = elsewhere().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(!ran.report.contains(GRAPH_LEFT), "{}", ran.report);
  assert_eq!(commits(&dir), "3");
  assert!(!bench.agent.prompts().iter().any(|prompt| prompt.contains(IMPACT_ASKED)));
  assert!(!ran.report.contains("its report does not answer for it"), "{}", ran.report);
}

#[tokio::test]
async fn a_job_that_began_after_somebody_else_s_commit_answers_for_its_own_commits_alone() {
  let (_bench, dir, ran) = on_checkout(
    |dir| {
      // A change to the file the graph names, committed before this job ever started.
      std::fs::write(dir.join("kept.ts"), "export const kept = 3;\n").unwrap();
      git(dir, &["commit", "-qam", "someone else"]);
    },
    |_| vec![finished()],
  )
  .await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(!ran.report.contains(GRAPH_LEFT), "{}", ran.report);
  // The commit that touched the file the graph names is there; it is simply not this job's.
  assert_eq!(commits(&dir), "3");
}

#[tokio::test]
async fn a_tracked_change_open_before_the_job_and_kept_byte_for_byte_does_not_fail_a_finished_job() {
  let (_bench, dir, ran) = on_checkout(
    |dir| std::fs::write(dir.join("kept.ts"), "export const kept = 42;\n").unwrap(),
    |_| vec![finished()],
  )
  .await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(std::fs::read_to_string(dir.join("kept.ts")).unwrap(), "export const kept = 42;\n");
}

// The checkout is shared: what one job or a person left open is not what the next one did.
async fn others_left() -> (Bench, PathBuf, Ran) {
  on_checkout(
    |dir| {
      std::fs::write(dir.join("kept.ts"), "export const kept = 42;\n").unwrap();
      std::fs::write(dir.join("theirs.txt"), "a note somebody left\n").unwrap();
    },
    |_| vec![finished()],
  )
  .await
}

#[tokio::test]
async fn neither_a_tracked_change_nor_a_file_git_never_saw_left_by_others_is_held_against_the_job() {
  let (_, _, ran) = others_left().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
}

#[tokio::test]
async fn the_report_says_whose_the_leftovers_are_instead_of_naming_them_as_the_job_s() {
  let (_, _, ran) = others_left().await;

  assert!(
    ran
      .report
      .contains("Uncommitted changes of earlier work were kept as they stood: kept.ts, theirs.txt"),
    "{}",
    ran.report
  );
  assert!(!ran.report.contains("Uncommitted changes are left"), "{}", ran.report);
}

// The check still catches what this job itself left open, in the same checkout and beside somebody else's.
#[tokio::test]
async fn a_further_edit_to_a_file_somebody_else_had_open_is_the_job_s_own_and_refuses_it() {
  let (_, _, ran) = on_checkout(
    |dir| std::fs::write(dir.join("kept.ts"), "export const kept = 42;\n").unwrap(),
    |dir| vec![finished().running(dir, "printf 'export const mine = 1;\\n' >> kept.ts")],
  )
  .await;

  assert!(ran.ended(JOB_UNCLOSED, false), "{ran:?}");
  assert!(ran.report.contains("Uncommitted changes are left: kept.ts"), "{}", ran.report);
}
