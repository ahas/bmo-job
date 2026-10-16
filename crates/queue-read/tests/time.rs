//! The time an item took (docs/reading.md, "The time an item took").

use job_codes::span_words::{SpanName, TokenKind};
use queue_read::time::{
  RUN_PHASE, Run, RunSplit, Span, merge_spans, run_split, span_reading, time_reading,
};

fn span(kind: &str, name: &str, seconds: f64) -> Span {
  Span {
    kind: kind.to_owned(),
    name: name.to_owned(),
    seconds,
    times: 1,
    tokens: None,
    input_tokens: None,
    output_tokens: None,
    cache_read_tokens: None,
    cache_write_tokens: None,
  }
}

fn usage(model: &str, tokens: i64, input: i64) -> Span {
  Span {
    tokens: Some(tokens),
    input_tokens: Some(input),
    ..span("usage", model, 0.0)
  }
}

fn run(id: i64, took: Option<i64>) -> Run {
  Run {
    id,
    launched_at: 1_000,
    finished_at: took.map(|took| 1_000 + took),
    outcome: took.map(|_| "done".to_owned()),
  }
}

fn no_server(_: &str) -> Option<(String, String)> {
  None
}

fn toolbook(name: &str) -> Option<(String, String)> {
  name
    .strip_prefix("mcp__")
    .and_then(|rest| rest.split_once("__"))
    .map(|(server, tool)| (server.to_owned(), tool.to_owned()))
}

#[test]
fn spans_of_several_runs_add_up_by_what_they_are_the_longest_first() {
  let merged = merge_spans(&[
    span("tool", "Bash", 3.0),
    span("tool", "Read", 5.0),
    usage("model-a", 10, 4),
    span("tool", "Bash", 4.0),
    usage("model-a", 5, 1),
  ]);

  assert_eq!(merged[0].name, "Bash");
  assert_eq!((merged[0].seconds, merged[0].times), (7.0, 2));
  assert_eq!(merged[1].name, "Read");
  assert_eq!(merged[2].tokens, Some(15));
  assert_eq!(merged[2].input_tokens, Some(5));
  assert_eq!(merged[2].output_tokens, None);
}

#[test]
fn a_tool_span_is_named_by_its_server_where_the_toolbook_knows_it() {
  assert_eq!(
    span_reading(&span("tool", "mcp__web__web_fetch", 1.0), &toolbook).called,
    Some(SpanName::Server {
      server: "web".to_owned(),
      tool: "web_fetch".to_owned()
    })
  );
  assert_eq!(
    span_reading(&span("tool", "Bash", 1.0), &toolbook).called,
    Some(SpanName::Tool {
      name: "Bash".to_owned()
    })
  );
}

#[test]
fn a_coworker_is_named_by_its_agent_type_beside_the_words_it_was_handed() {
  let typed = span_reading(&span("agent", "Agent(scout) \u{b7} find the launcher", 1.0), &no_server);
  let bare = span_reading(&span("agent", "Task", 1.0), &no_server);
  let branch = span_reading(&span("agent", "branch 2", 1.0), &no_server);

  assert_eq!(
    typed.called,
    Some(SpanName::Coworker {
      agent_type: Some("scout".to_owned())
    })
  );
  assert_eq!(typed.said.as_deref(), Some("find the launcher"));
  assert_eq!(bare.called, Some(SpanName::Coworker { agent_type: None }));
  assert_eq!(bare.said, None);
  assert_eq!(branch.called, None);
}

#[test]
fn the_run_divides_into_waiting_tooling_and_generating() {
  let spans = [
    span("phase", RUN_PHASE, 100.0),
    span("tool", "TaskOutput", 30.0),
    span("tool", "Bash", 20.4),
    span("agent", "Agent(scout)", 9.6),
    span("agent", "branch 1", 50.0),
  ];

  assert_eq!(
    run_split(&spans),
    Some(RunSplit {
      waiting: 30.0,
      tooling: 30.0,
      generating: 40.0
    })
  );
  assert_eq!(run_split(&[span("tool", "Bash", 3.0)]), None);
  // Spans longer than the run itself divide nothing.
  assert_eq!(run_split(&[span("phase", RUN_PHASE, 10.0), span("tool", "Bash", 30.0)]), None);
}

#[test]
fn an_item_nobody_ran_took_no_time() {
  assert_eq!(time_reading(&[], &no_server), None);
}

#[test]
fn the_attempts_add_up_and_each_model_spent_what_it_wrote_down() {
  let runs = [
    (
      run(1, Some(4_000)),
      vec![
        span("phase", RUN_PHASE, 4.0),
        span("tool", "TaskOutput", 1.0),
        span("agent", "Agent(scout)", 2.0),
        usage("model-a", 100, 60),
      ],
    ),
    (run(2, Some(6_000)), vec![usage("model-b", 300, 200)]),
    (run(3, None), vec![]),
  ];
  let read = time_reading(&runs, &no_server).unwrap();

  assert_eq!(read.attempts.len(), 3);
  assert_eq!((read.total_ms, read.finished, read.missing), (10_000, 2, 1));
  assert_eq!(read.attempts[2].took_ms, None);
  assert_eq!(read.phases.len(), 1);
  // A co-worker span stands, so the wait on the background branches does not.
  assert_eq!(
    read.inside.iter().map(|span| span.name.as_str()).collect::<Vec<_>>(),
    vec!["Agent(scout)"]
  );
  assert_eq!(read.inside_more, 0);
  assert_eq!(
    read.usage.iter().map(|spent| spent.model.as_str()).collect::<Vec<_>>(),
    vec!["model-b", "model-a"]
  );
  assert_eq!(read.usage[0].kinds[0].kind, TokenKind::Input);
  assert_eq!(read.usage[0].kinds[0].tokens, Some(200));
  assert_eq!(read.usage[0].kinds[1].tokens, None);
}

#[test]
fn a_reading_carries_the_twelve_longest_of_what_was_busy_inside_the_run() {
  let spans: Vec<Span> = (0..15).map(|at| span("tool", &format!("tool{at}"), f64::from(at))).collect();
  let read = time_reading(&[(run(1, Some(1)), spans)], &no_server).unwrap();

  assert_eq!(read.inside.len(), 12);
  assert_eq!(read.inside_more, 3);
  assert_eq!(read.inside[0].name, "tool14");
}
