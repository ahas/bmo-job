//! The owner's "stop here, resume after" through the verb (docs/verbs.md, "The TODO's edits and holds"): the
//! mark a wind-down leaves, the closing it refuses, the account it takes instead, the resume a condition tells,
//! an item stopped outright, a hold waiting on a deployment, and a job's shell refused taking a hold off.

use bmo_job::verbs::todo::holds::{Condition, STOPPED_HERE_MARK, wind_down_reason};
use queue_rows::todos::WIND_DOWN_NOTE_MARK;
use serde_json::json;

use crate::support::todo::{Bench, OWNER_SENDER, Ran};

const REASON: &str = "to clear the deploy window";
const OTHER_REASON: &str = "a reason of the run's own";
const UNIT: &str = "bmo-task-wound";

fn after_finished(id: i64) -> Condition {
  Condition::Finished { id }
}

async fn item(bench: &Bench, title: &str) -> i64 {
  bench.item(title, "Conclusion\n\nWhat to do.").await
}

// An item with its job running, and the item its resume waits on.
async fn wound_down() -> (Bench, i64, i64, i64) {
  let bench = Bench::new().await;
  let id = item(&bench, "training comparison").await;
  let after = item(&bench, "deploy window").await;
  let job = bench.job(id, UNIT).await;

  (bench, id, after, job)
}

async fn wind_down(bench: &Bench, id: i64, after: i64) -> Ran {
  bench
    .todo(&["--wind-down", &id.to_string(), "--why", REASON, "--after", &after.to_string()])
    .await
}

async fn held(bench: &Bench, id: i64) -> (Option<String>, Option<String>, bool) {
  (
    bench.field(id, "held_kind").await,
    bench.field(id, "held_reason").await,
    bench.done(id).await,
  )
}

async fn stop_hold(bench: &Bench, id: i64, reason: &str) {
  bench
    .run(
      r#"UPDATE "todos" SET "held_at" = 1756800000000, "held_reason" = $2, "held_kind" = 'stop' WHERE "id" = $1"#,
      vec![id.into(), reason.into()],
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wind_down_holds_it_waiting_to_resume_and_hands_the_running_job_one_line_naming_stop_here() {
  let (bench, id, after, job) = wound_down().await;
  let ran = wind_down(&bench, id, after).await;
  let notes = bench.notes(id).await;

  assert_eq!(ran.answer()["code"], "item_wound_down");
  assert_eq!(ran.answer()["job"], job);
  assert_eq!(ran.answer()["condition"], json!({ "kind": "item_finished", "id": after }));
  assert_eq!(
    held(&bench, id).await,
    (
      Some("pause".to_owned()),
      Some(wind_down_reason(REASON, Some(&after_finished(after)))),
      false
    )
  );
  assert_eq!(bench.field_number(id, "resume_after").await, Some(after));
  assert_eq!(notes.len(), 1);
  assert_eq!(notes[0].job, Some(job));
  assert_eq!(notes[0].author_id, Some(OWNER_SENDER));
  assert!(notes[0].text.starts_with(WIND_DOWN_NOTE_MARK), "{}", notes[0].text);
  assert!(notes[0].text.contains("stop-here"), "{}", notes[0].text);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wound_down_item_is_refused_closing_as_done_and_stays_open_and_held() {
  let (bench, id, after, _) = wound_down().await;

  wind_down(&bench, id, after).await.answer();

  let closing = bench
    .todo(&[
      "--done",
      &id.to_string(),
      "--report",
      "Done up to here.",
      "--unfinished",
      "Resume after the window.",
    ])
    .await;

  assert_eq!(closing.refused, "done_while_held");
  assert!(!bench.done(id).await);
  assert_eq!(bench.field(id, "held_kind").await.as_deref(), Some("pause"));
  assert_eq!(bench.field(id, "report").await, None);
  assert_eq!(bench.field(id, "unfinished").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_run_s_account_is_taken_as_words_on_the_item_not_handed_back_to_that_run() {
  let (bench, id, after, job) = wound_down().await;

  wind_down(&bench, id, after).await.answer();

  let stopped = bench
    .job_todo(
      UNIT,
      &["--stop-here", &id.to_string(), "--report", "Finished up to training A. B is next."],
    )
    .await;
  let notes = bench.notes(id).await;
  let account = notes.iter().find(|note| note.text.starts_with(STOPPED_HERE_MARK)).expect("the account");

  assert_eq!(stopped.answer()["code"], "stopped_here");
  assert_eq!(stopped.answer()["held"], "pause");
  assert_eq!(account.job, Some(job));
  assert!(account.delivered, "the run that wrote it is not handed its own account");
  assert!(!bench.done(id).await);
  assert_eq!(bench.field(id, "held_kind").await.as_deref(), Some("pause"));
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_the_item_it_waits_on_says_the_condition_is_met_and_leaves_it_held_until_released() {
  let (bench, id, after, _) = wound_down().await;

  wind_down(&bench, id, after).await.answer();
  bench
    .job_todo(UNIT, &["--stop-here", &id.to_string(), "--report", "B is next."])
    .await
    .answer();

  let closing = bench.todo(&["--done", &after.to_string(), "--report", "Deployed."]).await;

  assert_eq!(closing.code, 0, "{}", closing.said);
  assert!(
    closing.answers.iter().any(|answer| answer["code"] == "resume_condition_met"
      && answer["id"] == id
      && answer["after"] == after),
    "{:?}",
    closing.answers
  );
  assert_eq!(bench.field(id, "held_kind").await.as_deref(), Some("pause"));
  assert!(!bench.done(id).await);

  // The release resumes it, and the account stands as the only word on it.
  assert_eq!(bench.todo(&["--unhold", &id.to_string()]).await.answer()["code"], "item_unheld");
  assert_eq!(bench.field_number(id, "held_at").await, None);
  assert_eq!(bench.field(id, "held_kind").await, None);
  assert_eq!(
    bench
      .notes(id)
      .await
      .iter()
      .map(|note| note.text.starts_with(STOPPED_HERE_MARK))
      .collect::<Vec<_>>(),
    [true]
  );
  assert_eq!(bench.todo(&["--done", &id.to_string(), "--report", "Done to the end."]).await.code, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_wound_down_item_stays_in_the_ordinary_listing_with_its_reason() {
  let (bench, id, after, job) = wound_down().await;

  wind_down(&bench, id, after).await.answer();
  bench
    .run(
      r#"UPDATE "jobs" SET "finished_at" = $2, "report" = 'end', "ok" = 1, "outcome" = 'done' WHERE "id" = $1"#,
      vec![job.into(), bmo_job::now_ms().into()],
    )
    .await;

  let listed = bench.listed("").await;
  let line = listed
    .as_array()
    .unwrap()
    .iter()
    .find(|line| line["id"] == id)
    .expect("the item is listed");

  assert_eq!(line["title"], "training comparison");
  assert!(line.to_string().contains(REASON), "{line}");
}

#[tokio::test(flavor = "multi_thread")]
async fn with_no_job_running_a_wind_down_only_holds_and_refuses_a_condition_already_met() {
  let bench = Bench::new().await;
  let id = item(&bench, "training comparison").await;
  let done = item(&bench, "a finished item").await;

  bench.finish(done).await;

  let refused = bench
    .todo(&["--wind-down", &id.to_string(), "--why", REASON, "--after", &done.to_string()])
    .await;
  let plain = bench.todo(&["--wind-down", &id.to_string(), "--why", REASON]).await;

  assert_eq!(refused.refused, "after_not_open");
  assert_eq!(plain.answer()["code"], "item_wound_down");
  assert_eq!(plain.answer()["job"], json!(null));
  assert!(bench.notes(id).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_the_owner_stopped_is_refused_as_done_and_takes_a_stopped_here_account() {
  let bench = Bench::new().await;
  let id = item(&bench, "stopped work").await;

  bench.job(id, UNIT).await;
  stop_hold(&bench, id, REASON).await;

  assert_eq!(
    bench.job_todo(UNIT, &["--done", &id.to_string(), "--report", "x"]).await.refused,
    "done_while_held"
  );

  let stopped = bench.job_todo(UNIT, &["--stop-here", &id.to_string(), "--report", "Half done."]).await;

  assert_eq!(stopped.answer()["code"], "stopped_here");
  assert!(!bench.done(id).await);
  assert_eq!(bench.field(id, "held_kind").await.as_deref(), Some("stop"));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_no_run_is_working_takes_no_stopped_here_account() {
  let bench = Bench::new().await;
  let id = item(&bench, "ordinary work").await;
  let refused = bench
    .todo(&["--stop-here", &id.to_string(), "--why", REASON, "--report", "Half done."])
    .await;

  assert_eq!(refused.refused, "stop_here_not_held");
  assert!(bench.notes(id).await.is_empty());
  assert_eq!(bench.field(id, "held_kind").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_already_held_keeps_the_owner_s_reason_and_refuses_the_run_s_own() {
  let bench = Bench::new().await;
  let id = item(&bench, "stopped work").await;

  bench.job(id, UNIT).await;
  stop_hold(&bench, id, REASON).await;

  let refused = bench
    .job_todo(UNIT, &["--stop-here", &id.to_string(), "--why", OTHER_REASON, "--report", "Half done."])
    .await;

  assert_eq!(refused.refused, "stop_here_held_takes_no_reason");
  assert_eq!(bench.field(id, "held_kind").await.as_deref(), Some("stop"));
  assert_eq!(bench.field(id, "held_reason").await.as_deref(), Some(REASON));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_job_s_stop_here_without_a_reason_is_refused() {
  let (bench, id, _, _) = wound_down().await;
  let refused = bench
    .job_todo(UNIT, &["--stop-here", &id.to_string(), "--report", "Done up to A."])
    .await;

  assert_eq!(refused.refused, "stop_here_needs_reason");
  assert!(bench.notes(id).await.is_empty());
  assert_eq!(bench.field(id, "held_kind").await, None);
  assert!(!bench.done(id).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_running_job_s_stop_here_with_its_reason_and_condition_holds_the_item_and_leaves_its_account() {
  let (bench, id, after, job) = wound_down().await;
  let stopped = bench
    .job_todo(
      UNIT,
      &[
        "--stop-here",
        &id.to_string(),
        "--why",
        REASON,
        "--after",
        &after.to_string(),
        "--report",
        "Done up to A. B next.",
      ],
    )
    .await;
  let notes = bench.notes(id).await;
  let reason = wind_down_reason(REASON, Some(&after_finished(after)));

  assert_eq!(stopped.answer()["code"], "stopped_here");
  assert_eq!(stopped.answer()["held"], "pause");
  assert_eq!(stopped.answer()["reason"], reason);
  assert_eq!(held(&bench, id).await, (Some("pause".to_owned()), Some(reason), false));
  assert_eq!(bench.field_number(id, "resume_after").await, Some(after));
  // The run is the one stopping, so it is handed no line telling it to stop: its account alone stands.
  assert_eq!(notes.len(), 1);
  assert_eq!(notes[0].job, Some(job));
  assert!(notes[0].text.starts_with(STOPPED_HERE_MARK));
  assert_eq!(
    bench.job_todo(UNIT, &["--done", &id.to_string(), "--report", "Done."]).await.refused,
    "done_while_held"
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_stop_here_whose_condition_is_the_item_itself_is_refused_writing_nothing() {
  let (bench, id, _, _) = wound_down().await;
  let refused = bench
    .job_todo(
      UNIT,
      &[
        "--stop-here",
        &id.to_string(),
        "--why",
        REASON,
        "--after",
        &id.to_string(),
        "--report",
        "Half.",
      ],
    )
    .await;

  assert_eq!(refused.refused, "after_not_open");
  assert_eq!(bench.field(id, "held_kind").await, None);
  assert!(bench.notes(id).await.is_empty());
}

const COMMIT: &str = "c0ffee0000000000000000000000000000000000";

// bmo-testbed answering that the item named has one commit waiting to go live.
fn waiting_to_go_live(bench: &Bench, todo: i64) {
  bench.served.testbed.answer(
    "pending-list",
    &json!([{
      "repository": "job", "commit": COMMIT, "item": todo, "worker": "worker", "job": null, "reason": "up",
      "urgent": false, "created_at": 1
    }])
    .to_string(),
    "",
    0,
  );
}

async fn awaiting_deploy(bench: &Bench, id: i64) -> Option<String> {
  bench.field(id, "resume_after_deploy").await
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_s_number_settles_to_its_commits_waiting_to_go_live_said_after_the_reason() {
  let bench = Bench::new().await;
  let id = item(&bench, "part 3").await;
  let before = item(&bench, "parts 1 and 2").await;

  waiting_to_go_live(&bench, before);

  let ran = bench
    .todo(&["--hold", &id.to_string(), "--why", REASON, "--after-deploy", &format!("#{before}")])
    .await;
  let condition = Condition::Deployed {
    commits: vec![COMMIT.to_owned()],
    item: Some(before),
  };

  assert_eq!(ran.answer()["code"], "item_held");
  assert_eq!(bench.field(id, "held_kind").await, None);
  assert_eq!(bench.field(id, "held_reason").await, Some(wind_down_reason(REASON, Some(&condition))));
  assert!(awaiting_deploy(&bench, id).await.is_some_and(|commits| commits.contains(COMMIT)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_holding_its_own_item_on_a_deployment_takes_it_with_stop_here() {
  let (bench, id, _, _) = wound_down().await;

  waiting_to_go_live(&bench, id);

  let stopped = bench
    .job_todo(
      UNIT,
      &[
        "--stop-here",
        &id.to_string(),
        "--why",
        REASON,
        "--after-deploy",
        &id.to_string(),
        "--report",
        "Part 1 done.",
      ],
    )
    .await;

  assert_eq!(stopped.answer()["code"], "stopped_here");
  assert_eq!(bench.field(id, "held_kind").await.as_deref(), Some("pause"));
  assert_eq!(bench.field_number(id, "resume_after").await, None);
  assert!(awaiting_deploy(&bench, id).await.is_some_and(|commits| commits.contains(COMMIT)));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_hold_on_nothing_waiting_a_name_that_is_no_commit_or_two_conditions_is_refused_holding_nothing() {
  let bench = Bench::new().await;
  let id = item(&bench, "part 3").await;
  let before = item(&bench, "parts 1 and 2").await;
  let (id_text, before_text) = (id.to_string(), before.to_string());
  let nothing = bench.todo(&["--hold", &id_text, "--after-deploy", &before_text]).await;
  let no_commit = bench.todo(&["--hold", &id_text, "--after-deploy", "no-such-commit"]).await;
  let both = bench
    .todo(&[
      "--wind-down",
      &id_text,
      "--why",
      REASON,
      "--after",
      &before_text,
      "--after-deploy",
      &before_text,
    ])
    .await;

  assert_eq!(nothing.refused, "no_waiting_commits");
  assert_eq!(no_commit.code, 1);
  assert_eq!(both.refused, "condition_both");
  assert_eq!(bench.field_number(id, "held_at").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn taking_the_hold_off_takes_the_deployment_condition_with_it() {
  let bench = Bench::new().await;
  let id = item(&bench, "part 3").await;
  let before = item(&bench, "parts 1 and 2").await;

  waiting_to_go_live(&bench, before);
  bench
    .todo(&["--wind-down", &id.to_string(), "--why", REASON, "--after-deploy", &before.to_string()])
    .await
    .answer();
  bench.todo(&["--unhold", &id.to_string()]).await.answer();

  assert_eq!(awaiting_deploy(&bench, id).await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_s_shell_is_refused_taking_a_hold_off() {
  let bench = Bench::new().await;
  let id = item(&bench, "the next step").await;

  bench
    .run(
      r#"UPDATE "todos" SET "held_at" = 1756800000000, "held_reason" = $2 WHERE "id" = $1"#,
      vec![id.into(), REASON.into()],
    )
    .await;

  assert_eq!(
    bench.job_todo("bmo-task-other", &["--unhold", &id.to_string()]).await.refused,
    "job_cannot_unhold"
  );
  assert_eq!(bench.field(id, "held_reason").await.as_deref(), Some(REASON));
}
