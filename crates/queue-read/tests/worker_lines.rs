//! A worker's reading of an item (docs/reading.md, "A worker's reading"). Words in Korean are written as
//! escapes, so the test's own text stays English.

mod support;

use queue_read::blocks::{
  CONVERSATION_HEAD, PART_ORDER, SAID_INSTRUCTION, SAID_REPLY, own_sections, render_block,
};
use queue_read::lines::Wanted;
use queue_read::queue::JobQuestion;
use queue_read::worker_lines::{
  Kept, UNTRANSLATED_MARK, fenced_blocks, instruction_reading, item_record, job_author, line_reading,
  list_texts, reads_as_english, show_texts, todo_list_worker_lines, todo_show_worker_lines, without_fences,
  worker_reading, worker_settlement, written_by_job,
};

/// "Fix the launcher so the job starts", in Korean.
const KOREAN: &str =
  "\u{b7f0}\u{cc98}\u{b97c} \u{ace0}\u{cce4}\u{c11c} Job\u{c774} \u{b728}\u{ac8c} \u{d574}.";

/// "Fix it", in Korean.
const FIX_IT: &str = "\u{ace0}\u{cce4} \u{c918}";

#[test]
fn a_jobs_own_words_are_told_by_the_name_they_were_written_under() {
  assert_eq!(job_author(Some(12)), "Job 12");
  assert_eq!(job_author(None), "Job");
  assert!(written_by_job("Job 12"));
  assert!(written_by_job("Job"));
  assert!(!written_by_job("Jobs"));
  assert!(!written_by_job("Job twelve"));
  assert!(!written_by_job("Guest"));
  assert!(item_record("Job 12", true));
  assert!(!item_record("Job 12", false));
  assert!(!item_record("Guest", true));
}

#[test]
fn a_text_reads_as_english_by_its_letters_outside_code() {
  assert!(reads_as_english("Fix the launcher so the job starts."));
  assert!(!reads_as_english(KOREAN));
  assert!(reads_as_english(&format!("Fix it:\n```\n{KOREAN} {KOREAN}\n```\nand go.")));
  assert!(reads_as_english(&format!("Rename `{KOREAN}` to the English one.")));
}

#[test]
fn the_fenced_blocks_are_the_ones_without_fences_takes_out() {
  let text = "Said:\n```text\nkeep me\n```\nthen\n````\nlong\n```\nnot yet\n````\n```\nnever closed";

  assert_eq!(fenced_blocks(text), vec!["```text\nkeep me\n```", "````\nlong\n```\nnot yet\n````"]);
  assert_eq!(without_fences(text), "Said:\n\nthen\n\n```\nnever closed");
  assert!(fenced_blocks("No fence here.").is_empty());
}

#[test]
fn a_korean_text_stands_as_its_kept_english_or_under_the_mark() {
  let mut kept = Kept::new();

  kept.insert(FIX_IT.to_owned(), "Fix it".to_owned());

  assert_eq!(worker_reading(&kept, FIX_IT), "Fix it");
  assert_eq!(worker_reading(&kept, "Already English"), "Already English");
  assert_eq!(worker_reading(&kept, KOREAN), format!("{UNTRANSLATED_MARK}\n{KOREAN}"));
  assert_eq!(line_reading(&kept, KOREAN), format!("{KOREAN} [not translated]"));
}

// The conversation is for a person to read: the worker reads the item's own sections alone.
#[test]
fn an_unkept_instruction_reads_its_own_sections_marked_where_any_word_wants_english() {
  let [_, conclusion, to_do] = PART_ORDER;
  let at = "2026-10-12T05:42:45.000Z";
  let instruction = [
    CONVERSATION_HEAD.to_owned(),
    render_block(SAID_INSTRUCTION, &format!("Owner, {at}"), KOREAN),
    render_block(SAID_REPLY, &format!("Agent, {at}"), "On it."),
    render_block(conclusion, "written as the item was filed", KOREAN),
    render_block("Judgement", &format!("Agent, {at}"), "Apart."),
    "A loose paragraph.".to_owned(),
  ]
  .join("\n\n");
  let own = [
    render_block(conclusion, "written as the item was filed", KOREAN),
    render_block("Judgement", &format!("Agent, {at}"), "Apart."),
    "A loose paragraph.".to_owned(),
  ]
  .join("\n\n");
  let mut kept = Kept::new();

  assert_eq!(instruction_reading(&kept, &instruction), format!("{UNTRANSLATED_MARK}\n{own}"));
  assert!(!instruction_reading(&kept, &instruction).contains("On it."));

  let english = render_block(to_do, "written as the item was filed", "Fix the launcher.");

  assert_eq!(instruction_reading(&kept, &english), english);

  kept.insert(own_sections(&instruction), "All of it in English".to_owned());

  assert_eq!(instruction_reading(&kept, &instruction), "All of it in English");
}

#[test]
fn a_settled_question_names_the_owner_as_the_deployment_names_them() {
  let names = support::names();
  let asked = JobQuestion {
    question: "Which unit?".to_owned(),
    options: vec!["the old one".to_owned(), "the new one".to_owned()],
    recommended: 0,
    chosen: None,
    answer: None,
    settled_at: None,
    settled_by: None,
  };
  let chosen = JobQuestion {
    chosen: Some(1),
    settled_at: Some(1),
    settled_by: Some("owner".to_owned()),
    ..asked.clone()
  };
  let defaulted = JobQuestion {
    settled_at: Some(1),
    settled_by: Some("timeout".to_owned()),
    ..asked.clone()
  };
  let typed = JobQuestion {
    answer: Some("Neither.".to_owned()),
    settled_at: Some(1),
    ..asked.clone()
  };

  assert_eq!(worker_settlement(&asked, &names), "still waiting");
  assert_eq!(worker_settlement(&chosen, &names), "2. the new one (chosen by Owner)");
  assert_eq!(
    worker_settlement(&defaulted, &names),
    "1. the old one (no answer, the recommendation stood)"
  );
  assert_eq!(worker_settlement(&typed, &names), "Neither. (written by Owner)");
}

#[tokio::test]
async fn a_workers_listing_is_a_line_per_open_row() {
  let (_scratch, db) = support::seeded().await;
  let queue = support::opened(&db).await;
  let facing = support::facing(&queue);
  let kept = Kept::new();
  let lines = todo_list_worker_lines(queue.all_todos(), &Wanted::default(), &facing, &kept);

  assert_eq!(
    lines,
    vec![
      "#1 Fix the launcher \u{b7} pending [claude-code/claude-opus-5-5] \u{b7} web \u{b7} has a report \u{b7} has an \
       unfinished note",
      "#4 Split the reader \u{b7} pending [registered/flash (fallback) (changed)] \u{b7} co-work \u{b7} winding down \
       (waiting for review)",
      "#5 Repair the launcher fix \u{b7} awaiting approval (Guest)",
      "#6 Write the launcher docs \u{b7} pending",
    ]
  );

  let none = Wanted {
    held_only: true,
    chat_id: Some(99),
    ..Wanted::default()
  };

  assert!(list_texts(queue.all_todos()).is_empty());
  assert_eq!(todo_list_worker_lines(&[], &none, &facing, &kept), vec!["todo: nothing to show"]);
}

#[tokio::test]
async fn an_opened_item_reads_as_its_own_sections_its_record_and_its_questions() {
  let (_scratch, db) = support::seeded().await;
  let queue = support::opened(&db).await;
  let facing = support::facing(&queue);
  let item = queue.get_todo(1).unwrap();
  let read = todo_show_worker_lines(&queue, item, &facing, &Kept::new(), &support::names()).join("\n");

  assert!(read.starts_with("#1 Fix the launcher\nState: pending\n"));
  assert!(read.contains("Merged into this item:\n#2 Tidy the launcher logs (merged)"));
  assert!(read.contains("Items repairing this one:\n#5 Repair the launcher fix (open)"));
  assert!(read.contains("Model: claude-code/claude-opus-5-5\n"));
  assert!(read.contains("Worker: web\n"));
  assert!(read.contains(&render_block("What to do", "Agent", "Fix the launcher.")));
  // The conversation is left out of what a worker reads.
  assert!(!read.contains("The launcher is broken."));
  assert!(read.contains("Work order draft (written apart from the item):\nPurpose:\nA purpose of its own"));
  // Only the words the item's own jobs wrote are its record.
  assert!(read.contains("The launcher reads its unit now."));
  assert!(!read.contains("Check the docs too."));
  assert!(!read.contains("Narrowed to the launcher."));
  assert!(read.contains("- Which unit? \u{2192} 2. the new one (chosen by Owner)"));
  assert!(read.contains("- Restart it? \u{2192} still waiting"));
  assert!(read.contains("Completion report:\nFixed most of it."));
  assert!(read.contains("Unfinished note:\nThe docs are left."));
  assert!(read.contains(
    "Rows already raised from this unfinished note (not to be raised again):\n#6 Write the launcher docs (open)"
  ));
  assert!(show_texts(&queue, item).is_empty());
}

#[tokio::test]
async fn a_launched_item_reads_as_its_brief_and_the_agents_changes_to_it() {
  let (_scratch, db) = support::seeded().await;

  job_base::run(&db, "UPDATE todos SET brief = 'Fix the launcher unit.' WHERE id = 1", vec![])
    .await
    .unwrap();

  let queue = support::opened(&db).await;
  let facing = support::facing(&queue);
  let read =
    todo_show_worker_lines(&queue, queue.get_todo(1).unwrap(), &facing, &Kept::new(), &support::names())
      .join("\n");

  assert!(read.contains(&render_block(
    "Brief",
    "Agent's organized instruction for this item",
    "Fix the launcher unit."
  )));
  assert!(read.contains("Changes Agent made to the brief:"));
  assert!(read.contains("Narrowed to the launcher."));
  assert!(read.contains("handed 2026-09-20T11:09:00.000Z"));
  assert!(!read.contains("Work order draft"));
}

#[tokio::test]
async fn the_texts_an_opened_item_looks_up_are_those_that_want_english() {
  let (_scratch, db) = support::seeded().await;

  job_base::run(
    &db,
    "UPDATE todos SET title = $1, report = $2 WHERE id = 1",
    vec![KOREAN.into(), FIX_IT.into()],
  )
  .await
  .unwrap();

  let queue = support::opened(&db).await;
  let item = queue.get_todo(1).unwrap();
  let mut kept = Kept::new();

  assert_eq!(show_texts(&queue, item), vec![KOREAN.to_owned(), FIX_IT.to_owned()]);
  assert_eq!(list_texts(queue.all_todos()), vec![KOREAN.to_owned()]);

  kept.insert(KOREAN.to_owned(), "Fix the launcher".to_owned());

  let read = todo_show_worker_lines(&queue, item, &support::facing(&queue), &kept, &support::names());

  assert_eq!(read[0], "#1 Fix the launcher");
  assert!(read.join("\n").contains(&format!("Completion report:\n{UNTRANSLATED_MARK}\n{FIX_IT}")));
}
