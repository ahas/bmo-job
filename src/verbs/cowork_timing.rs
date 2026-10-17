//! Where a work order's time went (docs/verbs.md, "What a job cost"): `job cowork-timing`, read off the
//! order's body, its jobs' rows and spans, and bmo-agency's pool.

use indexmap::IndexMap;
use job_agency::Agency;
use sea_orm::DatabaseConnection;
use serde_json::Value;

use super::stand::{Stand, VerbError};
use crate::queue::units::agency_failed;
use crate::{all, one};

/// A stretch a task was running for, in seconds since the epoch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Run {
  pub from: f64,
  pub to: f64,
}

/// How long a set of runs takes together, merged where they overlap, so runs side by side count once and the
/// time between runs counts for none of it; none where there is no run.
pub fn run_seconds(runs: &[Run]) -> Option<f64> {
  let mut spans: Vec<Run> = runs
    .iter()
    .map(|run| Run {
      from: run.from,
      to: run.to.max(run.from),
    })
    .collect();

  spans.sort_by(|one, other| one.from.total_cmp(&other.from));

  let (first, rest) = spans.split_first()?;
  let mut whole = 0.0;
  let mut open = *first;

  for span in rest {
    if span.from > open.to {
      whole += open.to - open.from;
      open = *span;
    } else if span.to > open.to {
      open.to = span.to;
    }
  }

  Some(whole + (open.to - open.from))
}

struct Step {
  label: String,
  builder: bool,
  start: f64,
  end: f64,
  tasks: i64,
  jobs: Vec<i64>,
  runs: Vec<Run>,
}

/// Which work orders to read: the items named, or the newest that planned and finished.
pub enum Orders {
  Items(Vec<i64>),
  Recent(i64),
}

/// The usage the verb answers words it cannot read with.
pub const USAGE: &str = "usage: job cowork-timing <item>... [--steps] | --recent <n> [--steps]";

// Whole minutes, a nought never signed.
fn minutes(seconds: f64) -> String {
  format!("{}m", (seconds / 60.0).round() + 0.0)
}

// The one number a statement answers as its column `n`, cast to a double by the statement; none where it
// answers no row or a null.
async fn number(
  db: &DatabaseConnection,
  sql: &str,
  value: sea_orm::Value,
) -> Result<Option<f64>, VerbError> {
  let row = one(db, sql, vec![value]).await?;

  Ok(row.map(|row| row.try_get::<Option<f64>>("", "n")).transpose()?.flatten())
}

// Every task of an order under the label its step is read by.
fn labelled(order: &Value) -> Vec<(String, &Value)> {
  let mut labelled: Vec<(String, &Value)> = Vec::new();

  for task in order["history"].as_array().into_iter().flatten() {
    labelled.push((task["id"].as_str().unwrap_or_default().to_owned(), task));
  }

  for (n, stage) in order["stages"].as_array().into_iter().flatten().enumerate() {
    let label = format!("stage {}: {}", n + 1, stage["title"].as_str().unwrap_or_default());

    for task in stage["tasks"].as_array().into_iter().flatten() {
      labelled.push((label.clone(), task));
    }
  }

  for task in order["tasks"].as_array().into_iter().flatten() {
    let name = task["title"].as_str().or_else(|| task["id"].as_str()).unwrap_or_default();

    labelled.push((format!("build: {name}"), task));
  }

  if order["control"].is_object() {
    labelled.push((order["control"]["id"].as_str().unwrap_or_default().to_owned(), &order["control"]));
  }

  labelled
}

// How long the pool kept an order's tasks waiting for their places, in seconds.
async fn pool_wait(stand: &Stand, order: &Value) -> Result<f64, VerbError> {
  let mut waited = 0;

  for (_, task) in labelled(order) {
    let Some(token) = task["poolRequest"].as_str().filter(|token| !token.is_empty()) else {
      continue;
    };
    let request = stand.ground.agency.pool_request(token).await.map_err(agency_failed)?;

    if let Some(leased) = request.as_ref().and_then(|request| request.leased_at) {
      waited += leased - request.map_or(leased, |request| request.enqueued_at);
    }
  }

  Ok(waited as f64 / 1000.0)
}

/// Where each work order's time went: the time it was running, the part of it in builder turns and in branch
/// stages, the tool time and output tokens inside the builder turns, the stretch from the plan's start to the
/// last turn's end and how much of it nothing ran, and the time the pool kept it waiting; with `steps`, each
/// step on a line of its own.
pub async fn cowork_timing(stand: &Stand, orders: Orders, steps: bool) -> Result<Vec<String>, VerbError> {
  let db = &stand.db;
  let ids: Vec<i64> = match orders {
    Orders::Items(ids) => ids,
    Orders::Recent(count) => all(
      db,
      r#"SELECT DISTINCT "todo_id" FROM "jobs"
         WHERE "unit" LIKE 'bmo-work-%-plan-%' AND "finished_at" IS NOT NULL AND "todo_id" IS NOT NULL
         ORDER BY "todo_id" DESC LIMIT $1"#,
      vec![count.into()],
    )
    .await?
    .iter()
    .map(|row| row.try_get::<i64>("", "todo_id"))
    .collect::<Result<_, _>>()?,
  };

  if ids.is_empty() {
    return Err(VerbError::Ended {
      code: 2,
      said: USAGE.to_owned(),
    });
  }

  let mut lines = Vec::new();

  for id in ids {
    let Some(row) =
      one(db, r#"SELECT "body" FROM "work_orders" WHERE "todo_id" = $1"#, vec![id.into()]).await?
    else {
      continue;
    };
    let order: Value = serde_json::from_str(&row.try_get::<String>("", "body")?)
      .map_err(|why| VerbError::Refused(format!("cowork-timing: #{id}'s work order is not JSON: {why}")))?;
    let mut by_label: IndexMap<String, Step> = IndexMap::new();

    for (label, task) in labelled(&order) {
      let (Some(job_id), Some(started)) = (task["jobId"].as_i64(), task["startedAt"].as_f64()) else {
        continue;
      };
      let finished = number(
        db,
        r#"SELECT CAST("finished_at" AS DOUBLE PRECISION) AS "n" FROM "jobs" WHERE "id" = $1"#,
        job_id.into(),
      )
      .await?;
      let Some(finished) = finished.filter(|finished| *finished != 0.0).map(|finished| finished / 1000.0)
      else {
        continue;
      };
      let step = by_label.entry(label.clone()).or_insert_with(|| Step {
        label,
        builder: true,
        start: f64::INFINITY,
        end: 0.0,
        tasks: 0,
        jobs: Vec::new(),
        runs: Vec::new(),
      });

      step.builder &= task["role"].as_str() == Some("builder");
      step.start = step.start.min(started / 1000.0);
      step.end = step.end.max(finished);
      step.tasks += 1;
      step.jobs.push(job_id);
      step.runs.push(Run {
        from: started / 1000.0,
        to: finished,
      });
    }

    let mut every: Vec<Step> = by_label.into_values().collect();

    every.sort_by(|one, other| one.start.total_cmp(&other.start));

    let Some(first) = every.first() else { continue };
    let begun = first.start;
    let (mut builder, mut branch, mut tool, mut output, mut reached) = (0.0, 0.0, 0.0, 0.0, begun);

    for step in &every {
      let ran = run_seconds(&step.runs).unwrap_or(0.0);

      if step.builder {
        builder += ran;

        for job in &step.jobs {
          tool += number(
            db,
            r#"SELECT CAST(COALESCE(SUM("seconds"), 0) AS DOUBLE PRECISION) AS "n" FROM "job_spans"
               WHERE "job_id" = $1 AND "kind" = 'tool'"#,
            (*job).into(),
          )
          .await?
          .unwrap_or(0.0);
          output += number(
            db,
            r#"SELECT CAST(COALESCE(SUM("output_tokens"), 0) AS DOUBLE PRECISION) AS "n" FROM "job_spans"
               WHERE "job_id" = $1 AND "kind" = 'usage'"#,
            (*job).into(),
          )
          .await?
          .unwrap_or(0.0);
        }
      } else {
        branch += ran;
      }

      reached = reached.max(step.end);
    }

    // What the order worked is every run merged: an order paused for days, a stage waiting for a place in the
    // pool and a task rerun long after its stage all fall between the runs.
    let runs: Vec<Run> = every.iter().flat_map(|step| step.runs.iter().copied()).collect();
    let total = run_seconds(&runs).unwrap_or(0.0);
    let span = reached - begun;
    let turns: i64 = every.iter().filter(|step| step.builder).map(|step| step.tasks).sum();
    let branch_steps = every.iter().filter(|step| !step.builder).count();
    let branch_tasks: i64 = every.iter().filter(|step| !step.builder).map(|step| step.tasks).sum();
    let waited = pool_wait(stand, &order).await?;
    let share = if total == 0.0 { 0.0 } else { (100.0 * builder / total).round() };

    lines.push(
      [
        format!("#{id} {}", order["status"].as_str().unwrap_or_default()),
        format!("total {}", minutes(total)),
        format!(
          "builder {} ({share}%, {turns} turns, tools {}, {}k out)",
          minutes(builder),
          minutes(tool),
          (output / 1000.0).round()
        ),
        format!("branches {} ({branch_steps} stages, {branch_tasks} tasks)", minutes(branch)),
        format!("span {}", minutes(span)),
        format!("idle {}", minutes(span - total)),
        format!("pool wait {}s", waited.round()),
      ]
      .join(" · "),
    );

    if steps {
      for step in &every {
        lines.push(format!(
          "  {} {:>5}s ×{} {}",
          if step.builder { "B" } else { "-" },
          run_seconds(&step.runs).unwrap_or(0.0).round(),
          step.tasks,
          step.label
        ));
      }
    }
  }

  Ok(lines)
}
