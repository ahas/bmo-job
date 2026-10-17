//! The account a closing leaves on an item (docs/rows.md, "The closing's readings"): what the closing
//! refuses, what it keeps and hands over, the unfinished note resolved by hand or settled by the last of
//! the rows raised out of it, and what reopening, resolving and dropping answer — read back through the
//! server's readings.

use bmo_job::queue::questions::{self, NewQuestion};
use serde_json::{Value, json};

use crate::support::todo::{Bench, OWNER_CHAT};

const REPORT: &str =
  "Added two columns and made the closing take them.\nThey show in the list and the detail too.";
const WHY: &str = "The web side stands as #31, and the rest was done after it.";

fn owner_mark(bench: &Bench) -> String {
  bench.stand.policy.closing.strands.owner_mark.clone()
}

fn agent_mark(bench: &Bench) -> String {
  bench.stand.policy.closing.strands.agent_mark.clone()
}

// The note an ordinary closing leaves, one strand of the owner's.
fn note(bench: &Bench) -> String {
  format!(
    "1. {} old finished items carry no record. They pile up from the next closing on.",
    owner_mark(bench)
  )
}

// An approved item, ready to be closed.
async fn approved_item() -> (Bench, i64) {
  let bench = Bench::new().await;
  let id = bench.item("work that leaves a report", "Conclusion\n\nLeave a report.").await;

  (bench, id)
}

async fn close(bench: &Bench, id: i64, report: &str, unfinished: &str) -> crate::support::todo::Ran {
  bench
    .todo(&["--done", &id.to_string(), "--report", report, "--unfinished", unfinished])
    .await
}

fn marks(line: &Value) -> Vec<String> {
  line["marks"]
    .as_array()
    .unwrap()
    .iter()
    .map(|mark| mark["kind"].as_str().unwrap().to_owned())
    .collect()
}

async fn listed_line(bench: &Bench, id: i64) -> Value {
  let listed = bench.listed("?all").await;

  listed
    .as_array()
    .unwrap()
    .iter()
    .find(|line| line["id"] == id)
    .cloned()
    .expect("the item is listed")
}

// The memories filed with bmo-brain, by the action each holds.
fn filed_memories(bench: &Bench) -> Vec<Value> {
  bench
    .brain
    .calls()
    .into_iter()
    .filter(|call| call.route == "POST /memory/add")
    .filter_map(|call| call.body)
    .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn what_is_given_to_done_is_kept_whole_and_read_back_as_it_went_in() {
  let (bench, id) = approved_item().await;
  let ran = close(&bench, id, REPORT, &note(&bench)).await;

  assert_eq!(
    ran.answer(),
    &json!({ "code": "item_closed", "id": id, "written": ["report", "unfinished"], "carried": [] })
  );
  // Whole, not clipped: a report cut in half is not a report.
  assert_eq!(bench.field(id, "report").await.as_deref(), Some(REPORT));
  assert_eq!(bench.field(id, "unfinished").await, Some(note(&bench)));
  assert!(bench.done(id).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn done_refuses_a_left_section_or_a_strand_that_proposes_an_improvement_and_writes_nothing() {
  let (bench, id) = approved_item().await;
  let proposing = format!(
    "{REPORT}\n\nLeft\n1. The launch line is not mine to change, so I went around it; a follow-up could change it."
  );
  let left = bench.todo(&["--done", &id.to_string(), "--report", &proposing]).await;
  let strand = format!("1. {} decide in a later job whether to improve it.", owner_mark(&bench));
  let strand = close(&bench, id, REPORT, &strand).await;

  assert_eq!(left.refused, "closing_refused");
  assert!(left.said.contains("propose or defer an improvement"), "{}", left.said);
  assert_eq!(strand.refused, "closing_refused");
  assert!(strand.said.contains("propose or defer an improvement"), "{}", strand.said);
  assert!(!bench.done(id).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn done_refuses_a_report_or_a_strand_that_leaves_a_check_undone_and_writes_nothing() {
  let (bench, id) = approved_item().await;
  let report = format!("{REPORT}\n\nChecked\n1. The ten-minute limit was checked only by a unit test.");
  let undone = bench.todo(&["--done", &id.to_string(), "--report", &report]).await;
  let strand = format!("1. {} RAM module details need root, so they were not read.", agent_mark(&bench));
  let strand = close(&bench, id, REPORT, &strand).await;

  assert_eq!(undone.refused, "closing_refused");
  assert!(
    undone.said.contains("1. The ten-minute limit was checked only by a unit test."),
    "{}",
    undone.said
  );
  assert_eq!(strand.refused, "closing_refused");
  assert!(strand.said.contains("they were not read."), "{}", strand.said);
  assert!(!bench.done(id).await);

  let beyond =
    format!("{REPORT}\n\nLeft\n1. The 10 GbE link was not measured: the NIC is not yet in the lab.");

  assert_eq!(
    bench.todo(&["--done", &id.to_string(), "--report", &beyond]).await.answer()["code"],
    "item_closed"
  );
  assert!(bench.done(id).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn done_refuses_a_report_that_hands_the_owner_a_page_hosted_outside_the_archive() {
  let (bench, id) = approved_item().await;
  let report =
    format!("{REPORT}\n\nThe design is at https://claude.ai/artifact/abc for now.");
  let ran = bench.todo(&["--done", &id.to_string(), "--report", &report]).await;

  assert_eq!(ran.code, 1);
  assert!(ran.said.contains("https://claude.ai/artifact/abc"), "{}", ran.said);
  assert!(!bench.done(id).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reading_spreads_the_whole_account_out_under_the_instruction() {
  let (bench, id) = approved_item().await;

  close(&bench, id, REPORT, &note(&bench)).await.answer();

  let shown = bench.shown(id).await;

  assert_eq!(shown["report"], REPORT);
  assert_eq!(shown["unfinished"]["text"], note(&bench));
  assert_eq!(shown["unfinished"]["resolved_at"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reading_says_what_the_work_stopped_to_ask_and_how_each_question_was_settled() {
  let (bench, id) = approved_item().await;
  let options = vec!["dig a question table of its own".to_owned(), "put them on the todos row".to_owned()];
  let db = bench.served.db();
  let mut asked = vec![];

  for question in [
    "where should what was asked be written?",
    "this one nobody answered",
    "this one was answered in words",
  ] {
    let opened = questions::open(
      db,
      &NewQuestion {
        job_id: None,
        todo_id: id,
        chat_id: OWNER_CHAT,
        question,
        options: &options,
        recommended: 0,
        why: "it is hard to take back",
        asked_at: 1,
        due_at: 600_001,
        talks: true,
      },
    )
    .await
    .unwrap();

    asked.push(opened.id);
  }

  questions::settle(db, asked[0], &questions::Answer::Chosen(1), 2).await.unwrap();
  questions::settle_unanswered(db, asked[1], 0, 700_000).await.unwrap();
  questions::settle(db, asked[2], &questions::Answer::Words("neither; keep one in each room".into()), 2)
    .await
    .unwrap();

  let shown = bench.shown(id).await;
  let read = shown["questions"].as_array().unwrap();

  assert_eq!(read[0]["state"], json!({ "kind": "chosen" }));
  assert_eq!(read[0]["settled"], json!({ "number": 2, "option": "put them on the todos row" }));
  assert_eq!(read[1]["state"], json!({ "kind": "defaulted" }));
  assert_eq!(read[1]["settled"], json!({ "number": 1, "option": "dig a question table of its own" }));
  assert_eq!(read[2]["state"], json!({ "kind": "typed", "answer": "neither; keep one in each room" }));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_listing_marks_which_finished_item_left_an_account_and_which_left_something_undone() {
  let (bench, id) = approved_item().await;

  close(&bench, id, REPORT, &note(&bench)).await.answer();

  let line = listed_line(&bench, id).await;

  assert!(marks(&line).contains(&"report".to_owned()), "{line}");
  assert!(marks(&line).contains(&"unfinished".to_owned()), "{line}");
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_with_no_account_at_all_still_closes_and_writes_nothing_it_does_not_have() {
  let (bench, id) = approved_item().await;
  let ran = bench.todo(&["--done", &id.to_string()]).await;

  assert_eq!(ran.answer(), &json!({ "code": "item_closed", "id": id, "written": [], "carried": [] }));
  assert!(bench.done(id).await);
  assert_eq!(bench.field(id, "report").await, None);
  assert_eq!(bench.field(id, "unfinished").await, None);
  assert!(!marks(&listed_line(&bench, id).await).contains(&"report".to_owned()));
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_again_without_naming_the_flags_keeps_the_account_already_on_the_row() {
  let (bench, id) = approved_item().await;

  close(&bench, id, REPORT, &note(&bench)).await.answer();
  bench.todo(&["--done", &id.to_string()]).await.answer();

  assert_eq!(bench.field(id, "report").await.as_deref(), Some(REPORT));
  assert_eq!(bench.field(id, "unfinished").await, Some(note(&bench)));
}

#[tokio::test(flavor = "multi_thread")]
async fn naming_a_flag_empty_takes_what_it_holds_off_the_row() {
  let (bench, id) = approved_item().await;

  close(&bench, id, REPORT, &note(&bench)).await.answer();
  bench.todo(&["--done", &id.to_string(), "--unfinished", ""]).await.answer();

  assert_eq!(bench.field(id, "report").await.as_deref(), Some(REPORT));
  assert_eq!(bench.field(id, "unfinished").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_note_whose_strand_says_whose_it_is_not_at_all_is_refused_and_the_item_stays_open() {
  let (bench, id) = approved_item().await;
  let note = format!("1. {} mine.\n2. a strand nobody named as theirs.", agent_mark(&bench));
  let ran = close(&bench, id, REPORT, &note).await;

  assert_eq!(ran.refused, "closing_refused");
  assert!(ran.said.contains("a strand nobody named as theirs"), "{}", ran.said);
  // Nothing at all is written: the refusal comes before the closing.
  assert!(!bench.done(id).await);
  assert!(filed_memories(&bench).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn host_work_marked_as_the_owner_s_is_refused_and_closes_once_marked_as_the_agent_s() {
  let (bench, id) = approved_item().await;
  let work = "Keep fs.inotify.max_user_instances at 512 past a reboot with a sysctl.d entry.";
  let ran = close(&bench, id, REPORT, &format!("1. {} {work}", owner_mark(&bench))).await;

  assert_eq!(ran.refused, "closing_refused");
  assert!(ran.said.contains(work), "{}", ran.said);
  assert!(!bench.done(id).await);

  assert_eq!(
    close(&bench, id, REPORT, &format!("1. {} {work}", agent_mark(&bench))).await.answers[0]["code"],
    "item_closed"
  );
  assert!(bench.done(id).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closing_files_every_strand_of_the_agent_s_among_the_memories_and_hands_each_over_on_its_own() {
  let (bench, id) = approved_item().await;
  let note = [
    format!("1. {} the mail account file has to be made.", agent_mark(&bench)),
    format!("2. {} the mail server's setting is left.", agent_mark(&bench)),
    format!("3. {} the owner decides whether to use the domain.", owner_mark(&bench)),
  ]
  .join("\n");
  let ran = close(&bench, id, REPORT, &note).await;
  let handed: Vec<&Value> = ran.answers.iter().filter(|answer| answer["code"] == "strand_handed").collect();
  let memories = filed_memories(&bench);
  let actions: Vec<&str> = memories.iter().map(|memory| memory["action"].as_str().unwrap()).collect();

  assert_eq!(ran.answers[0]["code"], "item_closed");
  assert_eq!(handed.len(), 2, "{:?}", ran.answers);
  assert_eq!(actions, ["the mail account file has to be made.", "the mail server's setting is left."]);
  // The handover goes to the owner's chat whatever room the item stands in.
  assert!(
    memories
      .iter()
      .all(|memory| memory["chat"] == OWNER_CHAT && memory["todo"] == id && memory["type"] == "followup")
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_the_same_item_again_hands_nothing_over_twice() {
  let (bench, id) = approved_item().await;
  let note = format!("1. {} a mount has to be put up.", agent_mark(&bench));

  close(&bench, id, REPORT, &note).await.answer();

  // bmo-brain holds the memory the first closing filed, which the second reads before it hands anything.
  bench.brain.answer(
    "GET /memories",
    200,
    json!({ "answered": true, "memories": [{
      "id": 1, "slug": "memory-1", "type": "followup", "chatId": OWNER_CHAT, "action": filed_memories(&bench)[0]["action"],
      "todoId": id, "active": true, "setAt": 1, "doneAt": null
    }] }),
  );
  bench.todo(&["--done", &id.to_string()]).await.answer();

  assert_eq!(filed_memories(&bench).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn reopening_takes_the_closing_back_and_the_account_of_it_goes_with_it() {
  let (bench, id) = approved_item().await;

  close(&bench, id, REPORT, &note(&bench)).await.answer();
  assert_eq!(bench.todo(&["--reopen", &id.to_string()]).await.answer()["code"], "item_reopened");

  // An open item carrying an account of how it was finished would say the one thing that is not true of it.
  assert!(!bench.done(id).await);
  assert_eq!(bench.field(id, "report").await, None);
  assert_eq!(bench.field(id, "unfinished").await, None);
}

// An item closed with the ordinary note, for resolving it.
async fn closed() -> (Bench, i64) {
  let (bench, id) = approved_item().await;

  close(&bench, id, REPORT, &note(&bench)).await.answer();
  (bench, id)
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resolved_note_stays_on_the_row_word_for_word_and_the_row_gains_why_it_no_longer_stands() {
  let (bench, id) = closed().await;
  let ran = bench.todo(&["--resolve", &id.to_string(), "--why", WHY]).await;

  assert_eq!(ran.answer()["code"], "unfinished_resolved");
  assert_eq!(ran.answer()["why"], WHY);
  assert_eq!(bench.field(id, "unfinished").await, Some(note(&bench)));
  assert_eq!(bench.field(id, "unfinished_resolution").await.as_deref(), Some(WHY));
  assert!(bench.field_number(id, "unfinished_resolved_at").await.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_readings_tell_a_resolved_note_from_an_open_one() {
  let (bench, id) = closed().await;

  assert!(marks(&listed_line(&bench, id).await).contains(&"unfinished".to_owned()));

  bench.todo(&["--resolve", &id.to_string(), "--why", WHY]).await.answer();

  let shown = bench.shown(id).await;
  let line_marks = marks(&listed_line(&bench, id).await);

  assert!(shown["unfinished"]["resolved_at"].is_i64(), "{}", shown["unfinished"]);
  assert_eq!(shown["unfinished"]["resolution"], WHY);
  assert!(
    line_marks.contains(&"unfinished_resolved".to_owned())
      && !line_marks.contains(&"unfinished".to_owned()),
    "{line_marks:?}"
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_that_left_no_note_has_none_to_resolve() {
  let (bench, id) = approved_item().await;

  bench.todo(&["--done", &id.to_string()]).await.answer();

  assert_eq!(bench.todo(&["--resolve", &id.to_string(), "--why", WHY]).await.refused, "no_unfinished");
  assert_eq!(bench.field_number(id, "unfinished_resolved_at").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_already_resolved_is_refused_and_the_line_it_was_resolved_with_stands() {
  let (bench, id) = closed().await;

  bench.todo(&["--resolve", &id.to_string(), "--why", WHY]).await.answer();

  assert_eq!(
    bench.todo(&["--resolve", &id.to_string(), "--why", "write it over"]).await.refused,
    "unfinished_resolved_already"
  );
  assert_eq!(bench.field(id, "unfinished_resolution").await.as_deref(), Some(WHY));
}

#[tokio::test(flavor = "multi_thread")]
async fn resolving_with_nothing_to_say_writes_nothing() {
  let (bench, id) = closed().await;

  assert_eq!(bench.todo(&["--resolve", &id.to_string()]).await.refused, "resolve_needs_why");
  assert_eq!(
    bench.todo(&["--resolve", &id.to_string(), "--why", "   "]).await.refused,
    "resolve_needs_why"
  );
  assert_eq!(bench.field_number(id, "unfinished_resolved_at").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_again_with_the_same_note_leaves_its_resolution_standing() {
  let (bench, id) = closed().await;

  bench.todo(&["--resolve", &id.to_string(), "--why", WHY]).await.answer();
  bench.todo(&["--done", &id.to_string()]).await.answer();

  assert_eq!(bench.field(id, "unfinished_resolution").await.as_deref(), Some(WHY));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_closing_that_writes_a_different_note_writes_a_new_one_and_the_old_resolution_goes() {
  let (bench, id) = closed().await;
  let other = format!("{} something else is left this time.", owner_mark(&bench));

  bench.todo(&["--resolve", &id.to_string(), "--why", WHY]).await.answer();
  bench.todo(&["--done", &id.to_string(), "--unfinished", &other]).await.answer();

  assert_eq!(bench.field(id, "unfinished").await, Some(other));
  assert_eq!(bench.field_number(id, "unfinished_resolved_at").await, None);
  assert_eq!(bench.field(id, "unfinished_resolution").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn reopening_takes_the_resolution_back_along_with_the_note_it_was_about() {
  let (bench, id) = closed().await;

  bench.todo(&["--resolve", &id.to_string(), "--why", WHY]).await.answer();
  bench.todo(&["--reopen", &id.to_string()]).await.answer();

  assert_eq!(bench.field(id, "unfinished").await, None);
  assert_eq!(bench.field_number(id, "unfinished_resolved_at").await, None);
  assert_eq!(bench.field(id, "unfinished_resolution").await, None);
}

fn source_note(bench: &Bench) -> String {
  format!("{} this and that are left.", owner_mark(bench))
}

// An item closed with a note of the owner's.
async fn with_note() -> (Bench, i64) {
  let bench = Bench::new().await;
  let id = bench.item("an item that left a note", "Conclusion\n\nDo this.").await;

  close(&bench, id, "All done.", &source_note(&bench)).await.answer();
  (bench, id)
}

#[tokio::test(flavor = "multi_thread")]
async fn resolving_and_reopening_a_note_and_dropping_a_row_say_so_in_their_answers() {
  let (bench, id) = with_note().await;
  let resolved = bench.todo(&["--resolve", &id.to_string(), "--why", "It stood as another row."]).await;

  assert_eq!(resolved.answer()["code"], "unfinished_resolved");
  assert!(bench.field_number(id, "unfinished_resolved_at").await.is_some());
  assert_eq!(bench.todo(&["--reopen", &id.to_string()]).await.answer()["code"], "item_reopened");
  assert!(!bench.done(id).await);

  let dropped = bench.todo(&["--drop", &id.to_string()]).await;

  assert_eq!(
    dropped.answer(),
    &json!({ "code": "item_dropped", "id": id, "title": "an item that left a note" })
  );
  assert_eq!(bench.count(r#"SELECT COUNT(*) FROM "todos""#, vec![]).await, 0);
}

// An item closed with a note, and as many rows raised out of it as the test wants.
async fn broken(rows: usize) -> (Bench, i64, Vec<i64>) {
  let (bench, source) = with_note().await;
  let mut raised = vec![];

  for at in 1..=rows {
    let title = format!("a line raised from the note {at}");
    let ran = bench
      .todo(&[
        "--add",
        "--title",
        &title,
        "--chat",
        &OWNER_CHAT.to_string(),
        "--judgement",
        "This line is still work.",
        "--from-todo",
        &source.to_string(),
        "--raised-by",
        &source.to_string(),
      ])
      .await;

    assert_eq!(ran.answer()["code"], "item_raised");
    raised.push(bench.last().await);
  }

  (bench, source, raised)
}

async fn raised_from(bench: &Bench, source: i64) -> Vec<i64> {
  job_base::all(
    bench.served.db(),
    r#"SELECT "todo_id" FROM "todo_sources" WHERE "source_todo_id" = $1 ORDER BY "todo_id""#,
    vec![source.into()],
  )
  .await
  .unwrap()
  .iter()
  .map(|row| row.try_get::<i64>("", "todo_id").unwrap())
  .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_raising_writes_the_link_so_the_note_knows_what_it_was_broken_into() {
  let (bench, source, raised) = broken(2).await;

  assert_eq!(raised_from(&bench, source).await, raised);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reading_names_the_rows_its_note_was_broken_into_and_how_far_each_got() {
  let (bench, source, raised) = broken(2).await;

  bench
    .todo(&["--done", &raised[0].to_string(), "--report", "One strand done."])
    .await
    .answer();

  let shown = bench.shown(source).await;

  assert_eq!(
    shown["unfinished"]["raised"],
    json!([
      { "id": raised[0], "title": "a line raised from the note 1", "state": "done" },
      { "id": raised[1], "title": "a line raised from the note 2", "state": "open" },
    ])
  );
  assert_eq!(bench.field(source, "unfinished").await, Some(source_note(&bench)));
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_one_of_two_rows_settles_nothing() {
  let (bench, source, raised) = broken(2).await;
  let ran = bench.todo(&["--done", &raised[0].to_string(), "--report", "One strand done."]).await;

  assert!(
    ran.answers.iter().all(|answer| answer["code"] != "source_resolved"),
    "{:?}",
    ran.answers
  );
  assert_eq!(bench.field_number(source, "unfinished_resolved_at").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn closing_the_last_of_them_settles_the_note_and_says_which_rows_did_it() {
  let (bench, source, raised) = broken(2).await;

  bench
    .todo(&["--done", &raised[0].to_string(), "--report", "One strand done."])
    .await
    .answer();

  let ran = bench.todo(&["--done", &raised[1].to_string(), "--report", "The rest done."]).await;
  let settled = ran
    .answers
    .iter()
    .find(|answer| answer["code"] == "source_resolved")
    .expect("the note is settled");

  assert_eq!(settled["id"], source);
  assert!(bench.field_number(source, "unfinished_resolved_at").await.is_some());

  let resolution = bench.field(source, "unfinished_resolution").await.unwrap();

  assert!(
    resolution.contains(&format!("#{}", raised[0])) && resolution.contains(&format!("#{}", raised[1])),
    "{resolution}"
  );
  // The note itself is never taken off the row: settling is a mark, not a deletion.
  assert_eq!(bench.field(source, "unfinished").await, Some(source_note(&bench)));
  assert!(bench.shown(source).await["unfinished"]["resolved_at"].is_i64());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_row_open_again_puts_the_note_back_to_outstanding() {
  let (bench, source, raised) = broken(1).await;

  bench.todo(&["--done", &raised[0].to_string(), "--report", "Done."]).await.answer();
  assert!(bench.field_number(source, "unfinished_resolved_at").await.is_some());

  let ran = bench.todo(&["--reopen", &raised[0].to_string()]).await;

  assert_eq!(ran.answer()["sources_reopened"], json!([source]));
  assert_eq!(bench.field_number(source, "unfinished_resolved_at").await, None);
  assert_eq!(bench.field(source, "unfinished").await, Some(source_note(&bench)));
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_the_last_outstanding_row_settles_the_note_as_a_closing_would() {
  let (bench, source, raised) = broken(2).await;

  bench
    .todo(&["--done", &raised[0].to_string(), "--report", "One strand done."])
    .await
    .answer();

  let ran = bench.todo(&["--drop", &raised[1].to_string()]).await;

  assert!(
    ran
      .answers
      .iter()
      .any(|answer| answer["code"] == "source_resolved" && answer["id"] == source),
    "{:?}",
    ran.answers
  );
  assert!(bench.field_number(source, "unfinished_resolved_at").await.is_some());
  assert_eq!(raised_from(&bench, source).await, [raised[0]]);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_nobody_broke_into_rows_is_left_to_whoever_reads_it() {
  let (bench, source, _) = broken(0).await;

  assert!(bench.field(source, "unfinished").await.is_some());
  assert_eq!(bench.field_number(source, "unfinished_resolved_at").await, None);
  assert!(marks(&listed_line(&bench, source).await).contains(&"unfinished".to_owned()));
}
