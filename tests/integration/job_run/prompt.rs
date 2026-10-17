//! What a job is told, the prefix its session stands on, and the words added to its item (docs/runs.md, "The
//! prompt", "The turns" and "As the job ends"): its own text and then the trailer; the session handed over as
//! every job ends, the prefix it changed named; the item a handover belongs to; the agent's change to the brief
//! asked over once the turn answered, and never a person's words; and an item left open asked about once.

use std::path::{Path, PathBuf};

use bmo_job::queue::outcomes::JOB_DONE;
use futures_util::future::BoxFuture;
use job_agency::{Handover, Resume};
use queue_read::worker_lines::CHANGE_NOTE;

use super::on_checkout;
use crate::support::job::{Bench, Given, Ran, WORKER, git};
use crate::support::turns::{Act, Agent, FINISHED, MAPPED, Played, TRAILER, closing, commit_work, reply};

const TASK: &str = "There is one thing to do in this repository. Report when it is done.";
const ANSWER: &str = "Owner, the work is done.\n\nWhat changed\nThe panel was made taller.\n\n1 done";

#[tokio::test]
async fn a_job_is_handed_its_own_text_and_then_the_trailer() {
  let (bench, _, ran) =
    on_checkout(|_| {}, |_| vec![Played::answered(reply("plain-session", ANSWER))]).await;
  let handed = &bench.agent.prompts()[0];

  assert!(ran.success, "{ran:?}");
  assert!(handed.find(TASK).unwrap() < handed.find(TRAILER).unwrap(), "{handed}");
}

// Every job hands the session it ran in over as it ends, once its report has gone out. A job that committed
// a change to the prefix is handed over the same way, and why is read off its own commits.
async fn changed_prefix() -> (Bench, PathBuf, Ran) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  Agent::set(&bench.agent.prefix, vec!["CLAUDE.md".to_owned()]);
  bench.worker(WORKER, &dir, 0).await;

  let job = bench.job(None, WORKER, "bmo-task-timed").await;

  bench.agent.play([Played::answered(reply("prefix-session", FINISHED))
    .running(&dir, &commit_work(&["printf '# how to work here\\n' > CLAUDE.md"]))]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, dir, ran)
}

fn summary_of(session: &str, dir: &Path) -> Handover {
  Handover {
    model: MAPPED.into(),
    cwd: dir.display().to_string(),
    thread: None,
    session: Some(session.into()),
    note: None,
  }
}

#[tokio::test]
async fn a_job_that_committed_a_change_to_its_prefix_ends_as_the_work_it_was() {
  let (_bench, dir, ran) = changed_prefix().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(git(&dir, &["rev-list", "--count", "HEAD"]), "3");
}

#[tokio::test]
async fn the_changed_prefix_is_handed_over_through_a_summary_without_another_turn_of_the_old_session() {
  let (bench, dir, _) = changed_prefix().await;

  // What touches the prefix is asked of the job's own commits, and it named the file it changed.
  assert_eq!(bench.agent.prefix_asked(), [vec!["CLAUDE.md".to_owned()]]);
  assert_eq!(bench.agent.handed(), [summary_of("prefix-session", &dir)]);
  assert_eq!(bench.agent.prompts().len(), 1);
}

#[tokio::test]
async fn a_job_that_committed_nothing_of_its_prefix_hands_its_session_over_all_the_same() {
  let (bench, dir, ran) = on_checkout(
    |_| {},
    |dir| {
      vec![
        Played::answered(reply("plain-session", FINISHED))
          .running(dir, &commit_work(&["printf 'export const other = 1;\\n' > other.ts"])),
      ]
    },
  )
  .await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(bench.agent.prefix_asked(), [vec!["other.ts".to_owned()]]);
  // The session was recorded as the one the job answered on, then handed over through a summary.
  assert_eq!(bench.agent.recorded()[0].session_id.as_deref(), Some("plain-session"));
  assert_eq!(bench.agent.handed(), [summary_of("plain-session", &dir)]);
  assert_eq!(bench.agent.prompts().len(), 1);
}

// The note a handover leaves is the item's own: two items taking turns on one worker must not carry one item's
// thread into the other's job, and the next job of an item goes on the thread its last job left.
struct Turned {
  asked: Option<String>,
  handed: Option<String>,
  success: bool,
}

async fn items_taking_turns() -> [Turned; 3] {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  bench.scene.item(1, "Console items").await;
  bench.scene.item(2, "Memory links").await;

  let mut turned = vec![];

  for (todo, session) in [(1, "first-item-session"), (2, "second-item-session"), (2, "second-item-again")] {
    let job = bench.job(Some(todo), WORKER, "bmo-task-timed").await;
    let (asked, handed) = (bench.agent.asked().len(), bench.agent.handed().len());

    bench
      .scene
      .run(r#"UPDATE "todos" SET "done" = 0 WHERE "id" = $1"#, vec![todo.into()])
      .await;
    bench
      .agent
      .play([Played::answered(reply(session, FINISHED)).doing(closing(&bench.scene.db, todo))]);

    let ran = bench.run(&bench.given(job, &dir)).await;

    turned.push(Turned {
      asked: bench.agent.asked()[asked].thread.clone(),
      handed: bench.agent.handed().get(handed).and_then(|handover| handover.thread.clone()),
      success: ran.success,
    });
  }

  turned.try_into().unwrap_or_else(|_| unreachable!("three jobs ran"))
}

#[tokio::test]
async fn the_first_job_leaves_its_handover_on_its_own_item() {
  let [opening, ..] = items_taking_turns().await;

  assert!(opening.success);
  assert_eq!(opening.asked.as_deref(), Some("todo:1"));
  assert_eq!(opening.handed.as_deref(), Some("todo:1"));
}

#[tokio::test]
async fn a_job_of_another_item_opens_on_its_own_item_and_not_the_one_before_it() {
  let [_, other, _] = items_taking_turns().await;

  assert!(other.success);
  assert_eq!(other.asked.as_deref(), Some("todo:2"));
  assert_eq!(other.handed.as_deref(), Some("todo:2"));
}

#[tokio::test]
async fn the_next_job_of_the_same_item_goes_on_the_thread_its_last_job_handed_over() {
  let [_, other, again] = items_taking_turns().await;

  assert!(again.success);
  assert_eq!(again.asked, other.handed);
}

// A note on the item, written now or a minute before the job was launched.
async fn note(
  db: &sea_orm::DatabaseConnection,
  todo: i64,
  author: &str,
  text: &str,
  kind: Option<&str>,
  at: i64,
) {
  job_base::run(
    db,
    r#"INSERT INTO "todo_notes" ("todo_id", "author", "text", "added_at", "kind") VALUES ($1, $2, $3, $4, $5)"#,
    vec![todo.into(), author.into(), text.into(), at.into(), kind.map(str::to_owned).into()],
  )
  .await
  .expect("the note is written");
}

const RAW: &str = "RAW_PERSON_WORDS_STAY_WITH_THE_AGENT";
const CHANGE: &str = "Put the numbers in the report as well.";
const SECOND: &str = "Owner, here is the report again with the added words.\n\nWhat changed\nThe panel is taller and has its numbers.\n\n1 done";

// While the first turn works, a person appends raw words and the agent writes its change onto the item; the
// turn answers, and the asking after it is answered with the second answer.
async fn added_while_working() -> (Bench, Ran) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  bench.scene.item(5, "An item words are added to while it runs").await;

  let job = bench.job(Some(5), WORKER, "bmo-task-timed").await;
  let db = bench.scene.db.clone();
  let appended: Act = Box::new(move |_| -> BoxFuture<'static, ()> {
    Box::pin(async move {
      let now = bmo_job::now_ms();

      note(&db, 5, "Owner", RAW, None, now).await;
      note(&db, 5, "Agent", CHANGE, Some(CHANGE_NOTE), now).await;
    })
  });

  bench.agent.play([
    Played::answered(reply("s1", ANSWER)).doing(appended),
    Played::answered(reply("s1-again", SECOND)).doing(closing(&bench.scene.db, 5)),
  ]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, ran)
}

#[tokio::test]
async fn the_same_session_is_asked_again_with_the_agent_s_change_and_not_the_person_s_words() {
  let (bench, ran) = added_while_working().await;
  let prompts = bench.agent.prompts();

  assert!(ran.success, "{ran:?}");
  // Two prompts and no more: the trailer every job is held to rides on each of them.
  assert_eq!(prompts.len(), 2);
  assert!(prompts.iter().all(|prompt| prompt.contains(TRAILER)));
  assert!(prompts[1].contains(CHANGE), "{}", prompts[1]);
  assert!(prompts[1].contains("Change to the brief (note "), "{}", prompts[1]);
  assert!(!prompts.iter().any(|prompt| prompt.contains(RAW)), "{prompts:?}");
  // It is the item going on, not a job of its own, and the report the run already wrote has reached nobody.
  assert!(prompts[1].contains("is not a new job"), "{}", prompts[1]);
  assert!(prompts[1].contains("Your report has not been handed over"), "{}", prompts[1]);
  assert_eq!(bench.agent.asked()[1].resume, Resume::Session { id: "s1".into() });
}

#[tokio::test]
async fn the_second_answer_is_the_report_in_place_of_the_first() {
  let (bench, ran) = added_while_working().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(
    bench.told().contains("here is the report again with the added words"),
    "{}",
    bench.told()
  );
  assert!(!bench.told().contains("The panel was made taller."), "{}", bench.told());
}

#[tokio::test]
async fn the_change_is_marked_handed_over_and_the_person_s_words_never_are() {
  let (bench, _) = added_while_working().await;
  let rows = job_base::all(
    &bench.scene.db,
    r#"SELECT "text", "delivered_at" FROM "todo_notes" WHERE "todo_id" = 5 ORDER BY "id""#,
    vec![],
  )
  .await
  .unwrap();
  let delivered: Vec<(String, bool)> = rows
    .iter()
    .map(|row| {
      (
        row.try_get("", "text").unwrap(),
        row.try_get::<Option<i64>>("", "delivered_at").unwrap().is_some(),
      )
    })
    .collect();

  // The rows themselves stay: they are the record of what was asked while the work was being done.
  assert_eq!(delivered, [(RAW.to_owned(), false), (CHANGE.to_owned(), true)]);
}

#[tokio::test]
async fn the_session_recorded_and_handed_over_is_the_one_that_answered_last() {
  let (bench, _) = added_while_working().await;

  assert_eq!(bench.agent.recorded()[0].session_id.as_deref(), Some("s1-again"));
  assert_eq!(bench.agent.handed()[0].session.as_deref(), Some("s1-again"));
}

// Words added before the job was launched are already in the prompt it was composed from.
async fn added_before_launch() -> (Bench, Ran) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  bench.scene.item(5, "An item words were added to before it ran").await;
  note(
    &bench.scene.db,
    5,
    "Agent",
    "Said before the job was launched.",
    Some(CHANGE_NOTE),
    bmo_job::now_ms() - 60_000,
  )
  .await;

  let job = bench.job(Some(5), WORKER, "bmo-task-timed").await;

  bench
    .agent
    .play([Played::answered(reply("s1", ANSWER)).doing(closing(&bench.scene.db, 5))]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  (bench, ran)
}

#[tokio::test]
async fn a_job_whose_item_was_added_to_before_its_launch_is_one_turn_whose_answer_is_the_report() {
  let (bench, ran) = added_before_launch().await;

  assert!(ran.success, "{ran:?}");
  assert_eq!(bench.agent.prompts().len(), 1);
  assert!(bench.told().contains("The panel was made taller."), "{}", bench.told());
}

#[tokio::test]
async fn the_words_added_before_launch_are_left_as_they_were_since_nobody_was_handed_them() {
  let (bench, _) = added_before_launch().await;

  assert_eq!(
    job_base::one(
      &bench.scene.db,
      r#"SELECT "delivered_at" FROM "todo_notes" WHERE "todo_id" = 5"#,
      vec![]
    )
    .await
    .unwrap()
    .unwrap()
    .try_get::<Option<i64>>("", "delivered_at")
    .unwrap(),
    None
  );
}

// The ordinary job, which is nearly every job: one read of the item's notes at the end and nothing else.
#[tokio::test]
async fn a_job_nobody_added_anything_to_runs_once_and_reports_what_it_answered() {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  bench.scene.item(5, "An item nobody added words to").await;

  let job = bench.job(Some(5), WORKER, "bmo-task-timed").await;

  bench
    .agent
    .play([Played::answered(reply("s1", ANSWER)).doing(closing(&bench.scene.db, 5))]);

  let ran = bench.run(&bench.given(job, &dir)).await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert_eq!(bench.agent.prompts().len(), 1);
  // The first turn resumes whatever bmo-agency holds for the worker, and names no session of its own.
  assert_eq!(bench.agent.asked()[0].resume, Resume::Auto);
  assert!(bench.told().contains("The panel was made taller."), "{}", bench.told());
}

// A job that answers with its item still open leaves the queue counting it as waiting for the worker, so the
// same session is asked once to close or hold it; an item still open after that holds nothing back.
async fn left_open() -> (Bench, Ran) {
  let bench = Bench::new().await;
  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;
  bench.scene.item(5, "An item its job does not close").await;

  let job = bench.job(Some(5), WORKER, "bmo-task-timed").await;

  bench.agent.play([Played::answered(reply(
    "s1",
    "The README opens with its title.\n\nChecked\n1. Read the first line.\n\n1 done",
  ))]);

  let ran = bench
    .run(&Given {
      task: "Read the first line of the README and report it.".into(),
      ..bench.given(job, &dir)
    })
    .await;

  (bench, ran)
}

#[tokio::test]
async fn a_job_that_left_its_item_open_is_asked_once_with_the_verbs_that_close_or_hold_it() {
  let (bench, ran) = left_open().await;
  let prompts = bench.agent.prompts();

  assert!(ran.success, "{ran:?}");
  assert_eq!(prompts.iter().filter(|prompt| prompt.contains("Your item #5 is still open")).count(), 1);
  assert!(prompts[1].contains("todo --done 5 --report '"), "{}", prompts[1]);
  assert!(prompts[1].contains("todo --stop-here 5 --why '"), "{}", prompts[1]);
}

#[tokio::test]
async fn it_still_ends_as_the_work_it_was_with_the_item_left_open_named_under_its_report() {
  let (_, ran) = left_open().await;

  assert!(ran.ended(JOB_DONE, true), "{ran:?}");
  assert!(ran.report.contains("The job left its item #5 open"), "{}", ran.report);
}
