//! What a job's last reply is judged by as the job ends (docs/runs.md, "The judgement"): a reply that
//! reported nothing, the work a run left running or walked away from, the graph left behind by the job's own
//! commits, a change that reaches the graph its report does not answer for node by node, and the notes a
//! report is handed over with — its shape, a screen it changed and said nothing of sweeping, a check it left
//! undone, a word the owner banned, and what it left uncommitted.

use std::collections::HashSet;
use std::sync::LazyLock;

use job_agency::{LiveTask, RunWorkflow, TurnSpan};
use job_policy::Policy;
use queue_rows::closing::Reader;
use queue_rows::wording::{BannedTerm, banned_terms_in};
use regex::Regex;

use super::{ABANDONED_LABEL, LIVE_LABEL};

// The markers a room's session answers with when it has nothing to say there; a reply made of them is as
// empty as no reply at all.
const SILENCE_MARKERS: [&str; 2] = ["<<SKIP>>", "<<WAIT>>"];

/// Whether a job's answer is no report at all: blank, or nothing but those markers.
pub fn nothing_reported(text: &str) -> bool {
  SILENCE_MARKERS
    .iter()
    .fold(text.to_owned(), |rest, marker| rest.replace(marker, ""))
    .trim()
    .is_empty()
}

/// The report the agent is handed: the whole answer, which opens on its summary.
pub fn report_body(text: &str) -> String {
  text.trim().to_owned()
}

// A length in UTF-16 units, as the report shape counts it.
fn units(text: &str) -> usize {
  text.encode_utf16().count()
}

static PARAGRAPH_BREAK: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\n[ \t]*\n").expect("the paragraph break is a pattern"));

// The fold's own bounds on a conclusion: past them it is folded away with the detail.
const MAX_CLOSING_LENGTH: usize = 200;
const MAX_CLOSING_LINES: usize = 3;

/// What is wrong with the shape of a report, in words the agent reads, or nothing where it is written as it
/// should be: summary, detail and a short conclusion that carries figures.
pub fn report_shape_fault(text: &str) -> Option<String> {
  let paragraphs: Vec<&str> = PARAGRAPH_BREAK
    .split(text.trim())
    .map(str::trim)
    .filter(|paragraph| !paragraph.is_empty())
    .collect();

  if paragraphs.len() < 3 {
    return Some(format!(
      "it has {} paragraph(s) rather than the three of summary, detail and conclusion",
      paragraphs.len()
    ));
  }

  let closing = paragraphs.last().copied().unwrap_or_default();

  if units(closing) > MAX_CLOSING_LENGTH || closing.split('\n').count() > MAX_CLOSING_LINES {
    return Some("its closing line is too long and would be folded away with the detail".to_owned());
  }

  if !closing.chars().any(|c| c.is_ascii_digit()) {
    return Some("its closing line carries no figures".to_owned());
  }

  None
}

/// The screens a job's own commits moved, where its report says nothing of having swept them against the
/// design rules by any of `swept`'s names; nothing otherwise. A screen is a single-file component.
pub fn unswept_screens(paths: &[String], text: &str, swept: &[String]) -> Vec<String> {
  let screens: Vec<String> = paths.iter().filter(|path| path.ends_with(".vue")).cloned().collect();

  if screens.is_empty() {
    return screens;
  }

  let said: String = text.to_lowercase().chars().filter(|c| !c.is_whitespace()).collect();

  if swept.iter().any(|name| said.contains(&name.to_lowercase())) {
    Vec::new()
  } else {
    screens
  }
}

/// A list of paths as a person reads it: the first few by name, and how many more there were.
pub fn change_names(paths: &[String], limit: usize) -> String {
  let shown = paths.iter().take(limit).cloned().collect::<Vec<_>>().join(", ");

  if paths.len() > limit {
    format!("{shown} and {} more", paths.len() - limit)
  } else {
    shown
  }
}

// How much of what a run put down a notice naming it shows.
const ANSWERED_UNITS: usize = 120;

/// What a run put down, for a notice that has to name it: short enough to read in a line, something rather
/// than nothing where it answered blank, and quoted as code, since it may be one of the markers every send
/// takes out of text.
pub fn answered_with(text: &str) -> String {
  let said = text.split_whitespace().collect::<Vec<_>>().join(" ");

  if said.is_empty() {
    return "(nothing)".to_owned();
  }

  let shown = if units(&said) > ANSWERED_UNITS {
    let mut taken = String::new();
    let mut counted = 0;

    for c in said.chars() {
      counted += c.len_utf16();
      if counted > ANSWERED_UNITS {
        break;
      }
      taken.push(c);
    }
    format!("{taken}…")
  } else {
    said
  };
  let mut longest = 0;
  let mut run = 0;

  for c in shown.chars() {
    run = if c == '`' { run + 1 } else { 0 };
    longest = longest.max(run);
  }

  let ticks = "`".repeat(longest + 1);
  let pad = if shown.starts_with('`') || shown.ends_with('`') { " " } else { "" };

  format!("{ticks}{pad}{shown}{pad}{ticks}")
}

// The one word an ending carries when a workflow ran to the end.
const RAN_TO_THE_END: &str = "completed";

/// Which of the workflows a run launched it walked away from: every one whose ending said anything else or
/// nothing at all.
pub fn abandoned_workflows(workflows: &[RunWorkflow]) -> Vec<RunWorkflow> {
  workflows
    .iter()
    .filter(|flow| flow.status.as_deref() != Some(RAN_TO_THE_END))
    .cloned()
    .collect()
}

/// What was still running when the run ended, for the failure the chat is told about.
pub fn live_task_names(tasks: &[LiveTask], limit: usize) -> String {
  let shown: Vec<&str> = tasks
    .iter()
    .take(limit)
    .map(|task| {
      if task.description.trim().is_empty() {
        task.id.as_str()
      } else {
        task.description.trim()
      }
    })
    .collect();
  let more = if tasks.len() > shown.len() {
    format!(" and {} more", tasks.len() - shown.len())
  } else {
    String::new()
  };

  format!("{}{more}", shown.join(", "))
}

fn workflow_names(workflows: &[RunWorkflow], limit: usize) -> String {
  let shown: Vec<&str> = workflows
    .iter()
    .take(limit)
    .map(|flow| if flow.name.trim().is_empty() { flow.call.as_str() } else { flow.name.trim() })
    .collect();
  let more = if workflows.len() > shown.len() {
    format!(" and {} more", workflows.len() - shown.len())
  } else {
    String::new()
  };

  format!("{}{more}", shown.join(", "))
}

/// The lines naming what a run walked away from — which task, and which workflow — and none for a run that
/// left nothing.
pub fn unfinished_lines(live_tasks: &[LiveTask], workflows: &[RunWorkflow]) -> Vec<String> {
  let abandoned = abandoned_workflows(workflows);
  let mut lines = Vec::new();

  if !live_tasks.is_empty() {
    lines.push(format!("{LIVE_LABEL}: {}", live_task_names(live_tasks, 4)));
  }
  if !abandoned.is_empty() {
    lines.push(format!("{ABANDONED_LABEL}: {}", workflow_names(&abandoned, 4)));
  }

  lines
}

/// What the session is asked when its answer left background tasks running, which died with the turn: to
/// settle each of them — a helper is gone, work the report stands on is run again to its end — and to write
/// the whole report again. Its answer is judged by the same rule.
pub fn left_running_task(tasks: &[LiveTask]) -> String {
  [
    format!(
      "Your answer was not sent. When your turn ended these background tasks were still running, and they were killed with the turn: {}.",
      live_task_names(tasks, tasks.len())
    ),
    "Settle each of them now. A helper you raised only to check your work — a dev server, a stand-in, a watcher — is gone and needs nothing more; make sure nothing of it is left running on this machine (a process or a unit you started outside this session), and stop it if it is. Work your report stands on — a test run, a typecheck, a build, a push or a deployment — did not finish: run it again in the foreground, to its end, and read its result.".to_owned(),
    "Start nothing in the background this time. Then write your whole report again, in the same shape and covering the whole job; it is what will be sent in place of the answer above.".to_owned(),
  ]
  .join("\n\n")
}

/// What the session is asked when its answer left its own change open — a change of its own still
/// uncommitted, or a commit to code the graph names with the graph left behind — which is still the
/// session's to close. The checkout and the graph are read again afterwards, never the answer's word.
pub fn left_open_change(tracked: &[String], graph: &[String]) -> String {
  let mut open = vec![
    "Your answer was not sent. Your own change is still open, and a report handed over like that is about work nobody can pick up from where it stands:".to_owned(),
  ];

  if !tracked.is_empty() {
    open.push(format!(
      "- changes of yours still uncommitted in the checkout: {}",
      change_names(tracked, 8)
    ));
  }
  if !graph.is_empty() {
    open.push(format!(
      "- files your own commits touched that the code dependency graph names, with nothing said about the graph in those commits: {}",
      change_names(graph, 8)
    ));
  }

  [
    open.join("\n"),
    "Close it now, in this turn. Commit what is yours and push it; where the graph was left behind, bring it up to date in one pass — a node whose files moved, a symbol that is gone, an edge that is no longer there or one that now is — and put that pass in the same commit as the code, amending yours or adding one beside it. Leave alone anything in the checkout that is not your own work.".to_owned(),
    "Start nothing in the background. Then write your whole report again, in the same shape and covering the whole job; it is what will be sent in place of the answer above.".to_owned(),
  ]
  .join("\n\n")
}

/// What the session is asked when its answer left its item open — neither marked done nor held — which the
/// queue still counts as work waiting for the worker; `bmo` is the platform's command line, and `add_step`
/// the command a work order's final turn adds its unbuilt work with, where the job is one. The item is read
/// again afterwards, never the answer's word.
pub fn left_open_item(todo: i64, bmo: &str, add_step: Option<&str>) -> String {
  let mut asked = vec![
    format!(
      "Your answer was not sent. Your item #{todo} is still open: neither marked done nor held, so the queue still counts it as work waiting for this worker."
    ),
    format!(
      "Close it now, in this turn, as the closing-a-job skill says. Where its work is finished, mark it done with its account: `{bmo} todo --done {todo} --report '<how the work was done>'`, adding `--unfinished '<what it left behind>'` only for strands the skill allows. Where it is not finished, hold it with the reason: `{bmo} todo --stop-here {todo} --why '<reason>' --report '<what is finished and where the next run picks up>'`."
    ),
  ];

  if let Some(add_step) = add_step {
    asked.push(format!(
      "This is your work order's final turn. Where work this item committed to is still to be built, do not hold the item for it: add each piece as a step with {add_step} and end this turn without closing or holding the item; the service runs those steps and a fresh final turn after them. Hold it only for a check that needs the finished change live, once every step is done."
    ));
  }

  asked.push("Start nothing in the background. Then write your whole report again, in the same shape and covering the whole job; it is what will be sent in place of the answer above.".to_owned());
  asked.join("\n\n")
}

/// The item a job left open after it was asked to close it, named under its report.
pub fn open_item_note(todo: Option<i64>) -> String {
  todo.map_or_else(String::new, |todo| {
    format!("\n\nThe job left its item #{todo} open: neither marked done nor held, so the queue still counts it as waiting.")
  })
}

/// Which of the files a job's own commits touched the graph names, where those commits left the graph in
/// `directory` alone; nothing where they touched it, since bringing it up to date is one act.
pub fn unrecorded_change(touched: &[String], named: &HashSet<String>, directory: &str) -> Vec<String> {
  let under = format!("{}/", directory.trim_end_matches('/'));

  if touched.iter().any(|path| path.starts_with(&under)) {
    return Vec::new();
  }

  graph_mapped(touched, named)
}

/// Which of the files a job's own commits touched the graph names, whether or not those commits brought the
/// graph up to date: the change that reaches a node, which its report answers for node by node.
pub fn graph_mapped(touched: &[String], named: &HashSet<String>) -> Vec<String> {
  touched.iter().filter(|path| named.contains(*path)).cloned().collect()
}

/// What a job whose own commits reach the graph left out of answering for that reach: its report's impact
/// part, and any call of the tool that reads it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImpactFaults {
  pub missing_part: bool,
  pub no_call: bool,
}

impl ImpactFaults {
  pub fn any(self) -> bool {
    self.missing_part || self.no_call
  }
}

/// The calls of the impact tool among a record's spans, by whichever road they reached it.
pub fn impact_calls(policy: &Policy, spans: &[TurnSpan]) -> i64 {
  spans
    .iter()
    .filter(|span| span.kind == "tool" && span.name.ends_with(&policy.closing.impact.tool))
    .map(|span| span.times)
    .sum()
}

// The heading a report's impact part stands under, as the closing builds it.
fn impact_heading(policy: &Policy) -> Option<Regex> {
  let impact = &policy.closing.impact;
  let flags = if impact.heading_flags.contains('i') { "(?i)" } else { "" };

  Regex::new(&format!("{flags}{}", impact.heading)).ok()
}

/// Whether a report answers for the graph its job's commits reached: nothing is asked of a change that
/// reaches no file the graph names; one that does names its impact part and was preceded by a call of the
/// impact tool on the item.
pub fn impact_faults(policy: &Policy, mapped: &[String], body: &str, calls: i64) -> ImpactFaults {
  if mapped.is_empty() {
    return ImpactFaults::default();
  }

  ImpactFaults {
    missing_part: !impact_heading(policy).is_some_and(|heading| heading.is_match(body)),
    no_call: calls == 0,
  }
}

// What each fault is, in the words the job and the agent read it by.
fn impact_fault_lines(policy: &Policy, faults: ImpactFaults, mapped: &[String]) -> Vec<String> {
  let tool = &policy.closing.impact.tool;
  let mut lines = vec![];

  if faults.no_call {
    lines.push(format!(
      "- no `{tool}` call is recorded for this item, while your commits changed files the graph names: {}",
      change_names(mapped, 8)
    ));
  }
  if faults.missing_part {
    lines.push(
      "- the report has no Impact part naming each node the change reached and how each was verified"
        .to_owned(),
    );
  }

  lines
}

/// What the session is asked when its report does not answer for the graph its commits reached: the impact
/// tool called on the node it changed, and the report's impact part. The record and the report are read
/// again afterwards.
pub fn left_impact(policy: &Policy, faults: ImpactFaults, mapped: &[String]) -> String {
  let tool = &policy.closing.impact.tool;

  [
    std::iter::once(
      "Your answer was not sent. Your change reaches nodes of the code dependency graph and the report does not answer for them:".to_owned(),
    )
    .chain(impact_fault_lines(policy, faults, mapped))
    .collect::<Vec<_>>()
    .join("\n"),
    format!(
      "Settle it now, in this turn. Call `{tool}` on the node each of those files belongs to, read what it reaches, and check what your change touched there by a test run, a check or a read of the code. Then write the report again with an Impact part in its detail: a line for each node the change reached, with how it was verified, as the closing-a-job skill says."
    ),
    "Start nothing in the background. Write your whole report again, in the same shape and covering the whole job; it is what will be sent in place of the answer above.".to_owned(),
  ]
  .join("\n\n")
}

/// What a report still does not answer for the graph after the job was asked, named under it.
pub fn impact_note(policy: &Policy, faults: ImpactFaults, mapped: &[String]) -> String {
  if !faults.any() {
    return String::new();
  }

  std::iter::once(
    "\n\nThe job's change reaches the code dependency graph and its report does not answer for it:"
      .to_owned(),
  )
  .chain(impact_fault_lines(policy, faults, mapped))
  .collect::<Vec<_>>()
  .join("\n")
}

/// The fault of a report's shape named under it.
pub fn shape_note(body: &str) -> String {
  report_shape_fault(body)
    .map_or_else(String::new, |fault| format!("\n\nThe report is off its shape: {fault}."))
}

/// The lines of a report that leave a check undone, named under it for the agent to settle before telling it.
pub fn unchecked_note(policy: &Policy, reader: &Reader, body: &str) -> String {
  let lines = reader.unchecked_lines(body);

  if lines.is_empty() {
    return String::new();
  }

  std::iter::once(format!("\n\n{}", policy.closing.unchecked.note))
    .chain(lines.iter().enumerate().map(|(at, line)| format!("{}. {line}", at + 1)))
    .collect::<Vec<_>>()
    .join("\n")
}

/// A screen the job changed and said nothing of sweeping, named under its report.
pub fn sweep_note(own_files: &[String], body: &str, swept: &[String]) -> String {
  let unswept = unswept_screens(own_files, body, swept);

  if unswept.is_empty() {
    String::new()
  } else {
    format!(
      "\n\nThe job changed screens and its report says nothing of sweeping them against the design rules: {}.",
      change_names(&unswept, 8)
    )
  }
}

/// A word the owner banned that the report still uses, named under it with what to write instead.
pub fn wording_note(body: &str, glossary: &[BannedTerm]) -> String {
  let terms = banned_terms_in(body, glossary);

  if terms.is_empty() {
    return String::new();
  }

  let named: Vec<String> = terms
    .iter()
    .map(|term| {
      if term.instead.is_empty() {
        term.term.clone()
      } else {
        format!("{} (write {})", term.term, term.instead)
      }
    })
    .collect();

  format!("\n\nThe report still uses words the owner banned: {}.", named.join(", "))
}

/// What the job left uncommitted, named under its report, with what was open before it began named on its own
/// line as somebody else's.
pub fn uncommitted_note(own: &[String], preserved: &[String]) -> String {
  let mut note = String::new();

  if !own.is_empty() {
    note.push_str(&format!("\n\nUncommitted changes are left: {}", change_names(own, 8)));
  }
  if !preserved.is_empty() {
    note.push_str(&format!(
      "\n\nUncommitted changes of earlier work were kept as they stood: {}",
      change_names(preserved, 8)
    ));
  }

  note
}
