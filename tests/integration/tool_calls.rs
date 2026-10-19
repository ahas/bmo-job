//! What a job's own tool calls cost it (docs/verbs.md, "What a job cost"), read out of the lines bmo-agency's
//! agent log keeps for the job: the pairing on made-up lines, and the verb over a stand whose agency answers
//! the item's last job's lines.

use bmo_job::verbs::tool_calls::{calls_of, tool_calls, tool_calls_lines};
use job_agency::{JobLines, ToolCall, ToolResult};
use serde_json::{Value, json};

use crate::support::desk::Desk;

const OPENED: i64 = 1_788_743_531_000;

fn moment(seconds: i64) -> i64 {
  OPENED + seconds * 1000
}

fn used(seconds: i64, reference: &str, name: &str, input: Value) -> ToolCall {
  ToolCall {
    id: seconds,
    agent: "worker".into(),
    run: Some("job-1".into()),
    reference: Some(reference.into()),
    name: name.into(),
    source: None,
    at: moment(seconds),
    command: input.get("command").and_then(Value::as_str).map(str::to_owned),
    args: Some(input.to_string()),
  }
}

fn ran(seconds: i64, reference: &str, command: &str) -> ToolCall {
  used(seconds, reference, "Bash", json!({ "command": command }))
}

fn answered(seconds: i64, reference: &str) -> ToolResult {
  ToolResult {
    id: 10_000 + seconds,
    agent: "worker".into(),
    run: Some("job-1".into()),
    reference: Some(reference.into()),
    complete: true,
    length: Some(2),
    at: moment(seconds),
  }
}

fn lines(calls: Vec<ToolCall>, results: Vec<ToolResult>) -> JobLines {
  JobLines {
    calls,
    results,
    requests: 0,
    compactions: 0,
  }
}

fn record() -> JobLines {
  lines(
    vec![
      ran(10, "toolu_1", "bun test --parallel"),
      used(60, "toolu_2", "Read", json!({ "file_path": "/srv/repos/archive.ts" })),
      ran(100, "toolu_3", "git status"),
    ],
    vec![answered(45, "toolu_1"), answered(62, "toolu_2"), answered(105, "toolu_3")],
  )
}

#[test]
fn every_call_is_paired_with_the_result_that_answered_it_longest_first_and_added_up_by_tool() {
  let split = tool_calls(calls_of(&record()), moment(200)).unwrap();

  assert_eq!((split.times, split.open, split.seconds), (3, 0, 42));
  assert_eq!(
    split.calls.iter().map(|call| (call.name.as_str(), call.seconds)).collect::<Vec<_>>(),
    [("Bash", 35.0), ("Bash", 5.0), ("Read", 2.0)]
  );
  assert_eq!(split.calls[0].said, ["bun test --parallel"]);
  assert_eq!(split.calls[2].said, ["/srv/repos/archive.ts"]);
  assert_eq!(
    split
      .tools
      .iter()
      .map(|tool| (tool.name.as_str(), tool.seconds, tool.times, tool.open))
      .collect::<Vec<_>>(),
    [("Bash", 40, 2, 0), ("Read", 2, 1, 0)]
  );
}

#[test]
fn the_suite_s_own_calls_are_counted_apart_however_the_line_was_written() {
  let suite = lines(
    vec![
      ran(10, "toolu_1", "bun test --parallel"),
      ran(30, "toolu_2", "cd web && bun test agents/agent/tool-calls.test.ts"),
      ran(60, "toolu_3", "bun run scripts/deploy.ts"),
    ],
    vec![answered(20, "toolu_1"), answered(50, "toolu_2"), answered(90, "toolu_3")],
  );

  assert_eq!(tool_calls(calls_of(&suite), moment(200)).unwrap().tests, (30, 2));

  // A run of the suite written past where the shown lines stop is still the suite's.
  let buried = lines(
    vec![
      ran(
        10,
        "toolu_1",
        &format!("grep -n \"{}\" archive/archive.ts | head -20; cargo test -p archive", "x".repeat(150)),
      ),
      ran(40, "toolu_2", "echo one\necho two\necho three\nbun test --parallel"),
    ],
    vec![answered(25, "toolu_1"), answered(60, "toolu_2")],
  );

  assert_eq!(tool_calls(calls_of(&buried), moment(200)).unwrap().tests, (35, 2));
}

#[test]
fn a_call_still_out_runs_to_the_end_the_earliest_answer_ends_it_and_a_proxy_call_is_the_tool_it_reached() {
  let cut = lines(
    vec![ran(10, "toolu_1", "git status"), ran(100, "toolu_2", "bun test")],
    vec![answered(30, "toolu_1")],
  );
  let split = tool_calls(calls_of(&cut), moment(300)).unwrap();

  assert_eq!(split.open, 1);
  assert_eq!((split.calls[0].seconds, split.calls[0].open), (200.0, true));
  assert_eq!(split.tests, (200, 1));

  let twice = lines(
    vec![ran(10, "toolu_1", "git status")],
    vec![answered(90, "toolu_1"), answered(30, "toolu_1")],
  );

  assert_eq!(tool_calls(calls_of(&twice), moment(200)).unwrap().calls[0].seconds, 20.0);

  let book = lines(
    vec![used(
      10,
      "toolu_book",
      "mcp__toolbook__call",
      json!({ "tool": "web/naver_price", "arguments": { "query": "RTX 5090" } }),
    )],
    vec![answered(16, "toolu_book")],
  );
  let split = tool_calls(calls_of(&book), moment(200)).unwrap();

  assert_eq!(
    split
      .calls
      .iter()
      .map(|call| (call.name.as_str(), call.seconds, call.said.clone()))
      .collect::<Vec<_>>(),
    [("mcp__web__naver_price", 6.0, vec!["RTX 5090".to_owned()])]
  );
  assert_eq!(
    split.tools.iter().map(|tool| tool.name.as_str()).collect::<Vec<_>>(),
    ["mcp__web__naver_price"]
  );
}

#[test]
fn a_result_with_no_call_is_left_out_and_a_call_is_named_by_its_first_lines_alone() {
  assert!(tool_calls(calls_of(&lines(vec![], vec![answered(30, "toolu_gone")])), moment(200)).is_none());
  assert!(tool_calls(calls_of(&lines(vec![], vec![])), moment(200)).is_none());

  let long = lines(
    vec![ran(
      10,
      "toolu_1",
      &format!("\n\ncd /srv/work && {}\nbun test\ngit status\ngit push", "x".repeat(200)),
    )],
    vec![answered(20, "toolu_1")],
  );

  assert_eq!(
    tool_calls(calls_of(&long), moment(200)).unwrap().calls[0].said,
    [
      format!("cd /srv/work && {}…", "x".repeat(104)),
      "bun test".to_owned(),
      "git status".to_owned()
    ]
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_verb_reads_the_lines_of_the_item_s_last_job() {
  let desk = Desk::new().await;

  for (launched, finished) in [(OPENED - 9_000_000, OPENED - 8_000_000), (OPENED, moment(300))] {
    desk
      .run(
        r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "finished_at", "prompt", "todo_id", "worker", "model")
           VALUES (1, 'bmo-work-9', $1, $2, 'work', 9, 'worker', 'claude-opus-5')"#,
        vec![launched.into(), finished.into()],
      )
      .await;
  }

  let job = desk.count(r#"SELECT MAX("id") FROM "jobs""#, vec![]).await;

  desk.agency.answer("GET /log/jobs/{job}", 200, serde_json::to_value(record()).unwrap());

  let said = tool_calls_lines(&desk.stand, 9, 2).await.unwrap().lines;

  desk.agency.took("GET /log/jobs/{job}", &format!("/log/jobs/{job}"), None);
  assert!(said[0].starts_with(&format!("#9 job {job}, the last of 2 runs · ")), "{said:?}");
  assert!(
    said.iter().any(|line| line.starts_with("3 calls, ") && line.contains(" out in all")),
    "{said:?}"
  );
  assert!(said.iter().any(|line| line == "the 2 longest of 3 calls:"), "{said:?}");
  assert!(said.iter().any(|line| line == "     bun test --parallel"), "{said:?}");
  assert!(
    !said.iter().any(|line| line == "     /srv/repos/archive.ts"),
    "the third call is past the top: {said:?}"
  );

  let none = tool_calls_lines(&desk.stand, 8, 0).await.unwrap().lines;

  assert_eq!(none, ["tool-calls: no job ran #8"]);
}
