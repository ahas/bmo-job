//! An item's instruction as the TODO's verbs compose and rewrite it (docs/reading.md, "The blocks of an
//! instruction"): the three parts the filer writes around the copied conversation, a raised row's
//! judgement and the strands it came out of, each a block with its label and its source. The order every
//! reader reads the parts in is `queue-read`'s.

use std::sync::LazyLock;

use queue_read::blocks::{
  CONVERSATION_HEAD, PART_ORDER, SAID_INSTRUCTION, SAID_REPLY, Segment, ordered_instruction, render_block,
  segments,
};
use queue_read::dates::iso;
use regex::Regex;

use crate::js;

/// The label a raised row's judgement stands under; the console reads such rows by it.
pub const JUDGEMENT: &str = "Judgement";

/// The label of one strand of an unfinished note a raised row came out of.
pub const SOURCE: &str = "Source note";

/// The parts of an item's instruction its filer wrote, by the name a rewrite names each with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnBlock {
  What,
  ToDo,
  Conclusion,
  Judgement,
}

impl OwnBlock {
  /// The label the block stands under in the instruction.
  pub fn label(self) -> &'static str {
    match self {
      Self::What => PART_ORDER[0],
      Self::Conclusion => PART_ORDER[1],
      Self::ToDo => PART_ORDER[2],
      Self::Judgement => JUDGEMENT,
    }
  }
}

/// What a row a job raised names as what raised it.
pub fn raised_by_item(id: i64) -> String {
  format!("the work of item #{id}")
}

/// One message copied into an item: who said it, when in epoch milliseconds, and which kept message it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
  pub sender: String,
  pub sent_at: i64,
  pub message_id: Option<i64>,
  pub text: String,
}

fn render_entry(label: &str, entry: &Entry) -> String {
  let id = entry.message_id.map_or_else(String::new, |id| format!(", message {id}"));

  render_block(label, &format!("{}, {}{id}", entry.sender, iso(entry.sent_at)), &entry.text)
}

fn filed_by(filer: &str) -> String {
  format!("written by {filer} as it filed the row")
}

/// What an item filed out of a conversation says for itself.
pub struct Account<'a> {
  pub what: &'a str,
  pub to_do: &'a str,
  pub instructions: &'a [Entry],
  pub replies: &'a [Entry],
  pub conclusion: &'a str,
  /// Who wrote the three parts of its own.
  pub filer: &'a str,
}

/// The instruction of an item filed out of a conversation, its parts in the order every reader reads
/// them.
pub fn render_instruction(account: &Account<'_>) -> String {
  let by = filed_by(account.filer);
  let mut blocks = vec![
    render_block(OwnBlock::What.label(), &by, account.what),
    render_block(OwnBlock::ToDo.label(), &by, account.to_do),
    CONVERSATION_HEAD.to_owned(),
  ];

  blocks.extend(account.instructions.iter().map(|entry| render_entry(SAID_INSTRUCTION, entry)));
  blocks.extend(account.replies.iter().map(|entry| render_entry(SAID_REPLY, entry)));
  blocks.push(render_block(OwnBlock::Conclusion.label(), &by, account.conclusion));

  ordered_instruction(&blocks.join("\n\n"))
}

/// One strand of a note a raised row takes up, numbered as the note numbers it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PickedStrand {
  pub todo_id: i64,
  pub title: String,
  pub strand: usize,
  pub text: String,
}

/// The instruction of a row the agent raised itself: its judgement, when and by what it was made, and the
/// strand of each unfinished note it came out of.
pub fn render_raised_instruction(
  agent: &str,
  judgement: &str,
  raised_at: i64,
  raised_by: &str,
  sources: &[PickedStrand],
) -> String {
  let mut blocks =
    vec![render_block(JUDGEMENT, &format!("{agent}, {}, {raised_by}", iso(raised_at)), judgement)];

  blocks.extend(sources.iter().map(|source| {
    let line = format!("item #{} {}, strand {}", source.todo_id, source.title, source.strand);

    render_block(SOURCE, &line, &source.text)
  }));

  blocks.join("\n\n")
}

/// What the agent is told when a job raises a row: that the row is its own to approve and launch at once, and
/// the one thing that holds it back.
pub fn raised_row_instruction(
  id: i64,
  title: &str,
  raised_by: &str,
  sources: &[i64],
  unfinished: &str,
  owner: &str,
) -> String {
  let from = if sources.is_empty() {
    String::new()
  } else {
    format!(
      ", out of the {unfinished} of {}",
      sources.iter().map(|id| format!("#{id}")).collect::<Vec<_>>().join(", ")
    )
  };

  [
    format!("{raised_by} raised row #{id} {title}{from}."),
    format!(
      "A row raised this way is yours to approve and launch now: read it with todo_show {id}, approve it with todo_approve {id}, and launch its job with task the moment its worker is free."
    ),
    format!(
      "Hold it back only where it turns on what only {owner} can give (a password, an account of theirs, a judgement of their own taste) and then put that one thing to them."
    ),
    "Do not announce the instruction itself. What you say in the chat is that the row is approved and whether its job started."
      .to_owned(),
  ]
  .join("\n")
}

static HEAD: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(.+?) \(([^\n]*)\):\n").expect("a block's head"));

/// The text of one of the filer's own parts, where the instruction carries it at its top level.
pub fn own_block_text(instruction: &str, part: OwnBlock) -> Option<String> {
  segments(instruction)
    .into_iter()
    .find(|segment| segment.label == Some(part.label()))
    .map(|segment| segment.text.to_owned())
}

/// The instruction with one of the filer's own parts written again under its own label and source, or
/// none where it carries no such part.
pub fn rewrite_own_block(instruction: &str, part: OwnBlock, text: &str) -> Option<String> {
  let block = segments(instruction).into_iter().find(|segment| segment.label == Some(part.label()))?;

  with_block_text(instruction, &block, text)
}

/// The instruction with one of its blocks holding other words under the same label and source line, or
/// none where the stretch named is no block of it.
pub fn with_block_text(instruction: &str, block: &Segment<'_>, text: &str) -> Option<String> {
  let label = block.label?;
  let head = HEAD.captures(instruction.get(block.start..)?)?;
  let source = head.get(2).map_or("", |source| source.as_str());

  Some(format!(
    "{}{}{}",
    &instruction[..block.start],
    render_block(label, source, text),
    &instruction[block.end..]
  ))
}

/// A text on one line, cut where a line would otherwise run for pages.
pub fn clip_instruction(instruction: &str, length: usize) -> String {
  let flat = instruction
    .split(js::is_space)
    .filter(|word| !word.is_empty())
    .collect::<Vec<_>>()
    .join(" ");

  js::cut(&flat, length)
}
