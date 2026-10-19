//! What a job cost (docs/verbs.md, "What a job cost"): one job's spans in the database and its lines in
//! bmo-agency's agent log, and the figures `job figures` reads out of them.

use bmo_job::verbs::job_figures::{ClassRow, ShellClass, read};
use job_agency::{JobLines, ToolCall, ToolResult};
use queue_read::time::RUN_PHASE;

use crate::support::desk::Desk;

// The item the job ran for.
const ITEM: i64 = 4242;

async fn span(
  desk: &Desk,
  job: i64,
  kind: &str,
  name: &str,
  seconds: f64,
  times: i64,
  usage: [Option<i64>; 4],
) {
  desk
    .run(
      r#"INSERT INTO "job_spans" ("job_id", "todo_id", "kind", "name", "seconds", "times", "tokens", "input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens")
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
      vec![
        job.into(),
        ITEM.into(),
        kind.into(),
        name.into(),
        seconds.into(),
        times.into(),
        usage.iter().flatten().sum::<i64>().into(),
        usage[0].into(),
        usage[1].into(),
        usage[2].into(),
        usage[3].into(),
      ],
    )
    .await;
}

fn call(id: i64, reference: &str, at: i64, command: &str) -> ToolCall {
  ToolCall {
    id,
    agent: "worker".into(),
    run: Some("job-1".into()),
    reference: Some(reference.into()),
    name: "Bash".into(),
    source: Some("claude-code".into()),
    at,
    command: Some(command.into()),
    args: None,
  }
}

fn answer(id: i64, reference: &str, at: i64, length: u64) -> ToolResult {
  ToolResult {
    id,
    agent: "worker".into(),
    run: Some("job-1".into()),
    reference: Some(reference.into()),
    complete: true,
    length: Some(length),
    at,
  }
}

#[tokio::test(flavor = "multi_thread")]
async fn one_job_s_figures_are_read_off_its_spans_and_its_lines_in_the_agent_log() {
  let desk = Desk::new().await;

  desk
    .run(
      r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "finished_at", "outcome", "prompt", "todo_id", "worker")
         VALUES (1, 'bmo-task-4242', 1000000, 1600000, 'done', 'work', $1, 'worker')"#,
      vec![ITEM.into()],
    )
    .await;

  let job = desk.count(r#"SELECT MAX("id") FROM "jobs""#, vec![]).await;

  span(&desk, job, "phase", RUN_PHASE, 500.0, 1, [None; 4]).await;
  span(&desk, job, "tool", "Bash", 300.0, 2, [None; 4]).await;
  span(&desk, job, "tool", "mcp__worker__graph_search", 0.1, 3, [None; 4]).await;
  span(&desk, job, "tool", "mcp__worker__graph_impact", 0.1, 1, [None; 4]).await;
  span(
    &desk,
    job,
    "usage",
    "claude-opus-5-5",
    0.0,
    1,
    [Some(400), Some(9_000), Some(4_000_000), Some(60_000)],
  )
  .await;

  // Two calls, a type check that took a minute and a test run that took four, three model requests and a
  // compaction.
  let lines = JobLines {
    calls: vec![
      call(1, "t1", 1_100_000, "cd /repo && bunx tsc --noEmit"),
      call(2, "t2", 1_200_000, "timeout 590 bun test --parallel a.test.ts"),
    ],
    results: vec![answer(3, "t1", 1_160_000, 2_000), answer(4, "t2", 1_440_000, 31_000)],
    requests: 3,
    compactions: 1,
  };

  desk.agency.answer("GET /log/jobs/{job}", 200, serde_json::to_value(&lines).unwrap());

  let read = read(&desk.stand, &[ITEM]).await.unwrap();
  let figures = &read[0].jobs[0];

  desk.agency.took("GET /log/jobs/{job}", &format!("/log/jobs/{job}"), None);
  assert_eq!(read[0].todo, ITEM);
  assert_eq!(figures.job, job);
  assert_eq!(figures.wall_seconds, Some(600));
  // The run less its tools, in the whole seconds the division counts in.
  assert_eq!(figures.model_seconds, Some(200.0));
  assert_eq!((figures.shell_calls, figures.shell_seconds), (2, 300.0));
  assert_eq!(figures.requests, 3);
  assert_eq!(
    (
      figures.input_tokens,
      figures.output_tokens,
      figures.cache_read_tokens,
      figures.cache_write_tokens
    ),
    (400, 9_000, 4_000_000, 60_000)
  );
  assert_eq!((figures.graph_search, figures.graph_get, figures.graph_impact), (3, 0, 1));
  assert_eq!(figures.compactions, 1);
  assert_eq!(
    figures.classes,
    [
      ClassRow {
        class: ShellClass::BunTest,
        calls: 1,
        seconds: 240,
      },
      ClassRow {
        class: ShellClass::TypeCheck,
        calls: 1,
        seconds: 60,
      },
    ]
  );
  assert_eq!(figures.longest.iter().map(|long| long.chars).collect::<Vec<_>>(), [31_000, 2_000]);
  assert!(
    figures.longest[0].said.contains("bun test --parallel a.test.ts"),
    "{:?}",
    figures.longest
  );
}
