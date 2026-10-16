//! The shape an item's instruction is written in (docs/reading.md, "The blocks of an instruction"):
//! labelled blocks fenced with backticks, and loose paragraphs between them. The shape is the service's
//! own, so it is read here by hand rather than by any general reader.

use std::sync::LazyLock;

use regex::Regex;

/// The labels a copied message carries: the owner's words, and the agent's answer to them.
pub const SAID_INSTRUCTION: &str = "Instruction as said";
pub const SAID_REPLY: &str = "Reply as said";
/// The line over the copied conversation.
pub const CONVERSATION_HEAD: &str = "The conversation, word for word:";
/// The agent's own parts, in the order an item is read after its conversation: what the work is, what it
/// came to, and what is to be done.
pub const PART_ORDER: [&str; 3] = ["What the work is", "Conclusion", "What to do"];

// A block's head, read only where a block may start: its label, where its words came from, and the
// fence that opens it.
static HEAD: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^(.+?) \(([^\n]*)\):\n(`{3,})text\n").expect("a block's head"));

/// One stretch of an instruction: a block with its label, or loose text with none.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment<'a> {
  pub label: Option<&'a str>,
  /// Where a block's words came from, as its head says; loose text has none.
  pub source: Option<&'a str>,
  pub text: &'a str,
  pub start: usize,
  pub end: usize,
}

fn fence_for(text: &str) -> String {
  let mut longest = 0;
  let mut run = 0;

  for character in text.chars() {
    run = if character == '`' { run + 1 } else { 0 };
    longest = longest.max(run);
  }

  "`".repeat(3.max(longest + 1))
}

/// One block: a label, where the words came from, and the words inside a fence longer than any run
/// of backticks they hold.
pub fn render_block(label: &str, source: &str, text: &str) -> String {
  let fence = fence_for(text);

  format!("{label} ({source}):\n{fence}text\n{text}\n{fence}")
}

/// The instruction from the top, as the blocks and the loose paragraphs it stands in. Nothing is
/// dropped: a paragraph that is not a block comes back as loose text.
pub fn segments(instruction: &str) -> Vec<Segment<'_>> {
  let mut found = Vec::new();
  let mut at = 0;

  while at < instruction.len() {
    let Some(head) = HEAD.captures(&instruction[at..]) else {
      match instruction[at..].find("\n\n") {
        None => {
          loose_text(instruction, &mut found, at, instruction.len());
          break;
        }
        Some(gap) => {
          loose_text(instruction, &mut found, at, at + gap);
          at += gap + 2;
          continue;
        }
      }
    };

    let whole = head[0].len();
    let label = head.get(1).map_or("", |label| label.as_str());
    let fence = &head[3];
    let body_start = at + whole;
    let closing = format!("\n{fence}");
    let find_from = |from: usize| instruction[from..].find(&closing).map(|offset| from + offset);
    let mut close = find_from(body_start - 1);

    // The fence is longer than any run inside, so the first line that is the fence and nothing
    // more is where the block ends.
    while let Some(at_close) = close {
      if instruction.as_bytes().get(at_close + 1 + fence.len()) == Some(&b'`') {
        close = find_from(at_close + 1);
      } else {
        break;
      }
    }

    let Some(close) = close else {
      loose_text(instruction, &mut found, at, instruction.len());
      break;
    };

    let end = close + 1 + fence.len();

    found.push(Segment {
      label: Some(label),
      source: head.get(2).map(|source| source.as_str()),
      text: &instruction[body_start..body_start.max(close)],
      start: at,
      end,
    });
    at = if instruction[end..].starts_with("\n\n") { end + 2 } else { end };
  }

  found
}

// A stretch that is not a block, kept where it says anything.
fn loose_text<'a>(instruction: &'a str, found: &mut Vec<Segment<'a>>, start: usize, end: usize) {
  let text = instruction[start..end].trim();

  if !text.is_empty() {
    found.push(Segment {
      label: None,
      source: None,
      text,
      start,
      end,
    });
  }
}

fn rank(segment: &Segment<'_>) -> usize {
  let unnamed = PART_ORDER.len() + 1;

  match segment.label {
    None => {
      if segment.text == CONVERSATION_HEAD {
        0
      } else {
        unnamed
      }
    }
    Some(SAID_INSTRUCTION | SAID_REPLY) => 0,
    Some(label) => PART_ORDER.iter().position(|part| *part == label).map_or(unnamed, |at| at + 1),
  }
}

/// The instruction written out in the one reading order: the conversation, what kind of work it
/// is, what it came to, and what is to be done; everything else after, where it stood.
pub fn ordered_instruction(instruction: &str) -> String {
  let mut found = segments(instruction);

  found.sort_by_key(rank);

  found
    .iter()
    .map(|segment| instruction[segment.start..segment.end].trim())
    .collect::<Vec<_>>()
    .join("\n\n")
}

/// The instruction in the reading order with its copied conversation left out: the agent's own parts and
/// whatever else stood beside them, which is what a worker reads of an item no brief is written for yet.
pub fn own_sections(instruction: &str) -> String {
  let mut found: Vec<Segment<'_>> =
    segments(instruction).into_iter().filter(|segment| rank(segment) != 0).collect();

  found.sort_by_key(rank);

  found
    .iter()
    .map(|segment| instruction[segment.start..segment.end].trim())
    .collect::<Vec<_>>()
    .join("\n\n")
}

/// Whether a work order's draft still says only what the item's own blocks say.
pub fn draft_repeats_item(instruction: &str, intent: &str, draft: &str) -> bool {
  let own: Vec<&str> = segments(instruction)
    .iter()
    .filter(|segment| segment.label.is_some())
    .map(|segment| segment.text)
    .collect();

  own.contains(&intent) && own.contains(&draft)
}
