//! A person's reading of an item (docs/reading.md, "A person's reading"), read off the seeded queue.

mod support;

use job_codes::todo_reading::{HeldWord, LineMark, ModelMark, QuestionState, RowState};
use queue_read::blocks::own_sections;
use queue_read::lines::Wanted;
use queue_read::person_reading::{Settled, held_word, item_line, item_lines, item_reading};
use queue_read::queue::HoldKind;

fn no_server(_: &str) -> Option<(String, String)> {
  None
}

#[test]
fn a_held_item_wears_the_word_of_who_set_it_aside_and_whether_its_run_still_ends() {
  assert_eq!(held_word(Some(HoldKind::Stop), true), HeldWord::Stopping);
  assert_eq!(held_word(Some(HoldKind::Stop), false), HeldWord::Stop);
  assert_eq!(held_word(Some(HoldKind::Pause), true), HeldWord::Pausing);
  assert_eq!(held_word(Some(HoldKind::Pause), false), HeldWord::Pause);
  assert_eq!(held_word(Some(HoldKind::Failure), true), HeldWord::Pending);
  assert_eq!(held_word(Some(HoldKind::Failure), false), HeldWord::Failure);
  assert_eq!(held_word(None, false), HeldWord::Plain);
}

#[tokio::test]
async fn a_listing_carries_each_open_row_with_its_state_and_marks() {
  let (_scratch, db) = support::seeded().await;
  let queue = support::opened(&db).await;
  let facing = support::facing(&queue);
  let lines = item_lines(queue.all_todos(), &Wanted::default(), &facing);

  // The finished row and the merged one are not listed unless asked for.
  assert_eq!(lines.iter().map(|line| line.id).collect::<Vec<_>>(), vec![1, 4, 5, 6]);

  let first = &lines[0];

  assert_eq!(first.state, None);
  assert_eq!(first.marks, vec![LineMark::Report, LineMark::Unfinished]);
  assert_eq!(first.worker.as_deref(), Some("web"));
  assert_eq!(
    first.model.as_ref().map(|model| model.shown.as_str()),
    Some("claude-code/claude-opus-5-5")
  );
  assert_eq!(first.model.as_ref().map(|model| model.marks.clone()), Some(vec![]));

  let held = &lines[1];

  assert_eq!(held.marks, vec![LineMark::Cowork]);
  assert_eq!(held.hold.as_ref().map(|hold| hold.word), Some(HeldWord::Pausing));
  assert_eq!(held.hold.as_ref().and_then(|hold| hold.reason.as_deref()), Some("waiting for review"));
  assert_eq!(
    held.model.as_ref().map(|model| model.marks.clone()),
    Some(vec![ModelMark::Fallback, ModelMark::Changed])
  );
  assert_eq!(
    lines[2].state,
    Some(LineMark::PendingApproval {
      requester: "Guest".to_owned()
    })
  );

  let merged = item_line(queue.get_todo(2).unwrap(), &facing);

  assert_eq!(merged.state, Some(LineMark::MergingInto { id: 1 }));

  let merged_only = Wanted {
    merged_only: true,
    ..Wanted::default()
  };

  assert_eq!(
    item_lines(queue.all_todos(), &merged_only, &facing)
      .iter()
      .map(|line| line.id)
      .collect::<Vec<_>>(),
    vec![2]
  );
}

#[tokio::test]
async fn an_opened_item_carries_its_links_its_words_its_questions_and_its_time() {
  let (_scratch, db) = support::seeded().await;
  let queue = support::opened(&db).await;
  let facing = support::facing(&queue);
  let item = queue.get_todo(1).unwrap();
  let read = item_reading(&queue, item, &facing, &no_server);

  assert_eq!(
    read.line.model.as_ref().map(|model| model.marks.clone()),
    Some(vec![]),
    "the road asked for shows alone"
  );
  assert_eq!(
    read.merged_here.iter().map(|row| (row.id, row.state)).collect::<Vec<_>>(),
    vec![(2, RowState::Merged)]
  );
  assert_eq!(
    read.repaired_by.iter().map(|row| (row.id, row.state)).collect::<Vec<_>>(),
    vec![(5, RowState::Opened)]
  );
  assert_eq!((read.worker.as_str(), read.worker_assigned), ("web", true));
  assert!(read.instruction.starts_with("The conversation, word for word:"));
  assert_ne!(read.instruction, own_sections(&item.instruction));
  assert_eq!(read.draft.as_ref().map(|draft| draft.intent.as_str()), Some("A purpose of its own"));
  assert_eq!(read.notes.iter().map(|note| note.id).collect::<Vec<_>>(), vec![1, 2, 3]);
  assert_eq!(read.merged_work.len(), 1);
  assert_eq!(read.merged_work[0].notes[0].text, "Keep the old logs.");
  assert_eq!(read.questions[0].state, QuestionState::Chosen);
  assert_eq!(
    read.questions[0].settled,
    Some(Settled {
      number: 2,
      option: Some("the new one".to_owned())
    })
  );
  assert_eq!(read.questions[1].state, QuestionState::Waiting);

  let time = read.time.unwrap();

  assert_eq!((time.attempts.len(), time.total_ms, time.missing), (2, 240_000, 1));
  assert_eq!(time.usage[0].model, "claude-opus-5-5");

  let unfinished = read.unfinished.unwrap();

  assert_eq!(unfinished.text, "The docs are left.");
  assert_eq!(
    unfinished.raised.iter().map(|row| (row.id, row.state)).collect::<Vec<_>>(),
    vec![(6, RowState::Open)]
  );

  let merged = item_reading(&queue, queue.get_todo(2).unwrap(), &facing, &no_server);
  let into = merged.merged_into.unwrap();

  assert_eq!((into.id, into.title.as_deref(), into.follows), (1, Some("Fix the launcher"), true));
  assert_eq!(merged.time, None);
}
