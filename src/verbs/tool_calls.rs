//! What a job's own thread spent on its tool calls, one call at a time, read off its lines in bmo-agency's
//! agent log (docs/verbs.md, "What a job cost"): `job tool-calls`.
//!
//! The run's spans divide it into phases and say nothing of a single call, so a question like "how much of
//! that job was the suite" is answered here. The counting is per call and added rather than merged: two calls
//! sent side by side count the stretch they shared twice. A call with no result is read as having run until
//! the job ended, or until its last line where it has not.

use std::sync::LazyLock;

use job_agency::{Agency, JobLines};
use regex::Regex;
use serde_json::Value;

use super::stand::{Answer, Stand, VerbError, clock, span as span_clock};
use super::toolbook;
use crate::all;
use crate::queue::units::agency_failed;

/// One call: what it was, when it went out, how long it was out and what it was pointed at.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCall {
  pub name: String,
  /// Whole and fractional seconds alike: most calls are shorter than the second a length is read out in.
  pub seconds: f64,
  /// When it went out, in milliseconds.
  pub at: i64,
  /// Whether the job ended with this call still out.
  pub open: bool,
  /// The first lines of what it was called with, or none where the call names nothing.
  pub said: Vec<String>,
}

/// One tool by its name: how long its calls were out, added, and how many there were.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolTally {
  pub name: String,
  pub seconds: i64,
  pub times: usize,
  pub open: usize,
}

/// Every call of a thread, and what they add up to.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolCallSplit {
  /// Longest first, and earliest first where two are the same length, so two runs read alike.
  pub calls: Vec<ToolCall>,
  pub tools: Vec<ToolTally>,
  pub seconds: i64,
  pub times: usize,
  pub open: usize,
  /// The suite's own calls, counted apart, as seconds and times.
  pub tests: (i64, usize),
}

/// One call as the log holds it: its tool, when it went out and came back, the lines shown of it and the
/// whole of what it was pointed at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
  pub name: String,
  pub from: i64,
  pub to: Option<i64>,
  pub said: Vec<String>,
  pub whole: String,
}

// The fields a call says what it was pointed at in, in the order a reader wants them.
const SAID_FIELDS: [&str; 8] =
  ["command", "file_path", "path", "pattern", "query", "url", "prompt", "description"];
// How much of that goes into the line: the first lines of a script are what tell one call from another.
const SAID_LINES: usize = 3;
const SAID_WIDTH: usize = 120;

// A run of the suite, however it was written on the line.
static TEST_COMMAND: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?:^|[\s;&|(])(?:bun\s+test|cargo\s+test|bmo-testbed\s+suite)(?:\s|$)")
    .expect("a test command pattern")
});

// The tool a call reached and what that tool was handed: a toolbook proxy call as the inner call, in the
// same road's spelling; every other call as it was made.
fn reached(name: &str, input: Option<&Value>) -> (String, Option<Value>) {
  let inner = input.and_then(|input| input.get("tool")).and_then(Value::as_str);
  let arguments = input.and_then(|input| input.get("arguments")).filter(|arguments| arguments.is_object());

  match (inner, arguments) {
    (Some(inner), Some(arguments)) => match toolbook::reached(name, inner) {
      Some(tool) => (tool, Some(arguments.clone())),
      None => (name.to_owned(), input.cloned()),
    },
    _ => (name.to_owned(), input.cloned()),
  }
}

// What the call was pointed at: the whole of it, and the few lines of it that are shown. A blank line carries
// nothing and is passed over.
fn said_of(input: Option<&Value>, command: Option<&str>) -> (Vec<String>, String) {
  let held = input.and_then(Value::as_object);
  let found = held
    .and_then(|held| {
      SAID_FIELDS
        .iter()
        .find_map(|field| held.get(*field).and_then(Value::as_str).filter(|text| !text.trim().is_empty()))
    })
    .or(command.filter(|command| !command.trim().is_empty()));
  let Some(whole) = found else {
    return (vec![], String::new());
  };
  let said = whole
    .split('\n')
    .map(str::trim)
    .filter(|line| !line.is_empty())
    .take(SAID_LINES)
    .map(|line| {
      // Characters are counted as UTF-16 units, as the log's own reader counts them.
      let units: Vec<u16> = line.encode_utf16().collect();

      if units.len() <= SAID_WIDTH {
        line.to_owned()
      } else {
        format!("{}…", String::from_utf16_lossy(&units[..SAID_WIDTH]))
      }
    })
    .collect();

  (said, whole.to_owned())
}

/// Every call in a job's lines, each with the moment its answer came back or none where none did. A result
/// whose call is not among them is left out; a call answered more than once takes the earliest.
pub fn calls_of(lines: &JobLines) -> Vec<Call> {
  lines
    .calls
    .iter()
    .map(|call| {
      let input: Option<Value> = call.args.as_deref().and_then(|args| serde_json::from_str(args).ok());
      let (name, input) = reached(&call.name, input.as_ref());
      let (said, whole) = said_of(input.as_ref(), call.command.as_deref());
      let to = call.reference.as_ref().and_then(|reference| {
        lines
          .results
          .iter()
          .filter(|result| result.reference.as_ref() == Some(reference))
          .map(|result| result.at)
          .min()
      });

      Call {
        name,
        from: call.at,
        to,
        said,
        whole,
      }
    })
    .collect()
}

// JavaScript's `Math.round`: a half goes up, toward the larger number.
fn js_round(value: f64) -> i64 {
  (value + 0.5).floor() as i64
}

/// The calls taken apart, a call still out read as running until `ended`; none where there is no call.
pub fn tool_calls(calls: Vec<Call>, ended: i64) -> Option<ToolCallSplit> {
  if calls.is_empty() {
    return None;
  }

  let measured: Vec<(ToolCall, String)> = calls
    .into_iter()
    .map(|call| {
      (
        ToolCall {
          seconds: (call.to.unwrap_or(ended) - call.from).max(0) as f64 / 1000.0,
          name: call.name,
          at: call.from,
          open: call.to.is_none(),
          said: call.said,
        },
        call.whole,
      )
    })
    .collect();
  let tests: Vec<&ToolCall> = measured
    .iter()
    .filter(|(_, whole)| TEST_COMMAND.is_match(whole))
    .map(|(call, _)| call)
    .collect();
  let tests = (js_round(tests.iter().map(|call| call.seconds).sum()), tests.len());
  let mut calls: Vec<ToolCall> = measured.into_iter().map(|(call, _)| call).collect();

  calls.sort_by(|left, right| right.seconds.total_cmp(&left.seconds).then(left.at.cmp(&right.at)));

  Some(ToolCallSplit {
    tools: by_name(&calls),
    seconds: js_round(calls.iter().map(|call| call.seconds).sum()),
    times: calls.len(),
    open: calls.iter().filter(|call| call.open).count(),
    tests,
    calls,
  })
}

// The calls gathered under the name of the tool that made them, longest first, and by name where two are the
// same length, so two runs read alike.
fn by_name(calls: &[ToolCall]) -> Vec<ToolTally> {
  let mut tools: Vec<(String, f64, usize, usize)> = vec![];

  for call in calls {
    match tools.iter_mut().find(|(name, _, _, _)| *name == call.name) {
      Some(held) => {
        held.1 += call.seconds;
        held.2 += 1;
        held.3 += usize::from(call.open);
      }
      None => tools.push((call.name.clone(), call.seconds, 1, usize::from(call.open))),
    }
  }

  let mut tallies: Vec<ToolTally> = tools
    .into_iter()
    .map(|(name, seconds, times, open)| ToolTally {
      name,
      seconds: js_round(seconds),
      times,
      open,
    })
    .collect();

  tallies.sort_by(|left, right| right.seconds.cmp(&left.seconds).then_with(|| left.name.cmp(&right.name)));
  tallies
}

struct Job {
  id: i64,
  launched_at: i64,
  finished_at: Option<i64>,
}

/// The lines of a split as the verb prints them: its head, the totals and the calls, `top` of them where it
/// is above nought.
pub fn split_lines(split: &ToolCallSplit, top: usize) -> Vec<String> {
  let open = if split.open == 0 { String::new() } else { format!(", {} still out", split.open) };
  let mut lines = vec![
    format!("{} calls, {} out in all{open}", split.times, span_clock(split.seconds as f64)),
    format!(
      "  {}",
      split
        .tools
        .iter()
        .map(|tool| format!("{} {} ×{}", tool.name, span_clock(tool.seconds as f64), tool.times))
        .collect::<Vec<_>>()
        .join(" · ")
    ),
    format!(
      "  {}",
      if split.tests.1 > 0 {
        format!("the suite: {} calls, {}", split.tests.1, span_clock(split.tests.0 as f64))
      } else {
        "the suite: no call ran it".to_owned()
      }
    ),
    String::new(),
    if top > 0 && split.calls.len() > top {
      format!("the {top} longest of {} calls:", split.calls.len())
    } else {
      "every call, longest first:".to_owned()
    },
  ];
  let shown = if top > 0 { &split.calls[..top.min(split.calls.len())] } else { &split.calls[..] };

  for (index, call) in shown.iter().enumerate() {
    let still_out = if call.open { " (still out)" } else { "" };

    lines.push(format!(
      "{:>3}. {} {} at {}{still_out}",
      index + 1,
      call.name,
      span_clock(call.seconds),
      clock(call.at)
    ));

    for line in &call.said {
      lines.push(format!("     {line}"));
    }
  }

  lines
}

/// The reading of the item's last job's calls, as the lines the verb prints.
pub async fn tool_calls_lines(stand: &Stand, todo: i64, top: usize) -> Result<Answer, VerbError> {
  let rows = all(
    &stand.db,
    r#"SELECT "id", "launched_at", "finished_at" FROM "jobs" WHERE "todo_id" = $1 ORDER BY "id""#,
    vec![todo.into()],
  )
  .await?;
  let mut jobs = vec![];

  for row in rows {
    jobs.push(Job {
      id: row.try_get("", "id")?,
      launched_at: row.try_get("", "launched_at")?,
      finished_at: row.try_get("", "finished_at")?,
    });
  }

  // An item tried more than once is read at its last attempt, the earlier ones named.
  let Some(job) = jobs.last() else {
    return Ok(vec![format!("tool-calls: no job ran #{todo}")].into());
  };
  let lines = stand.ground.agency.job_lines(job.id).await.map_err(agency_failed)?;
  let last = lines
    .calls
    .iter()
    .map(|call| call.at)
    .chain(lines.results.iter().map(|result| result.at))
    .max();
  let ended = job.finished_at.or(last).unwrap_or(job.launched_at);
  let earlier = if jobs.len() > 1 {
    format!(", the last of {} runs", jobs.len())
  } else {
    String::new()
  };
  let mut head = vec![
    format!("#{todo} job {}{earlier}", job.id),
    format!(
      "{} ~ {}",
      clock(job.launched_at),
      job.finished_at.map_or_else(|| "still running".to_owned(), clock)
    ),
  ];

  if let Some(finished) = job.finished_at {
    head.push(span_clock((finished - job.launched_at) as f64 / 1000.0));
  }

  let mut said = vec![head.join(" · ")];

  match tool_calls(calls_of(&lines), ended) {
    None => said.push("no tool call in the job's lines".to_owned()),
    Some(split) => said.extend(split_lines(&split, top)),
  }

  Ok(said.into())
}
