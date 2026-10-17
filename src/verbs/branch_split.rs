//! What went on inside a Claude Workflow's branches, read out of the records the workflow kept for them under
//! the job's own configuration directory (docs/verbs.md, "What a job cost"): `job branches`.
//!
//! A branch is a run of its own and the run's log cannot time anything inside it: its lines arrive in bursts
//! once the workflow is over. The records can, every line of them carrying the moment it was written, so this
//! is the one way to see whether a slow branch was waiting on the shell or writing. Those records belong to one
//! session and the program sweeps them, so the figures are read while they are there.
//!
//! A branch's time divides in two. One term is the stretch during which some call the branch made was
//! outstanding — each call paired with the result that answered it, and the stretches merged, so two calls
//! sent side by side count the time they shared once. The other is everything left of the branch's own
//! length, which is the model reading, thinking and writing. A call still out when the branch's record ends
//! is read as having run until it ended, which is the most that record supports.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use job_base::Refusal;
use job_codes::{BranchSplitRefusal, JobError};
use serde::Deserialize;
use serde_json::Value;

use super::stand::span;
use super::toolbook;

/// What a call names where it names no tool.
const UNNAMED_TOOL: &str = "(unnamed)";

/// One tool by its name: how long its calls ran, added, and how many there were. Two calls of the same tool
/// sent side by side count their shared stretch twice: this answers how long the tool was busy, not how much
/// of the branch it held.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchTool {
  pub name: String,
  pub seconds: i64,
  pub times: i64,
  /// Calls of this tool still out when the branch ended.
  pub open: i64,
}

/// A branch's length and the two terms it divides into, in whole seconds, the second carrying the rounding as
/// the remainder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BranchSplit {
  pub seconds: i64,
  pub tooling: i64,
  pub generating: i64,
  pub calls: i64,
  pub open: i64,
  pub tools: Vec<BranchTool>,
}

struct Call {
  name: String,
  from: i64,
  to: Option<i64>,
}

// When an entry was written, in milliseconds; none for a line the program did not stamp, which is
// scaffolding rather than something that happened.
fn moment_of(entry: &Value) -> Option<i64> {
  let stamp = entry.get("timestamp")?.as_str()?;

  chrono::DateTime::parse_from_rfc3339(stamp).ok().map(|at| at.timestamp_millis())
}

fn blocks_of(entry: &Value) -> impl Iterator<Item = &Value> {
  entry
    .pointer("/message/content")
    .and_then(Value::as_array)
    .into_iter()
    .flatten()
    .filter(|block| block.is_object())
}

// The tool a call reached: a toolbook proxy call as the inner call, in the same road's spelling; every other
// call as it was made.
fn reached_tool(name: &str, input: Option<&Value>) -> String {
  let inner = input.and_then(|input| input.get("tool")).and_then(Value::as_str);
  let handed = input.and_then(|input| input.get("arguments")).is_some_and(Value::is_object);

  inner
    .filter(|_| handed)
    .and_then(|inner| toolbook::reached(name, inner))
    .unwrap_or_else(|| name.to_owned())
}

// Every call the branch made, each with the moment its answer came back or none where none did. A result whose
// call is nowhere in the record is left out; a call answered more than once takes the earliest answer, which
// is the one that ended the wait.
fn calls_of(entries: &[Value]) -> Vec<Call> {
  let mut order: Vec<String> = Vec::new();
  let mut calls: HashMap<String, Call> = HashMap::new();

  for entry in entries {
    let Some(at) = moment_of(entry) else { continue };

    for block in blocks_of(entry) {
      let Some(id) = block.get("id").and_then(Value::as_str) else { continue };

      if block.get("type").and_then(Value::as_str) != Some("tool_use") || calls.contains_key(id) {
        continue;
      }

      // A call made through the toolbook is timed under the tool it reached.
      let name = match block.get("name").and_then(Value::as_str) {
        Some(name) => reached_tool(name, block.get("input")),
        None => UNNAMED_TOOL.to_owned(),
      };

      order.push(id.to_owned());
      calls.insert(
        id.to_owned(),
        Call {
          name,
          from: at,
          to: None,
        },
      );
    }
  }

  for entry in entries {
    let Some(at) = moment_of(entry) else { continue };

    for block in blocks_of(entry) {
      if block.get("type").and_then(Value::as_str) != Some("tool_result") {
        continue;
      }

      let Some(call) = block.get("tool_use_id").and_then(Value::as_str).and_then(|id| calls.get_mut(id))
      else {
        continue;
      };

      call.to = Some(call.to.map_or(at, |to| to.min(at)));
    }
  }

  order.into_iter().filter_map(|id| calls.remove(&id)).collect()
}

// The stretch during which at least one span was out: the spans in order, run together where they touch.
fn merged(mut spans: Vec<(i64, i64)>) -> i64 {
  spans.sort_by_key(|span| span.0);

  let mut total = 0;
  let mut current: Option<(i64, i64)> = None;

  for (from, to) in spans {
    current = match current {
      Some((open, end)) if from <= end => Some((open, end.max(to))),
      Some((open, end)) => {
        total += end - open;
        Some((from, to))
      }
      None => Some((from, to)),
    };
  }

  total + current.map_or(0, |(from, to)| to - from)
}

// Seconds out of milliseconds, whole, halves going up as a reader rounds them.
fn whole_seconds(milliseconds: f64) -> i64 {
  (milliseconds / 1000.0).round() as i64
}

// The calls gathered under the name of the tool that made them, longest first, and by name where two are the
// same length, so a reading comes out the same way twice.
fn by_name(calls: &[Call], ended: i64) -> Vec<BranchTool> {
  let mut order: Vec<&str> = Vec::new();
  let mut held: HashMap<&str, (f64, i64, i64)> = HashMap::new();

  for call in calls {
    let entry = held.entry(&call.name).or_insert_with(|| {
      order.push(&call.name);
      (0.0, 0, 0)
    });

    entry.0 += (call.to.unwrap_or(ended) - call.from).max(0) as f64;
    entry.1 += 1;
    entry.2 += i64::from(call.to.is_none());
  }

  let mut tools: Vec<BranchTool> = order
    .into_iter()
    .map(|name| {
      let (milliseconds, times, open) = held[name];

      BranchTool {
        name: name.to_owned(),
        seconds: whole_seconds(milliseconds),
        times,
        open,
      }
    })
    .collect();

  tools.sort_by(|left, right| {
    right
      .seconds
      .cmp(&left.seconds)
      .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
      .then_with(|| left.name.cmp(&right.name))
  });

  tools
}

/// The division of one branch's record, or none where it has no length to divide: a record with no moment in
/// it, or whose moments are all the same, is a record that was never written rather than a branch whose time
/// went nowhere.
pub fn branch_split(entries: &[Value]) -> Option<BranchSplit> {
  let moments: Vec<i64> = entries.iter().filter_map(moment_of).collect();
  let opened = *moments.iter().min()?;
  let ended = *moments.iter().max()?;
  let length = ended - opened;

  if length <= 0 {
    return None;
  }

  let calls = calls_of(entries);
  let spans = calls.iter().map(|call| (call.from, call.from.max(call.to.unwrap_or(ended)))).collect();
  let seconds = whole_seconds(length as f64);
  let tooling = whole_seconds(merged(spans) as f64);

  Some(BranchSplit {
    seconds,
    tooling,
    generating: seconds - tooling,
    calls: calls.len() as i64,
    open: calls.iter().filter(|call| call.to.is_none()).count() as i64,
    tools: by_name(&calls, ended),
  })
}

/// A count with its thousands set apart.
pub fn grouped(number: i64) -> String {
  let digits = number.unsigned_abs().to_string();
  let mut out = String::new();

  for (at, digit) in digits.chars().enumerate() {
    if at > 0 && (digits.len() - at).is_multiple_of(3) {
      out.push(',');
    }

    out.push(digit);
  }

  if number < 0 { format!("-{out}") } else { out }
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowBranch {
  index: Option<i64>,
  label: Option<String>,
  agent_id: Option<String>,
  model: Option<String>,
  tokens: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkflowRecord {
  workflow_name: Option<String>,
  timestamp: Option<String>,
  status: Option<String>,
  workflow_progress: Option<Vec<WorkflowBranch>>,
}

struct FoundWorkflow {
  id: String,
  session: String,
  record: WorkflowRecord,
}

impl FoundWorkflow {
  fn branches(&self) -> Vec<&WorkflowBranch> {
    self
      .record
      .workflow_progress
      .iter()
      .flatten()
      .filter(|branch| branch.agent_id.is_some())
      .collect()
  }
}

fn sorted_names(dir: &Path) -> Vec<String> {
  let mut names: Vec<String> = std::fs::read_dir(dir)
    .into_iter()
    .flatten()
    .flatten()
    .map(|entry| entry.file_name().to_string_lossy().into_owned())
    .collect();

  names.sort();
  names
}

// Every workflow the program still holds a record of under a project folder, newest first. A record still
// being written, or shaped by another version, names nothing this can use and is passed over.
fn found_workflows(project: &Path) -> Vec<FoundWorkflow> {
  let mut found = Vec::new();

  for session in sorted_names(project) {
    let dir = project.join(&session).join("workflows");

    if !dir.is_dir() {
      continue;
    }

    for name in sorted_names(&dir) {
      let Some(id) = name.strip_suffix(".json").filter(|_| name.starts_with("wf_")) else {
        continue;
      };
      let Ok(text) = std::fs::read_to_string(dir.join(&name)) else { continue };
      let Ok(record) = serde_json::from_str::<WorkflowRecord>(&text) else { continue };

      found.push(FoundWorkflow {
        id: id.to_owned(),
        session: session.clone(),
        record,
      });
    }
  }

  found.sort_by(|left, right| {
    let stamp = |found: &FoundWorkflow| found.record.timestamp.clone().unwrap_or_default();

    stamp(right).cmp(&stamp(left))
  });
  found
}

// Each branch's own record under a workflow's folder, by the branch's id: its JSON lines, read where they read.
fn branch_transcripts(dir: &Path) -> HashMap<String, Vec<Value>> {
  sorted_names(dir)
    .into_iter()
    .filter_map(|name| {
      let id = name.strip_prefix("agent-")?.strip_suffix(".jsonl")?.to_owned();
      let text = std::fs::read_to_string(dir.join(&name)).ok()?;
      let entries = text
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('{'))
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

      Some((id, entries))
    })
    .collect()
}

// The type the branch was spawned as, written down beside its record.
fn agent_type(dir: &Path, agent_id: &str) -> String {
  std::fs::read_to_string(dir.join(format!("agent-{agent_id}.meta.json")))
    .ok()
    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    .and_then(|meta| meta.get("agentType").and_then(Value::as_str).map(str::to_owned))
    .unwrap_or_else(|| "(no type)".to_owned())
}

fn share(part: i64, whole: i64) -> String {
  if whole <= 0 {
    "0%".to_owned()
  } else {
    format!("{}%", (part as f64 / whole as f64 * 100.0).round() as i64)
  }
}

/// A split as the lines a person reads.
pub fn split_lines(split: &BranchSplit) -> Vec<String> {
  let open = if split.open == 0 { String::new() } else { format!(", {} still out", split.open) };
  let mut lines = vec![format!(
    "  {} in all · tools {} ({}, {} calls{open}) · the model {} ({})",
    span(split.seconds as f64),
    span(split.tooling as f64),
    share(split.tooling, split.seconds),
    split.calls,
    span(split.generating as f64),
    share(split.generating, split.seconds)
  )];

  if !split.tools.is_empty() {
    let tools: Vec<String> = split
      .tools
      .iter()
      .map(|tool| format!("{} {} ×{}", tool.name, span(tool.seconds as f64), tool.times))
      .collect();

    lines.push(format!("  {}", tools.join(" · ")));
  }

  lines
}

fn joined(parts: &[String]) -> String {
  parts.iter().filter(|part| !part.is_empty()).cloned().collect::<Vec<_>>().join(" · ")
}

// One workflow read out: its own line, then a branch at a time — what it was called, on what model, as what
// type, and how its time divided.
fn workflow_lines(project: &Path, found: &FoundWorkflow) -> Vec<String> {
  let record = &found.record;
  let mut branches = found.branches();
  let head = joined(&[
    found.id.clone(),
    record.workflow_name.clone().unwrap_or_default(),
    record.timestamp.clone().unwrap_or_default(),
    record.status.clone().unwrap_or_default(),
    format!("{} branches", branches.len()),
  ]);
  let dir = project.join(&found.session).join("subagents").join("workflows").join(&found.id);
  let transcripts = branch_transcripts(&dir);

  if transcripts.is_empty() {
    return vec![head, "  the branches' records are gone".to_owned()];
  }

  let mut lines = vec![head];

  branches.sort_by_key(|branch| branch.index.unwrap_or(0));

  for branch in branches {
    let agent_id = branch.agent_id.clone().unwrap_or_default();
    let index = branch.index.map_or_else(|| "?".to_owned(), |index| index.to_string());

    lines.push(joined(&[
      format!("branch {index}"),
      branch.label.clone().unwrap_or_else(|| agent_id.clone()),
      branch.model.clone().unwrap_or_default(),
      agent_type(&dir, &agent_id),
      branch.tokens.map_or_else(String::new, |tokens| format!("{} tokens", grouped(tokens))),
    ]));

    match transcripts.get(&agent_id).and_then(|entries| branch_split(entries)) {
      Some(split) => lines.extend(split_lines(&split)),
      None => lines.push("  its record holds no length to divide".to_owned()),
    }
  }

  lines
}

/// The program's project folder for a working directory: every character but a letter or a digit a hyphen,
/// under the projects folder of the configuration directory named, or the home's.
pub fn project_dir(config_dir: Option<&Path>, home: &Path, cwd: &Path) -> PathBuf {
  let config_dir = config_dir.map_or_else(|| home.join(".claude"), Path::to_path_buf);
  let project: String = cwd
    .to_string_lossy()
    .chars()
    .map(|one| if one.is_ascii_alphanumeric() { one } else { '-' })
    .collect();

  config_dir.join("projects").join(project)
}

fn refused(refusal: BranchSplitRefusal) -> Refusal {
  JobError::from(refusal).into()
}

/// What `job branches` prints for a project folder: every workflow still on disk with `list`, or one
/// workflow's branches — the one named, or the newest.
pub fn branch_split_lines(
  project: &Path,
  workflow: Option<&str>,
  list: bool,
) -> Result<Vec<String>, Refusal> {
  let shown = project.to_string_lossy().into_owned();
  let workflows = found_workflows(project);

  if workflows.is_empty() {
    return Err(refused(BranchSplitRefusal::NoWorkflowRecords { project: shown }));
  }

  if list {
    return Ok(
      workflows
        .iter()
        .map(|found| {
          [
            found.id.clone(),
            found.record.workflow_name.clone().unwrap_or_default(),
            found.record.timestamp.clone().unwrap_or_default(),
            format!("{} branches", found.branches().len()),
            format!("session {}", found.session),
          ]
          .join(" · ")
        })
        .collect(),
    );
  }

  let chosen: Vec<&FoundWorkflow> = match workflow.filter(|wanted| !wanted.is_empty()) {
    None => workflows.iter().take(1).collect(),
    Some(wanted) => workflows.iter().filter(|found| found.id == wanted).collect(),
  };

  if chosen.is_empty() {
    return Err(refused(BranchSplitRefusal::WorkflowNotFound {
      workflow: workflow.unwrap_or_default().to_owned(),
      project: shown,
    }));
  }

  Ok(chosen.into_iter().flat_map(|found| workflow_lines(project, found)).collect())
}
