//! A co-work branch's turn (docs/runs.md, "The turns"): a session it throws away, in the place it is given and
//! under the name its task alone is given, on the road its lease names — Claude Code, Codex or OpenRouter — with
//! no session of the worker's read, recorded or handed over, no bench made for it, and on a stand-in road the
//! upstream that took the turn written on the job's row.

use std::path::PathBuf;

use bmo_job::job_run::{WORK_SESSION_FRESH, WORK_STEP_PHASE};
use bmo_job::queue::outcomes::JOB_WORK_STEP;
use job_agency::{Provider, Reply, TurnAsking, TurnSeat};

use super::pooled_branch;
use crate::support::job::{Bench, Given, Ran, WORKER};
use crate::support::turns::{Played, reply};

const ANSWER: &str = "I read the one angle: the panel draws its height from the theme.";
const TODO: i64 = 4;

// Where the branch runs: the standing branch checkout of its pool, or a folder outside every one.
enum Where {
  Standing,
  Loose,
}

struct Seen {
  bench: Bench,
  ran: Ran,
  place: PathBuf,
  asked: TurnAsking,
  road: (Option<String>, Option<String>),
}

// A branch of item 4's order on the road given, its model and its answer, run in the place given.
async fn side(provider: &str, model: &str, answer: Reply, place: Where) -> Seen {
  let mut bench = Bench::new().await;
  let dir = bench.checkout("checkout");
  let place = match place {
    Where::Standing => bench.at("branch-agents/pool/1"),
    Where::Loose => bench.at("loose-branch"),
  };
  let unit = format!("bmo-work-{TODO}-stage-0-0-1");
  // The name the work-order runner gives the branch for this task alone, which no worker's row declares.
  let named = format!("work-order-{TODO}-stage-0-0-1");

  bench.scene.units.providers.push(Provider {
    name: "openrouter".into(),
    enabled: true,
    registered: false,
    coworker: true,
    lines: vec!["worker".into()],
  });
  std::fs::create_dir_all(&place).unwrap();
  bench.worker(WORKER, &dir, 0).await;

  let job = pooled_branch(&bench, TODO, provider, model, &unit, &named).await;

  bench.agent.play([Played::answered(answer)]);

  let ran = bench
    .run(&Given {
      model: format!("{provider}::{model}"),
      unit,
      worker: named,
      ..bench.step(job, &place, WORK_STEP_PHASE, WORK_SESSION_FRESH)
    })
    .await;
  let row = job_base::one(
    &bench.scene.db,
    r#"SELECT "execution_mode", "execution_upstream" FROM "jobs" WHERE "id" = $1"#,
    vec![job.into()],
  )
  .await
  .unwrap()
  .unwrap();
  let road = (
    row.try_get("", "execution_mode").unwrap(),
    row.try_get("", "execution_upstream").unwrap(),
  );
  let asked = bench.agent.asked().remove(0);

  Seen {
    bench,
    ran,
    place,
    asked,
    road,
  }
}

// What every branch's turn is: a seat of the branch's own that hears no words, nothing of the worker's sessions
// touched, and no bench made for a name that lives one task.
fn thrown_away(seen: &Seen) {
  assert!(seen.ran.ended(JOB_WORK_STEP, true), "{:?}", seen.ran);
  assert_eq!(
    seen.asked.seat,
    TurnSeat::Branch {
      todo: Some(TODO),
      role: Some("scout".into())
    }
  );
  assert_eq!(seen.asked.agent, format!("work-order-{TODO}-stage-0-0-1"));
  assert_eq!(seen.asked.cwd, seen.place.display().to_string());
  assert!(!seen.asked.hears_words);
  assert!(seen.bench.agent.kept_asked().is_empty());
  assert!(seen.bench.agent.handed().is_empty());
  assert!(!seen.bench.agent.upkeep().iter().any(|call| call == "make_bench"));
}

fn claude(session: &str) -> Reply {
  reply(session, ANSWER)
}

#[tokio::test]
async fn on_claude_code_a_branch_runs_in_its_standing_checkout_s_place_on_a_session_it_throws_away() {
  let seen =
    side("claude-code", "claude-haiku-4-5-20251001", claude("branch-session"), Where::Standing).await;

  thrown_away(&seen);
  assert_eq!(seen.asked.provider, "claude-code");
  assert!(seen.place.ends_with("branch-agents/pool/1"));
}

#[tokio::test]
async fn on_claude_code_a_branch_outside_every_standing_checkout_runs_in_the_folder_it_was_given() {
  let seen = side("claude-code", "claude-haiku-4-5-20251001", claude("loose-session"), Where::Loose).await;

  thrown_away(&seen);
  assert!(seen.place.ends_with("loose-branch"));
}

#[tokio::test]
async fn on_codex_a_branch_runs_once_and_keeps_no_thread() {
  let answer = Reply {
    mode: "codex".into(),
    model: Some("gpt-5.6-terra".into()),
    session_id: None,
    spend: vec![],
    ..reply("unused", ANSWER)
  };
  let seen = side("codex", "gpt-5.6-terra", answer, Where::Standing).await;

  thrown_away(&seen);
  assert_eq!(seen.asked.provider, "codex");
  assert_eq!(seen.road, (Some("codex".into()), None));
  assert_eq!(seen.bench.agent.asked().len(), 1);
}

#[tokio::test]
async fn on_openrouter_a_branch_runs_once_with_the_upstream_that_took_it_written_down() {
  let answer = Reply {
    mode: "openrouter".into(),
    model: Some("anthropic/claude-sonnet-5".into()),
    session_id: None,
    spend: vec![],
    upstream: Some("Anthropic".into()),
    ..reply("unused", ANSWER)
  };
  let seen = side("openrouter", "anthropic/claude-sonnet-5", answer, Where::Standing).await;

  thrown_away(&seen);
  assert_eq!(seen.asked.provider, "openrouter");
  assert_eq!(seen.road, (Some("openrouter".into()), Some("Anthropic".into())));
}
