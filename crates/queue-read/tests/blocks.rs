//! The blocks of an instruction (docs/reading.md, "The blocks of an instruction").

use queue_read::blocks::{
  CONVERSATION_HEAD, SAID_INSTRUCTION, draft_repeats_item, ordered_instruction, own_sections, render_block,
  segments,
};

#[test]
fn blocks_and_loose_text_are_read_apart_and_put_in_the_reading_order() {
  let instruction = [
    render_block("What to do", "Agent", "1. Fix it"),
    CONVERSATION_HEAD.to_owned(),
    render_block(SAID_INSTRUCTION, "Owner", "Fix it please\n```\ninside\n```"),
    render_block("Conclusion", "Agent", "Fix it"),
  ]
  .join("\n\n");
  let ordered = ordered_instruction(&instruction);

  assert_eq!(segments(&instruction).len(), 4);
  assert!(ordered.starts_with(CONVERSATION_HEAD));
  assert!(ordered.find("Conclusion (").unwrap() < ordered.find("What to do (").unwrap());
  assert!(ordered.contains("````text\nFix it please\n```\ninside\n```\n````"));
}

#[test]
fn the_own_sections_leave_the_conversation_out_and_keep_every_loose_paragraph() {
  let instruction = [
    "A loose paragraph.".to_owned(),
    CONVERSATION_HEAD.to_owned(),
    render_block(SAID_INSTRUCTION, "Owner", "Fix it please"),
    render_block("What to do", "Agent", "Fix it"),
  ]
  .join("\n\n");

  assert_eq!(
    own_sections(&instruction),
    format!("{}\n\nA loose paragraph.", render_block("What to do", "Agent", "Fix it"))
  );
}

#[test]
fn a_block_never_closed_is_loose_text() {
  let read = segments("Conclusion (Agent):\n```text\nnever closed");

  assert_eq!(read.len(), 1);
  assert_eq!(read[0].label, None);
}

#[test]
fn a_draft_repeating_the_items_own_blocks_is_said_to_repeat_them() {
  let instruction =
    [render_block("Conclusion", "Agent", "one"), render_block("What to do", "Agent", "two")].join("\n\n");

  assert!(draft_repeats_item(&instruction, "one", "two"));
  assert!(!draft_repeats_item(&instruction, "one", "three"));
}
