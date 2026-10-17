//! What a job cost, a job at a time (docs/verbs.md, "What a job cost"): `job figures <item…>`. The time and
//! the tokens are the spans a job wrote as it ended; the model's requests, its compactions, its longest results
//! and its shell calls by what they ran are its lines in bmo-agency's agent log, a call paired with its answer
//! by the agent, run and reference they share.

use job_agency::{Agency, JobLines, ToolCall, ToolResult};
use queue_read::time::{Run, Span, run_split};
use serde::Serialize;

use super::branch_split::grouped;
use super::job_guard::shell::{SHELLS, program_of, shell_script, split, unquoted};
use super::stand::{Stand, VerbError, span};
use crate::Refusal;
use crate::all;
use crate::queue::units::agency_failed;

/// What a shell call ran, as its command reads, heaviest first: a line that runs several is the heaviest of
/// them, since that is what its time went to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ShellClass {
  Check,
  BunTest,
  Cargo,
  TypeCheck,
  QuickCheck,
  TestLog,
  Git,
  Reading,
  Other,
}

impl ShellClass {
  pub fn name(self) -> &'static str {
    match self {
      Self::TypeCheck => "type check",
      Self::QuickCheck => "quick-check",
      Self::BunTest => "bun test",
      Self::Cargo => "cargo",
      Self::TestLog => "test-log",
      Self::Check => "check",
      Self::Git => "git",
      Self::Reading => "reading",
      Self::Other => "other",
    }
  }
}

// What a program and the words after it that are not flags run, the first row that matches deciding: the
// words given are matched in order from the first, that one as its last path part, and no word given matches
// whatever follows. A script of a repository and the command of the platform's command line it answers for
// are read alike.
const CLASSES: &[(&str, &[&str], ShellClass)] = &[
  ("tsc", &[], ShellClass::TypeCheck),
  ("vue-tsc", &[], ShellClass::TypeCheck),
  ("bunx", &["tsc"], ShellClass::TypeCheck),
  ("bunx", &["vue-tsc"], ShellClass::TypeCheck),
  ("npx", &["tsc"], ShellClass::TypeCheck),
  ("bun", &["quick-check.ts"], ShellClass::QuickCheck),
  ("bun", &["test-log.ts"], ShellClass::TestLog),
  ("bun", &["check.ts"], ShellClass::Check),
  ("bun", &["cargo.ts"], ShellClass::Cargo),
  ("bmo", &["check", "quick"], ShellClass::QuickCheck),
  ("bmo", &["test", "log"], ShellClass::TestLog),
  ("bmo", &["check"], ShellClass::Check),
  ("bmo", &["cargo"], ShellClass::Cargo),
  ("bun", &["test"], ShellClass::BunTest),
  ("cargo", &[], ShellClass::Cargo),
  ("git", &[], ShellClass::Git),
  ("cat", &[], ShellClass::Reading),
  ("head", &[], ShellClass::Reading),
  ("tail", &[], ShellClass::Reading),
  ("sed", &[], ShellClass::Reading),
  ("rg", &[], ShellClass::Reading),
  ("grep", &[], ShellClass::Reading),
  ("ls", &[], ShellClass::Reading),
  ("find", &[], ShellClass::Reading),
  ("wc", &[], ShellClass::Reading),
  ("jq", &[], ShellClass::Reading),
  ("nl", &[], ShellClass::Reading),
  ("awk", &[], ShellClass::Reading),
  ("tree", &[], ShellClass::Reading),
];

// What a line does before the command it is for, which decides nothing of what it ran.
const SETUP: [&str; 6] = ["cd", "export", "set", "pwd", "true", ":"];

// The tools a shell command is run through, on every road.
const SHELL_TOOLS: [&str; 4] = ["Bash", "bash", "shell", "local_shell"];

/// Whether a call was a shell command.
pub fn is_shell_call(name: &str) -> bool {
  SHELL_TOOLS.contains(&name)
}

// The class of one simple command, or none for one that only sets the line up.
fn class_of(words: &[String]) -> Option<ShellClass> {
  let run = program_of(words)?;
  let program = run.program.as_str();

  if SETUP.contains(&program) {
    return None;
  }

  if SHELLS.contains(&program)
    && let Some(script) = shell_script(&run)
  {
    return Some(shell_class(&script));
  }

  let args: Vec<String> = run.args.iter().map(|arg| unquoted(arg)).collect();

  // A command run in the machine's queue is the command inside it.
  if program == "bmo"
    && args.first().map(String::as_str) == Some("testbed")
    && let Some(at) = args.iter().position(|arg| arg == "--")
  {
    return Some(class_of(&run.args[at + 1..]).unwrap_or(ShellClass::Other));
  }

  let said: Vec<&str> = args
    .iter()
    .filter(|arg| !arg.starts_with('-'))
    .enumerate()
    .map(|(at, arg)| if at == 0 { arg.rsplit('/').next().unwrap_or(arg) } else { arg.as_str() })
    .collect();

  Some(
    CLASSES
      .iter()
      .find(|(named, words, _)| *named == program && said.starts_with(words))
      .map_or(ShellClass::Other, |(_, _, class)| *class),
  )
}

/// What a shell call ran: the heaviest of its simple commands that does more than set the line up, a command
/// run in the queue read as the command inside it; the rest where none is.
pub fn shell_class(command: &str) -> ShellClass {
  split(command)
    .commands
    .iter()
    .filter_map(|one| class_of(&one.words))
    .min()
    .unwrap_or(ShellClass::Other)
}

/// One class's shell calls.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ClassRow {
  pub class: ShellClass,
  pub calls: i64,
  /// From each call to its answer, summed; a call with no answer counts nothing.
  pub seconds: i64,
}

/// One of the longest results kept whole.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LongResult {
  pub tool: String,
  pub chars: i64,
  pub said: String,
}

/// What one job cost.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct JobFigures {
  pub job: i64,
  pub outcome: Option<String>,
  pub wall_seconds: Option<i64>,
  /// The share of the run left to the model, where the spans divide the run.
  pub model_seconds: Option<f64>,
  pub shell_calls: i64,
  pub shell_seconds: f64,
  pub requests: i64,
  pub input_tokens: i64,
  pub output_tokens: i64,
  pub cache_read_tokens: i64,
  pub cache_write_tokens: i64,
  pub graph_search: i64,
  pub graph_get: i64,
  pub graph_impact: i64,
  pub compactions: i64,
  pub longest: Vec<LongResult>,
  pub classes: Vec<ClassRow>,
}

// How many of the longest results a job's figures name.
const LONGEST: usize = 5;
// How much of a call names it.
const SAID_WIDTH: usize = 90;

fn tool_calls(spans: &[Span], tool: &str) -> i64 {
  spans
    .iter()
    .filter(|span| span.kind == "tool" && (span.name == tool || span.name.ends_with(&format!("__{tool}"))))
    .map(|span| span.times)
    .sum()
}

/// What a call names it by, on one line and cut to `width`.
pub fn call_said(call: &ToolCall, width: usize) -> String {
  let said = call.command.as_deref().unwrap_or(&call.name);
  let one_line = said.replace('\n', " ⏎ ");

  if one_line.chars().count() > width {
    format!("{}…", one_line.chars().take(width - 1).collect::<String>())
  } else {
    one_line
  }
}

// The result that answered a call: the one of the same agent, run and reference.
fn answer_of<'a>(call: &ToolCall, results: &'a [ToolResult]) -> Option<&'a ToolResult> {
  call.reference.as_ref()?;
  results.iter().find(|result| {
    result.agent == call.agent && result.run == call.run && result.reference == call.reference
  })
}

/// One job's figures, out of its run, its spans and its lines.
pub fn job_figures(run: &Run, spans: &[Span], lines: &JobLines) -> JobFigures {
  let usage = |field: fn(&Span) -> Option<i64>| -> i64 {
    spans.iter().filter(|span| span.kind == "usage").filter_map(field).sum()
  };
  let shell: Vec<&Span> =
    spans.iter().filter(|span| span.kind == "tool" && is_shell_call(&span.name)).collect();
  let mut classes: Vec<ClassRow> = vec![];
  let mut kept: Vec<(&ToolCall, i64)> = vec![];

  for call in &lines.calls {
    let answer = answer_of(call, &lines.results);

    if let Some(chars) = answer.filter(|answer| answer.complete).and_then(|answer| answer.length) {
      kept.push((call, i64::try_from(chars).unwrap_or(i64::MAX)));
    }

    if !is_shell_call(&call.name) {
      continue;
    }

    let class = shell_class(call.command.as_deref().unwrap_or_default());
    let seconds = answer.map_or(0, |answer| ((answer.at - call.at) / 1000).max(0));

    match classes.iter_mut().find(|row| row.class == class) {
      Some(row) => {
        row.calls += 1;
        row.seconds += seconds;
      }
      None => classes.push(ClassRow {
        class,
        calls: 1,
        seconds,
      }),
    }
  }

  classes.sort_by(|a, b| b.seconds.cmp(&a.seconds).then(b.calls.cmp(&a.calls)));
  kept.sort_by_key(|(_, chars)| std::cmp::Reverse(*chars));

  JobFigures {
    job: run.id,
    outcome: run.outcome.clone(),
    wall_seconds: run.finished_at.map(|finished| (finished - run.launched_at) / 1000),
    model_seconds: run_split(spans).map(|split| split.generating),
    shell_calls: shell.iter().map(|span| span.times).sum(),
    shell_seconds: shell.iter().map(|span| span.seconds).sum(),
    requests: i64::try_from(lines.requests).unwrap_or(i64::MAX),
    input_tokens: usage(|span| span.input_tokens),
    output_tokens: usage(|span| span.output_tokens),
    cache_read_tokens: usage(|span| span.cache_read_tokens),
    cache_write_tokens: usage(|span| span.cache_write_tokens),
    graph_search: tool_calls(spans, "graph_search"),
    graph_get: tool_calls(spans, "graph_get"),
    graph_impact: tool_calls(spans, "graph_impact"),
    compactions: i64::try_from(lines.compactions).unwrap_or(i64::MAX),
    longest: kept
      .into_iter()
      .take(LONGEST)
      .map(|(call, chars)| LongResult {
        tool: call.name.clone(),
        chars,
        said: call_said(call, SAID_WIDTH),
      })
      .collect(),
    classes,
  }
}

/// An item's jobs, oldest first, each with its figures.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ItemFigures {
  pub todo: i64,
  pub jobs: Vec<JobFigures>,
}

// The runs of an item, oldest first, each with the spans it wrote down.
async fn item_runs(stand: &Stand, todo: i64) -> Result<Vec<(Run, Vec<Span>)>, Refusal> {
  let db = &stand.db;
  let jobs = all(
    db,
    r#"SELECT "id", "launched_at", "finished_at", "outcome" FROM "jobs" WHERE "todo_id" = $1 ORDER BY "id""#,
    vec![todo.into()],
  )
  .await?;
  let mut runs = vec![];

  for job in jobs {
    let run = Run {
      id: job.try_get("", "id")?,
      launched_at: job.try_get("", "launched_at")?,
      finished_at: job.try_get("", "finished_at")?,
      outcome: job.try_get("", "outcome")?,
    };
    let rows = all(
      db,
      r#"SELECT "kind", "name", "seconds", "times", "tokens", "input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens"
         FROM "job_spans" WHERE "job_id" = $1 ORDER BY "seconds" DESC, "id""#,
      vec![run.id.into()],
    )
    .await?;
    let spans = rows
      .iter()
      .map(|row| {
        Ok(Span {
          kind: row.try_get("", "kind")?,
          name: row.try_get("", "name")?,
          seconds: row.try_get("", "seconds")?,
          times: row.try_get("", "times")?,
          tokens: row.try_get("", "tokens")?,
          input_tokens: row.try_get("", "input_tokens")?,
          output_tokens: row.try_get("", "output_tokens")?,
          cache_read_tokens: row.try_get("", "cache_read_tokens")?,
          cache_write_tokens: row.try_get("", "cache_write_tokens")?,
        })
      })
      .collect::<Result<Vec<_>, sea_orm::DbErr>>()?;

    runs.push((run, spans));
  }

  Ok(runs)
}

/// The figures of every job of each item named.
pub async fn read(stand: &Stand, items: &[i64]) -> Result<Vec<ItemFigures>, VerbError> {
  let mut read = vec![];

  for todo in items {
    let mut jobs = vec![];

    for (run, spans) in item_runs(stand, *todo).await? {
      let lines = stand.ground.agency.job_lines(run.id).await.map_err(agency_failed)?;

      jobs.push(job_figures(&run, &spans, &lines));
    }

    read.push(ItemFigures { todo: *todo, jobs });
  }

  Ok(read)
}

fn count(value: i64) -> String {
  grouped(value.max(0))
}

/// The figures as lines a person reads, an item at a time and a job under it.
pub fn figures_lines(read: &[ItemFigures]) -> Vec<String> {
  let mut lines = vec![];

  for item in read {
    if item.jobs.is_empty() {
      lines.push(format!("#{}: no job ran it", item.todo));
      continue;
    }

    for job in &item.jobs {
      let model = job.model_seconds.map_or_else(|| "not divided".to_owned(), span);
      let wall = job.wall_seconds.map_or_else(|| "still running".to_owned(), |wall| span(wall as f64));

      lines.push(format!("#{} job {} ({})", item.todo, job.job, job.outcome.as_deref().unwrap_or("open")));
      lines.push(format!(
        "  wall {wall} · model {model} · shell {} calls, {} · requests {}",
        count(job.shell_calls),
        span(job.shell_seconds),
        count(job.requests)
      ));
      lines.push(format!(
        "  tokens in {} · out {} · cache read {} · cache write {}",
        count(job.input_tokens),
        count(job.output_tokens),
        count(job.cache_read_tokens),
        count(job.cache_write_tokens)
      ));
      lines.push(format!(
        "  graph_search {} · graph_get {} · graph_impact {} · compactions recorded {}",
        job.graph_search, job.graph_get, job.graph_impact, job.compactions
      ));

      if !job.classes.is_empty() {
        let classes: Vec<String> = job
          .classes
          .iter()
          .map(|row| format!("{} {} · {}", row.class.name(), row.calls, span(row.seconds as f64)))
          .collect();

        lines.push(format!("  shell by class: {}", classes.join("; ")));
      }

      for (at, long) in job.longest.iter().enumerate() {
        lines.push(format!("  {}. {} chars · {} · {}", at + 1, count(long.chars), long.tool, long.said));
      }
    }
  }

  lines
}

/// The usage `--help` prints.
pub const USAGE: &str = "usage: job figures <item…> [--json]

For every job of each item: its wall time, the model's share of its run, its shell calls and time, its model
requests, its tokens, its graph tool calls, the compactions its record wrote, its longest results kept whole,
and its shell calls by what they ran, each with its count and time.";
