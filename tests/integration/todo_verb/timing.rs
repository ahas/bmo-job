//! How long an item took and what it burnt (docs/api.md, "Readings"), read through the server's reading of
//! the item off the runs behind it and the spans each run left.

use queue_read::time::{BRANCH_WORD, RUN_PHASE};
use serde_json::{Value, json};

use crate::support::todo::Bench;

const SECOND: i64 = 1000;

// One run behind an item, timed in seconds from the start of the test's clock.
struct Run {
  launched: i64,
  finished: Option<i64>,
  outcome: Option<&'static str>,
}

const fn run(launched: i64, finished: Option<i64>, outcome: Option<&'static str>) -> Run {
  Run {
    launched,
    finished,
    outcome,
  }
}

// One approved item with as many runs behind it as the test wants.
async fn item_with_runs(runs: &[Run]) -> (Bench, i64, Vec<i64>) {
  let bench = Bench::new().await;
  let id = bench.item("work that keeps its time", "Conclusion\n\nKeep the time too.").await;
  let mut jobs = vec![];

  for (at, one) in runs.iter().enumerate() {
    let job = bench.job(id, &format!("bmo-task-{at}")).await;

    bench
      .run(
        r#"UPDATE "jobs" SET "launched_at" = $2, "finished_at" = $3, "outcome" = $4 WHERE "id" = $1"#,
        vec![
          job.into(),
          (one.launched * SECOND).into(),
          one.finished.map(|finished| finished * SECOND).into(),
          one.outcome.into(),
        ],
      )
      .await;
    jobs.push(job);
  }

  (bench, id, jobs)
}

// One span a run left: its kind, its name, its seconds, how many times, and the tokens it stated.
struct Span {
  kind: &'static str,
  name: String,
  seconds: f64,
  times: i64,
  tokens: Option<i64>,
  split: Option<[i64; 4]>,
}

fn span(kind: &'static str, name: &str, seconds: f64, times: i64) -> Span {
  Span {
    kind,
    name: name.to_owned(),
    seconds,
    times,
    tokens: None,
    split: None,
  }
}

// What one model burnt in a run, each of the four kinds by name.
fn usage(model: &str, total: i64, split: [i64; 4]) -> Span {
  Span {
    tokens: Some(total),
    split: Some(split),
    ..span("usage", model, 0.0, 1)
  }
}

async fn record(bench: &Bench, job: i64, todo: i64, spans: Vec<Span>) {
  for one in spans {
    let [input, output, read, write] = one.split.map_or([None; 4], |split| split.map(Some));

    bench
      .run(
        r#"INSERT INTO "job_spans" ("job_id", "todo_id", "kind", "name", "seconds", "times", "tokens", "input_tokens", "output_tokens", "cache_read_tokens", "cache_write_tokens")
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)"#,
        vec![
          job.into(),
          todo.into(),
          one.kind.into(),
          one.name.into(),
          one.seconds.into(),
          one.times.into(),
          one.tokens.into(),
          input.into(),
          output.into(),
          read.into(),
          write.into(),
        ],
      )
      .await;
  }
}

async fn time(bench: &Bench, id: i64) -> Value {
  bench.shown(id).await["time"].clone()
}

fn names(spans: &Value) -> Vec<(String, f64)> {
  spans
    .as_array()
    .unwrap()
    .iter()
    .map(|span| (span["name"].as_str().unwrap().to_owned(), span["seconds"].as_f64().unwrap()))
    .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn every_attempt_is_added_up_and_named_and_the_spans_spread_longest_first() {
  let (bench, id, jobs) =
    item_with_runs(&[run(1000, Some(1300), Some("failed")), run(2000, Some(2600), Some("done"))]).await;

  record(
    &bench,
    jobs[0],
    id,
    vec![
      span("phase", "setup", 3.0, 1),
      span("phase", RUN_PHASE, 290.0, 1),
      span("tool", "Bash", 200.0, 12),
      span("tool", "TaskOutput", 400.0, 1),
    ],
  )
  .await;
  record(
    &bench,
    jobs[1],
    id,
    vec![
      span("phase", RUN_PHASE, 590.0, 1),
      Span {
        tokens: Some(292_463),
        ..span("agent", &format!("{BRANCH_WORD} 1 · build:usage · claude-opus-5"), 500.0, 1)
      },
    ],
  )
  .await;

  let time = time(&bench, id).await;

  assert_eq!(time["total_ms"], 900 * SECOND);
  assert_eq!(
    time["attempts"],
    json!([
      { "job": jobs[0], "took_ms": 300 * SECOND, "outcome": "failed" },
      { "job": jobs[1], "took_ms": 600 * SECOND, "outcome": "done" },
    ])
  );
  assert_eq!(names(&time["phases"]), [(RUN_PHASE.to_owned(), 880.0), ("setup".to_owned(), 3.0)]);
  assert_eq!(time["phases"][0]["times"], 2);
  // The wait on the branches is not carried beside the branch it waited on: those are the same seconds.
  assert_eq!(
    names(&time["inside"]),
    [
      (format!("{BRANCH_WORD} 1 · build:usage · claude-opus-5"), 500.0),
      ("Bash".to_owned(), 200.0)
    ]
  );
  assert_eq!(time["inside"][0]["tokens"], 292_463);
  assert_eq!(time["inside"][1]["times"], 12);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_run_divides_into_the_wait_on_the_branches_its_own_tools_and_the_model() {
  let (bench, id, jobs) = item_with_runs(&[run(1000, Some(2600), Some("done"))]).await;

  record(
    &bench,
    jobs[0],
    id,
    vec![
      span("phase", RUN_PHASE, 1490.0, 1),
      span("tool", "TaskOutput", 1006.0, 2),
      span("agent", &format!("{BRANCH_WORD} 1 · build:rename · claude-opus-5"), 613.0, 1),
      span("tool", "Bash", 54.0, 28),
      span("tool", "mcp__chrome-devtools__take_snapshot", 52.0, 19),
    ],
  )
  .await;

  assert_eq!(
    time(&bench, id).await["split"],
    json!({ "waiting": 1006.0, "tooling": 106.0, "generating": 378.0 })
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn no_division_is_made_where_the_spans_do_not_make_one() {
  let (bench, id, jobs) = item_with_runs(&[run(1000, Some(2600), Some("done"))]).await;

  record(
    &bench,
    jobs[0],
    id,
    vec![
      span("phase", RUN_PHASE, 600.0, 1),
      span("tool", "TaskOutput", 500.0, 1),
      span("tool", "Bash", 400.0, 9),
    ],
  )
  .await;

  let time = time(&bench, id).await;

  assert_eq!(time["phases"].as_array().unwrap().len(), 1);
  assert_eq!(time["split"], Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn what_the_item_burnt_is_said_each_kind_by_name_added_up_over_the_attempts() {
  let (bench, id, jobs) =
    item_with_runs(&[run(1000, Some(1300), Some("failed")), run(2000, Some(2600), Some("done"))]).await;

  record(
    &bench,
    jobs[0],
    id,
    vec![
      span("phase", RUN_PHASE, 290.0, 1),
      usage("claude-opus-5", 1_040_000, [1000, 9000, 1_000_000, 30_000]),
    ],
  )
  .await;
  record(
    &bench,
    jobs[1],
    id,
    vec![
      usage("claude-opus-5", 520_000, [500, 4500, 500_000, 15_000]),
      usage("claude-haiku-4-5", 3000, [100, 400, 2000, 500]),
    ],
  )
  .await;

  let time = time(&bench, id).await;
  let kinds = |input: i64, output: i64, read: i64, write: i64| {
    json!([
      { "kind": "input", "tokens": input }, { "kind": "output", "tokens": output },
      { "kind": "cache_read", "tokens": read }, { "kind": "cache_write", "tokens": write },
    ])
  };

  // Both attempts of the same model are one, and the heaviest model stands first.
  assert_eq!(
    time["usage"],
    json!([
      { "model": "claude-opus-5", "tokens": 1_560_000, "kinds": kinds(1500, 13_500, 1_500_000, 45_000) },
      { "model": "claude-haiku-4-5", "tokens": 3000, "kinds": kinds(100, 400, 2000, 500) },
    ])
  );
  assert_eq!(time["missing"], 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_attempt_that_left_no_spending_is_counted_as_missing() {
  let (bench, id, jobs) =
    item_with_runs(&[run(1000, Some(1600), Some("failed")), run(2000, Some(2200), Some("done"))]).await;

  // The first attempt ran before the spending was kept; only the second left any.
  record(&bench, jobs[0], id, vec![span("phase", RUN_PHASE, 590.0, 1)]).await;
  record(&bench, jobs[1], id, vec![usage("claude-opus-5", 1000, [100, 100, 700, 100])]).await;

  let time = time(&bench, id).await;

  assert_eq!(time["usage"].as_array().unwrap().len(), 1);
  assert_eq!(time["missing"], 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_that_left_no_record_of_what_they_burnt_carry_no_spending() {
  let (bench, id, jobs) = item_with_runs(&[run(1000, Some(1300), Some("done"))]).await;

  record(&bench, jobs[0], id, vec![span("phase", RUN_PHASE, 290.0, 1)]).await;

  let time = time(&bench, id).await;

  // The spans were kept before the spending was, so the time is there and the tokens are not.
  assert_eq!(time["usage"], json!([]));
  assert_eq!(time["missing"], 1);
  assert_eq!(time["phases"].as_array().unwrap().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_still_going_counts_as_nothing_and_says_that_it_is_going() {
  let (bench, id, jobs) =
    item_with_runs(&[run(1000, Some(1300), Some("failed")), run(2000, None, None)]).await;
  let time = time(&bench, id).await;

  assert_eq!((time["total_ms"].as_i64(), time["finished"].as_i64()), (Some(300 * SECOND), Some(1)));
  assert_eq!(time["attempts"][1], json!({ "job": jobs[1], "took_ms": null, "outcome": null }));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_run_that_has_not_finished_is_no_total_of_nothing() {
  let (bench, id, jobs) = item_with_runs(&[run(1000, None, None)]).await;
  let time = time(&bench, id).await;

  // There is no finished attempt to add up yet, and its spans are not missing: the run has not ended.
  assert_eq!(time["finished"], 0);
  assert_eq!(time["attempts"], json!([{ "job": jobs[0], "took_ms": null, "outcome": null }]));
  assert_eq!(time["phases"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_nobody_has_run_carries_no_time() {
  let (bench, id, _) = item_with_runs(&[]).await;

  assert_eq!(time(&bench, id).await, Value::Null);
}

#[tokio::test(flavor = "multi_thread")]
async fn runs_older_than_the_keeping_of_spans_are_added_up_with_no_spans() {
  let (bench, id, _) = item_with_runs(&[run(1000, Some(1300), Some("done"))]).await;
  let time = time(&bench, id).await;

  assert_eq!((time["total_ms"].as_i64(), time["finished"].as_i64()), (Some(300 * SECOND), Some(1)));
  assert_eq!((time["phases"].clone(), time["inside"].clone()), (json!([]), json!([])));
}
