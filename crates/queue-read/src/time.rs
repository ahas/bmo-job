//! The time an item took (docs/reading.md, "The time an item took"): the attempts added up, the spans
//! the jobs wrote down as they ended (the phases, the run's own division, what was busy inside it) and
//! what each model spent.

use std::sync::LazyLock;

use job_codes::span_words::{SpanName, TokenKind};
use regex::Regex;
use serde::Serialize;

/// The phase a job's run itself stands under, which the breakdowns of its time divide again.
pub const RUN_PHASE: &str = "agent run";

/// The word a co-work branch's span is named by, ahead of the index it ran at.
pub const BRANCH_WORD: &str = "branch";

// What a span of a model's spending is kept as.
const USAGE_KIND: &str = "usage";

// The tool a run waits on its background branches with.
const BACKGROUND_WAIT_TOOL: &str = "TaskOutput";

// How many of the spans inside the run a reading carries.
const SHOWN_SPANS: usize = 12;

/// Which names are MCP calls: a name read as the server and tool it called, where it is one.
pub type Toolbook<'a> = &'a dyn Fn(&str) -> Option<(String, String)>;

/// One stretch of a job, as the job wrote it down when it ended.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
  pub kind: String,
  pub name: String,
  pub seconds: f64,
  pub times: i64,
  pub tokens: Option<i64>,
  pub input_tokens: Option<i64>,
  pub output_tokens: Option<i64>,
  pub cache_read_tokens: Option<i64>,
  pub cache_write_tokens: Option<i64>,
}

/// One run of an item: its job, when it was launched and ended, and how.
#[derive(Clone, Debug, PartialEq)]
pub struct Run {
  pub id: i64,
  pub launched_at: i64,
  pub finished_at: Option<i64>,
  pub outcome: Option<String>,
}

fn added(held: Option<i64>, more: Option<i64>) -> Option<i64> {
  more.map_or(held, |more| Some(held.unwrap_or(0) + more))
}

/// The spans of several runs added up by what they are, the longest first.
pub fn merge_spans(spans: &[Span]) -> Vec<Span> {
  let mut totals: Vec<Span> = vec![];

  for span in spans {
    match totals.iter_mut().find(|held| held.kind == span.kind && held.name == span.name) {
      None => totals.push(span.clone()),
      Some(held) => {
        held.seconds += span.seconds;
        held.times += span.times;
        held.tokens = added(held.tokens, span.tokens);
        held.input_tokens = added(held.input_tokens, span.input_tokens);
        held.output_tokens = added(held.output_tokens, span.output_tokens);
        held.cache_read_tokens = added(held.cache_read_tokens, span.cache_read_tokens);
        held.cache_write_tokens = added(held.cache_write_tokens, span.cache_write_tokens);
      }
    }
  }

  totals.sort_by(|left, right| right.seconds.total_cmp(&left.seconds));
  totals
}

// Whole seconds, a half going up.
fn rounded(seconds: f64) -> f64 {
  (seconds + 0.5).floor()
}

fn is_branch_name(name: &str) -> bool {
  name.starts_with(&format!("{BRANCH_WORD} "))
}

// A co-worker's span: its agent type in parentheses where one was named, then the words it was handed.
static COWORKER: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"^(?:Task|Agent)(?:\(([^)]*)\))?(?: \u{b7} (.+))?$").expect("a co-worker's name")
});

/// One span as a reading carries it: its name as written and, where it reads as one, what it names.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct SpanReading {
  pub kind: String,
  pub name: String,
  pub called: Option<SpanName>,
  /// The words a co-worker was handed, where its span says them.
  pub said: Option<String>,
  pub seconds: f64,
  pub times: i64,
  pub tokens: Option<i64>,
}

/// A span read for what it names.
pub fn span_reading(span: &Span, toolbook: Toolbook<'_>) -> SpanReading {
  let (called, said) = match span.kind.as_str() {
    "tool" => (
      Some(match toolbook(&span.name) {
        Some((server, tool)) => SpanName::Server { server, tool },
        None => SpanName::Tool {
          name: span.name.clone(),
        },
      }),
      None,
    ),
    "agent" if !is_branch_name(&span.name) => match COWORKER.captures(&span.name) {
      Some(coworker) => (
        Some(SpanName::Coworker {
          agent_type: coworker.get(1).map(|kind| kind.as_str().to_owned()).filter(|kind| !kind.is_empty()),
        }),
        coworker.get(2).map(|said| said.as_str().to_owned()).filter(|said| !said.is_empty()),
      ),
      None => (None, None),
    },
    _ => (None, None),
  };

  SpanReading {
    kind: span.kind.clone(),
    name: span.name.clone(),
    called,
    said,
    seconds: span.seconds,
    times: span.times,
    tokens: span.tokens,
  }
}

fn is_background_wait(span: &Span) -> bool {
  span.kind == "tool" && span.name == BACKGROUND_WAIT_TOOL
}

// What happened inside the run as a reading carries it: the wait on background branches stands only where
// no co-worker span does.
fn shown_inside(inside: Vec<Span>) -> Vec<Span> {
  if inside.iter().any(|span| span.kind == "agent") {
    inside.into_iter().filter(|span| !is_background_wait(span)).collect()
  } else {
    inside
  }
}

/// How the run's own phase divided, in whole seconds: waiting on its branches, running its own tools, and
/// writing.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct RunSplit {
  pub waiting: f64,
  pub tooling: f64,
  pub generating: f64,
}

/// How a run's spans divide its own phase, where they divide it at all.
pub fn run_split(spans: &[Span]) -> Option<RunSplit> {
  let sum = |keep: &dyn Fn(&Span) -> bool| {
    spans.iter().filter(|span| keep(span)).map(|span| span.seconds).sum::<f64>()
  };
  let seconds = sum(&|span| span.kind == "phase" && span.name == RUN_PHASE);

  if seconds <= 0.0 {
    return None;
  }

  let whole = rounded(seconds);
  let waiting = rounded(sum(&is_background_wait));
  let tooling = rounded(sum(&|span| {
    !is_background_wait(span)
      && (span.kind == "tool" || (span.kind == "agent" && !is_branch_name(&span.name)))
  }));
  let generating = whole - waiting - tooling;

  (generating >= 0.0).then_some(RunSplit {
    waiting,
    tooling,
    generating,
  })
}

/// One attempt: its job, how long it took where it finished, and its outcome.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Attempt {
  pub job: i64,
  pub took_ms: Option<i64>,
  pub outcome: Option<String>,
}

/// A count of one kind of token, none where the run wrote none down.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TokenCount {
  pub kind: TokenKind,
  pub tokens: Option<i64>,
}

/// What one model spent across the runs.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Usage {
  pub model: String,
  pub tokens: i64,
  pub kinds: Vec<TokenCount>,
}

/// The time an item's runs took.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TimeReading {
  pub attempts: Vec<Attempt>,
  /// The time the finished attempts took together.
  pub total_ms: i64,
  pub finished: usize,
  pub phases: Vec<SpanReading>,
  pub split: Option<RunSplit>,
  /// The longest of what was busy inside the run.
  pub inside: Vec<SpanReading>,
  /// How many more were busy inside it than are carried.
  pub inside_more: usize,
  /// What each model spent, the most first.
  pub usage: Vec<Usage>,
  /// How many attempts wrote down no spending.
  pub missing: usize,
}

/// How long an item took and what the time went to; none for an item nobody has run.
pub fn time_reading(runs: &[(Run, Vec<Span>)], toolbook: Toolbook<'_>) -> Option<TimeReading> {
  if runs.is_empty() {
    return None;
  }

  let every: Vec<Span> = runs.iter().flat_map(|(_, spans)| spans.iter().cloned()).collect();
  let spans = merge_spans(&every);
  let inside = shown_inside(
    spans
      .iter()
      .filter(|span| span.kind != "phase" && span.kind != USAGE_KIND)
      .cloned()
      .collect(),
  );
  let mut usage: Vec<Usage> = spans
    .iter()
    .filter(|span| span.kind == USAGE_KIND)
    .map(|span| Usage {
      model: span.name.clone(),
      tokens: span.tokens.unwrap_or(0),
      kinds: [
        (TokenKind::Input, span.input_tokens),
        (TokenKind::Output, span.output_tokens),
        (TokenKind::CacheRead, span.cache_read_tokens),
        (TokenKind::CacheWrite, span.cache_write_tokens),
      ]
      .into_iter()
      .map(|(kind, tokens)| TokenCount { kind, tokens })
      .collect(),
    })
    .collect();

  usage.sort_by_key(|spent| std::cmp::Reverse(spent.tokens));

  let spending = runs
    .iter()
    .filter(|(_, spans)| spans.iter().any(|span| span.kind == USAGE_KIND))
    .count();
  let attempts: Vec<Attempt> = runs
    .iter()
    .map(|(run, _)| Attempt {
      job: run.id,
      took_ms: run.finished_at.map(|done| (done - run.launched_at).max(0)),
      outcome: run.outcome.clone(),
    })
    .collect();

  Some(TimeReading {
    total_ms: attempts.iter().filter_map(|attempt| attempt.took_ms).sum(),
    finished: attempts.iter().filter(|attempt| attempt.took_ms.is_some()).count(),
    missing: attempts.len().saturating_sub(spending),
    attempts,
    phases: spans
      .iter()
      .filter(|span| span.kind == "phase")
      .map(|span| span_reading(span, toolbook))
      .collect(),
    split: run_split(&spans),
    inside_more: inside.len().saturating_sub(SHOWN_SPANS),
    inside: inside.iter().take(SHOWN_SPANS).map(|span| span_reading(span, toolbook)).collect(),
    usage,
  })
}
