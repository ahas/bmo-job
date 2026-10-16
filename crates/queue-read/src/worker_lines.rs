//! A worker's reading of an item (docs/reading.md, "A worker's reading"): English lines, in which every
//! text a person or the agent wrote stands as its kept English translation, which the caller reads from
//! bmo-gate beforehand. Nothing here asks the translator, and what is not translated yet stands in its
//! original under a mark. The time an item's runs took is not here.

use std::collections::HashMap;
use std::sync::LazyLock;

use job_policy::Names;
use regex::Regex;

use crate::blocks::{draft_repeats_item, own_sections, render_block, segments};
use crate::dates::iso;
use crate::lines::{Facing, Wanted, todo_list_items, todo_worker};
use crate::queue::{HoldKind, JobQuestion, Queue, TodoItem, TodoNote};
use crate::ran_model::item_ran_model;

/// The line a text that is not translated yet stands under.
pub const UNTRANSLATED_MARK: &str = "[Not translated: this stands in its original language.]";
/// The mark a one-line text wears after it where it is not translated yet.
pub const UNTRANSLATED_INLINE: &str = "[not translated]";

/// The kept English of each original that has one, by the original, as bmo-gate answers it.
pub type Kept = HashMap<String, String>;

// The Hangul syllables a text is counted by.
const HANGUL_SYLLABLES: std::ops::RangeInclusive<char> = '\u{AC00}'..='\u{D7A3}';

// A fenced code block's opening line: up to three spaces, then a run of backticks or tildes.
static FENCE_OPEN: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^ {0,3}(`{3,}|~{3,})").expect("a fence's opening"));
static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`\n]*`").expect("inline code"));

// Whether a line closes a fence of exactly this run: up to three spaces, the run, and nothing after it
// but spaces and tabs.
fn closes(line: &str, run: &str) -> bool {
  let indent = line.len() - line.trim_start_matches(' ').len();

  indent <= 3
    && line[indent..]
      .strip_prefix(run)
      .is_some_and(|rest| rest.chars().all(|c| c == ' ' || c == '\t'))
}

// The fenced code blocks of a text's lines, each by its opening and closing line: a block runs from its
// opening line to the first later line closing a run of the same fence; an opening run longer than three
// may close on a shorter one when no line closes the longer one. An opening line nothing closes opens no
// block. Only the fences are read here, to leave code out of a count of letters, so this is not a reading
// of markdown.
fn fence_spans(lines: &[&str]) -> Vec<(usize, usize)> {
  let mut spans = Vec::new();
  let mut at = 0;

  while at < lines.len() {
    let closing = FENCE_OPEN.captures(lines[at]).and_then(|open| {
      let run = &open[1];
      let fence = &run[..1];

      (3..=run.len()).rev().find_map(|length| {
        let shorter = fence.repeat(length);

        (at + 1..lines.len()).find(|&later| closes(lines[later], &shorter))
      })
    });

    match closing {
      Some(end) => {
        spans.push((at, end));
        at = end + 1;
      }
      None => at += 1,
    }
  }

  spans
}

/// The text with its fenced code blocks taken out.
pub fn without_fences(text: &str) -> String {
  let lines: Vec<&str> = text.split('\n').collect();
  let mut kept: Vec<&str> = Vec::new();
  let mut at = 0;

  for (start, end) in fence_spans(&lines) {
    kept.extend(&lines[at..start]);
    // The block and its closing line go; the line break after the closing line stays.
    kept.push("");
    at = end + 1;
  }

  kept.extend(&lines[at..]);
  kept.join("\n")
}

/// The fenced code blocks of a text in order, each whole from its opening line to its closing line, read
/// as `without_fences` takes them out.
pub fn fenced_blocks(text: &str) -> Vec<String> {
  let lines: Vec<&str> = text.split('\n').collect();

  fence_spans(&lines)
    .into_iter()
    .map(|(start, end)| lines[start..=end].join("\n"))
    .collect()
}

/// Whether a text reads as English: its Hangul syllables times four fewer than its Latin letters, outside
/// code.
pub fn reads_as_english(text: &str) -> bool {
  let prose = without_fences(text);
  let prose = INLINE_CODE.replace_all(&prose, "");
  let hangul = prose.chars().filter(|c| HANGUL_SYLLABLES.contains(c)).count();
  let latin = prose.chars().filter(char::is_ascii_alphabetic).count();

  hangul * 4 < latin
}

// Whether a text needs a translation for the worker to read it.
fn wants_english(text: &str) -> bool {
  !text.trim().is_empty() && !reads_as_english(text)
}

/// A text as the worker reads it: as it stands where it is English, its kept English, or marked.
pub fn worker_reading(kept: &Kept, text: &str) -> String {
  if !wants_english(text) {
    return text.to_owned();
  }

  kept.get(text).cloned().unwrap_or_else(|| format!("{UNTRANSLATED_MARK}\n{text}"))
}

/// The same for a text that stands inside a line: the mark goes after it rather than over it.
pub fn line_reading(kept: &Kept, text: &str) -> String {
  if !wants_english(text) {
    return text.to_owned();
  }

  kept.get(text).cloned().unwrap_or_else(|| format!("{text} {UNTRANSLATED_INLINE}"))
}

/// An item's instruction as the worker reads it: its own sections, the copied conversation left out, as
/// their kept English, or as written, marked where any of their words still wants a translation.
pub fn instruction_reading(kept: &Kept, instruction: &str) -> String {
  let own = own_sections(instruction);

  if let Some(english) = kept.get(&own) {
    return english.clone();
  }

  if segments(&own).iter().any(|segment| wants_english(segment.text)) {
    format!("{UNTRANSLATED_MARK}\n{own}")
  } else {
    own
  }
}

/// The word a held item wears, by who set it aside and whether its run still ends.
pub fn worker_held_label(kind: Option<HoldKind>, pending: bool) -> &'static str {
  match (kind, pending) {
    (Some(HoldKind::Stop), true) => "stopping",
    (Some(HoldKind::Stop), false) => "stopped",
    (Some(HoldKind::Pause), true) => "winding down",
    (Some(HoldKind::Pause), false) => "waiting to resume",
    (_, true) => "waiting for its run to end before it is held",
    (Some(HoldKind::Failure), false) => "held after failing",
    (None, false) => "held",
  }
}

fn merged_mark(item: &TodoItem) -> String {
  let into = item.merged_into.unwrap_or_default();

  if item.merged() {
    format!("merging into #{into}")
  } else {
    format!("merged into #{into}")
  }
}

/// The model an item wears: the road its work last went out on with its fallback and changed marks, and
/// in an opened item the model it asked for where the two differ. None where it asked for the default and
/// nothing has gone out yet.
fn item_model(item: &TodoItem, facing: &Facing, opened: bool) -> Option<String> {
  item_ran_model(item.model.as_deref(), item.ran_on.as_ref(), &facing.providers).map(|model| {
    let requested = model.requested.as_ref().filter(|_| opened);

    [
      Some(model.shown.clone()),
      model.fallback.then(|| "(fallback)".to_owned()),
      model.changed.then(|| "(changed)".to_owned()),
      requested.map(|requested| format!("(requested {requested})")),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ")
  })
}

/// One item as a worker's listing shows it.
pub fn todo_worker_line(item: &TodoItem, facing: &Facing, kept: &Kept) -> String {
  let mark = if item.merged() {
    merged_mark(item)
  } else if item.done {
    format!(
      "done{}",
      if item.merged_into.is_none() {
        String::new()
      } else {
        format!(" ({})", merged_mark(item))
      }
    )
  } else if item.approved {
    line_reading(kept, &item.status)
  } else {
    format!("awaiting approval ({})", item.requester)
  };
  let model = item_model(item, facing, false).map_or_else(String::new, |model| format!(" [{model}]"));
  let worker = if item.worker.is_none() {
    String::new()
  } else {
    format!(" \u{b7} {}", todo_worker(item, &facing.workers))
  };
  let cowork = if item.cowork { " \u{b7} co-work" } else { "" };
  let report = if item.report.is_none() { "" } else { " \u{b7} has a report" };
  let unfinished = match (&item.unfinished, item.unfinished_resolved_at) {
    (None, _) => "",
    (Some(_), None) => " \u{b7} has an unfinished note",
    (Some(_), Some(_)) => " \u{b7} unfinished note resolved",
  };
  let held = match item.held_at {
    None => String::new(),
    Some(_) => format!(
      " \u{b7} {}{}",
      worker_held_label(item.held_kind, facing.stopping.contains(&item.id)),
      item
        .held_reason
        .as_ref()
        .map_or_else(String::new, |reason| format!(" ({})", line_reading(kept, reason))),
    ),
  };

  format!(
    "#{} {} \u{b7} {mark}{model}{worker}{cowork}{held}{report}{unfinished}",
    item.id,
    line_reading(kept, &item.title)
  )
}

/// What a worker's listing prints: a line per row, or the one line saying there are none.
pub fn todo_list_worker_lines(
  items: &[TodoItem],
  wanted: &Wanted,
  facing: &Facing,
  kept: &Kept,
) -> Vec<String> {
  let shown = todo_list_items(items, wanted, facing);

  if shown.is_empty() {
    vec!["todo: nothing to show".to_owned()]
  } else {
    shown.iter().map(|item| todo_worker_line(item, facing, kept)).collect()
  }
}

/// The texts a worker's listing looks up: each listed row's title, state and reason for a hold.
pub fn list_texts(items: &[TodoItem]) -> Vec<String> {
  items
    .iter()
    .flat_map(|item| [Some(item.title.clone()), Some(item.status.clone()), item.held_reason.clone()])
    .flatten()
    .filter(|text| wants_english(text))
    .collect()
}

/// The kind of note that is the agent's change to an item's brief, apart from the words people and jobs
/// add.
pub const CHANGE_NOTE: &str = "change";

/// Whether a note is the agent's change to the brief.
pub fn is_change(note: &TodoNote) -> bool {
  note.kind.as_deref() == Some(CHANGE_NOTE)
}

/// The name a job's own words onto its item are written under: `Job` and the number of the job running
/// it.
pub fn job_author(job: Option<i64>) -> String {
  job.map_or_else(|| "Job".to_owned(), |job| format!("Job {job}"))
}

/// Whether words on an item are a job's, by the name they were written under, apart from the words a
/// person added.
pub fn written_by_job(author: &str) -> bool {
  author == "Job"
    || author
      .strip_prefix("Job ")
      .is_some_and(|job| !job.is_empty() && job.bytes().all(|byte| byte.is_ascii_digit()))
}

/// Whether words on an item are its record, which a later run reads: written under a job's name by a job
/// of the item. A job's words on another item's row are read there like anybody else's.
pub fn item_record(author: &str, with_items_job: bool) -> bool {
  with_items_job && written_by_job(author)
}

/// The agent's changes to an item's brief as a worker reads them: each with when it was written and
/// whether a running job has been handed it.
pub fn changes_for_worker(changes: &[&TodoNote], kept: &Kept) -> String {
  changes
    .iter()
    .map(|change| {
      let handed = change
        .delivered_at
        .map_or_else(|| "not handed to a running job".to_owned(), |at| format!("handed {}", iso(at)));

      render_block(
        "Change",
        &format!("note {}, {}, {handed}", change.id, iso(change.added_at)),
        &worker_reading(kept, &change.text),
      )
    })
    .collect::<Vec<_>>()
    .join("\n\n")
}

// The brief and the agent's changes to it, which stand in place of the item's own sections once a job was
// launched.
fn brief_lines(brief: &str, changes: &[&TodoNote], kept: &Kept, names: &Names) -> Vec<String> {
  let agent = &names.agent;
  let mut lines =
    vec![render_block("Brief", &format!("{agent}'s organized instruction for this item"), brief)];

  if !changes.is_empty() {
    lines.push(String::new());
    lines.push(format!("Changes {agent} made to the brief:"));
    lines.push(changes_for_worker(changes, kept));
  }

  lines
}

pub fn notes_for_worker(notes: &[&TodoNote], kept: &Kept) -> String {
  notes
    .iter()
    .map(|note| {
      render_block(
        "Added words",
        &format!("note {}, {}, {}", note.id, note.author, iso(note.added_at)),
        &worker_reading(kept, &note.text),
      )
    })
    .collect::<Vec<_>>()
    .join("\n\n")
}

fn merge_lines(queue: &Queue, item: &TodoItem, kept: &Kept) -> Vec<String> {
  let mut lines = Vec::new();

  if let Some(into) = item.merged_into {
    let title = queue.get_todo(into).map_or_else(String::new, |target| target.title.clone());

    lines.push(format!(
      "Merged into: #{into} {}{}{}",
      line_reading(kept, &title),
      item.merged_at.map_or_else(String::new, |at| format!(" at {}", iso(at))),
      if item.merged() { "; it is finished when that item is" } else { "" },
    ));
  }

  let merged = queue.todos_merged_into(item.id);

  if !merged.is_empty() {
    lines.push("Merged into this item:".to_owned());
    lines.extend(merged.iter().map(|row| {
      format!(
        "#{} {} ({})",
        row.id,
        line_reading(kept, &row.title),
        if row.done { "done" } else { "merged" }
      )
    }));
  }

  lines
}

fn repair_lines(queue: &Queue, item: &TodoItem, kept: &Kept) -> Vec<String> {
  let mut lines = Vec::new();

  if let Some(target) = item.repairs_todo_id {
    let title = queue
      .get_todo(target)
      .map_or_else(String::new, |found| format!(" {}", line_reading(kept, &found.title)));

    lines.push(format!("Repairs: #{target}{title}"));
  }

  let repairs = queue.todo_repairs(item.id);

  if !repairs.is_empty() {
    lines.push("Items repairing this one:".to_owned());
    lines.extend(repairs.iter().map(|row| {
      let state = if row.done {
        "done"
      } else if row.held_at.is_some() {
        "held"
      } else {
        "open"
      };

      format!("#{} {} ({state})", row.id, line_reading(kept, &row.title))
    }));
  }

  lines
}

fn order_lines(queue: &Queue, item: &TodoItem, kept: &Kept) -> Vec<String> {
  match queue.work_order(item.id) {
    Some(order)
      if order.status == "draft"
        && !draft_repeats_item(&item.instruction, &order.intent, &order.instruction) =>
    {
      vec![
        String::new(),
        "Work order draft (written apart from the item):".to_owned(),
        format!("Purpose:\n{}", worker_reading(kept, &order.intent)),
        format!("Instruction:\n{}", worker_reading(kept, &order.instruction)),
      ]
    }
    _ => Vec::new(),
  }
}

/// What one question became, in the worker's words, the owner named as the deployment names them.
pub fn worker_settlement(question: &JobQuestion, names: &Names) -> String {
  let owner = &names.owner;

  if question.settled_at.is_none() {
    return "still waiting".to_owned();
  }

  if let Some(answer) = &question.answer {
    return format!("{answer} (written by {owner})");
  }

  let chosen = question.chosen.unwrap_or(question.recommended);
  let option = usize::try_from(chosen)
    .ok()
    .and_then(|at| question.options.get(at))
    .map_or("(no such option)", String::as_str);
  let by = if question.settled_by.as_deref() == Some("owner") {
    format!("chosen by {owner}")
  } else {
    "no answer, the recommendation stood".to_owned()
  };

  format!("{}. {option} ({by})", chosen + 1)
}

fn question_lines(queue: &Queue, item: &TodoItem, names: &Names) -> Vec<String> {
  let questions = queue.todo_questions(item.id);

  if questions.is_empty() {
    return Vec::new();
  }

  let mut lines = vec![String::new(), "What a job asked:".to_owned()];

  lines.extend(
    questions
      .iter()
      .map(|question| format!("- {} \u{2192} {}", question.question, worker_settlement(question, names))),
  );

  lines
}

// The words the item's own jobs wrote onto it: its record, which a later run reads.
fn jobs_notes<'a>(queue: &'a Queue, item: &TodoItem) -> Vec<&'a TodoNote> {
  queue
    .todo_notes(item.id)
    .iter()
    .filter(|note| !is_change(note) && item_record(&note.author, note.with_items_job))
    .collect()
}

fn changes<'a>(queue: &'a Queue, item: &TodoItem) -> Vec<&'a TodoNote> {
  queue.todo_notes(item.id).iter().filter(|note| is_change(note)).collect()
}

fn note_lines(queue: &Queue, item: &TodoItem, kept: &Kept) -> Vec<String> {
  let notes = jobs_notes(queue, item);

  if notes.is_empty() {
    Vec::new()
  } else {
    vec![
      String::new(),
      "What jobs wrote onto the item:".to_owned(),
      notes_for_worker(&notes, kept),
    ]
  }
}

fn raised_lines(queue: &Queue, item: &TodoItem, kept: &Kept) -> Vec<String> {
  let raised = queue.todos_raised_from(item.id);

  if raised.is_empty() {
    return Vec::new();
  }

  let mut lines =
    vec!["Rows already raised from this unfinished note (not to be raised again):".to_owned()];

  lines.extend(raised.iter().map(|row| {
    format!(
      "#{} {} ({})",
      row.id,
      line_reading(kept, &row.title),
      if row.done { "done" } else { "open" }
    )
  }));

  lines
}

/// The whole of one item as a worker reads it; the item's own records are read into the queue first.
pub fn todo_show_worker_lines(
  queue: &Queue,
  item: &TodoItem,
  facing: &Facing,
  kept: &Kept,
  names: &Names,
) -> Vec<String> {
  let state = if item.merged() {
    "merged".to_owned()
  } else if item.done {
    "done".to_owned()
  } else if item.held_at.is_some() {
    format!(
      "{} ({})",
      worker_held_label(item.held_kind, facing.stopping.contains(&item.id)),
      line_reading(kept, &item.status)
    )
  } else {
    line_reading(kept, &item.status)
  };
  let mut lines =
    vec![format!("#{} {}", item.id, line_reading(kept, &item.title)), format!("State: {state}")];

  if let Some(held) = item.held_at {
    lines.push(format!(
      "Held: {}{}",
      iso(held),
      item
        .held_reason
        .as_ref()
        .map_or_else(String::new, |reason| format!(" ({})", line_reading(kept, reason))),
    ));
  }

  lines.extend(merge_lines(queue, item, kept));
  lines.extend(repair_lines(queue, item, kept));
  lines.push(format!(
    "Requested by: {}{}",
    item.requester,
    item.requester_id.map_or_else(String::new, |id| format!(" ({id})")),
  ));
  lines.push(format!("Approved: {}", if item.approved { "yes" } else { "no" }));
  lines.push(format!("Priority: {}", item.priority));
  lines.push(format!("Model: {}", item_model(item, facing, true).as_deref().unwrap_or("default")));
  lines.push(format!(
    "Worker: {}{}",
    todo_worker(item, &facing.workers),
    if item.worker.is_none() { " (none assigned, the built-in worker)" } else { "" },
  ));
  lines.push(format!("Co-work: {}", if item.cowork { "on" } else { "off (runs as one session)" }));

  if let Some(grounds) = &item.impact {
    lines.push("Impact grounds:".to_owned());
    lines.extend(grounds.lines());
  }

  lines.push(String::new());

  // A launched item reads as the brief the agent organized; one never launched as its own sections. The
  // conversation, the words people added and the owner's note reach a worker only as the agent folded
  // them into the brief.
  if let Some(brief) = &item.brief {
    lines.extend(brief_lines(brief, &changes(queue, item), kept, names));
  } else {
    lines.push(instruction_reading(kept, &item.instruction));
    lines.extend(order_lines(queue, item, kept));
  }

  lines.extend(note_lines(queue, item, kept));
  lines.extend(question_lines(queue, item, names));

  if let Some(report) = &item.report {
    lines.push(String::new());
    lines.push("Completion report:".to_owned());
    lines.push(worker_reading(kept, report));
  }

  if let Some(unfinished) = &item.unfinished {
    lines.push(String::new());
    lines.push(format!(
      "Unfinished note{}:",
      if item.unfinished_resolved_at.is_none() { "" } else { " (resolved)" }
    ));
    lines.push(worker_reading(kept, unfinished));

    if let Some(resolved) = item.unfinished_resolved_at {
      lines.push(format!(
        "Resolved {}: {}",
        iso(resolved),
        worker_reading(kept, item.unfinished_resolution.as_deref().unwrap_or("")),
      ));
    }

    lines.extend(raised_lines(queue, item, kept));
  }

  lines
}

/// Every text the worker's reading of an item looks up, so its kept English is read beforehand.
pub fn show_texts(queue: &Queue, item: &TodoItem) -> Vec<String> {
  let mut texts = vec![item.title.clone(), item.status.clone()];

  if item.brief.is_none() {
    texts.push(own_sections(&item.instruction));
  }

  texts.extend(item.held_reason.clone());
  texts.extend(item.report.clone());
  texts.extend(item.unfinished.clone());
  texts.extend(item.unfinished_resolution.clone());
  texts.extend(jobs_notes(queue, item).into_iter().map(|note| note.text.clone()));

  if item.brief.is_some() {
    texts.extend(changes(queue, item).into_iter().map(|note| note.text.clone()));
  } else if let Some(order) = queue.work_order(item.id) {
    texts.push(order.intent.clone());
    texts.push(order.instruction.clone());
  }

  for linked in [item.merged_into, item.repairs_todo_id].into_iter().flatten() {
    texts.extend(queue.get_todo(linked).map(|row| row.title.clone()));
  }

  texts.extend(queue.todo_repairs(item.id).into_iter().map(|row| row.title.clone()));
  texts.extend(queue.todos_raised_from(item.id).into_iter().map(|row| row.title));
  texts.extend(queue.todos_merged_into(item.id).into_iter().map(|row| row.title));

  let mut wanted: Vec<String> = Vec::new();

  for text in texts.into_iter().filter(|text| wants_english(text)) {
    if !wanted.contains(&text) {
      wanted.push(text);
    }
  }

  wanted
}
