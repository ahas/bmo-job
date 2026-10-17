//! A control step's design and the shape it is asked for after it (docs/runs.md, "The turns"): the design
//! said in the step's own words and never parsed, the shape asked of the session that wrote it, a refused
//! shape asked again with the validator's words and each refused answer kept under the job's documents, and a
//! format turn on the design's own session sent the request alone.

use std::path::PathBuf;

use bmo_job::job_run::{WORK_SESSION_RESUME, WORK_STEP_PHASE};
use bmo_job::queue::outcomes::JOB_WORK_STEP;
use job_agency::Reply;

use crate::support::job::{Bench, Given, Ran, WORKER};
use crate::support::turns::{DESIGN, PLAN, Played, reply};

const SHAPE: &str = "[Shape the answer]";
const CORRECTION: &str = "[JSON correction]";

// A plan whose two outer delimiters are missing, as a recorded failure had it.
fn unclosed_plan() -> &'static str {
  PLAN.strip_suffix("]}").expect("the plan closes its stages")
}

// A middle step on a worker in a checkout of its own, answering in the structure given, over the turns; where
// its refused shapes are kept.
async fn shaped(structure: &str, model: &str, plays: Vec<Played>) -> (Bench, Ran, PathBuf) {
  let mut bench = Bench::new().await;

  bench.roster();

  let dir = bench.checkout("checkout");

  bench.worker(WORKER, &dir, 0).await;

  let job = bench.job(None, WORKER, "bmo-task-timed").await;
  let given = Given {
    model: model.into(),
    structure: structure.into(),
    ..bench.step(job, &dir, WORK_STEP_PHASE, WORK_SESSION_RESUME)
  };

  bench.agent.play(plays);

  let ran = bench.run(&given).await;
  let kept = bench.at("documents").join("jobs").join(format!("work-output-{job}"));

  (bench, ran, kept)
}

#[tokio::test]
async fn a_planning_step_reports_its_design_in_its_own_words_and_is_asked_again_for_the_shape() {
  let (bench, ran, kept) = shaped(
    "plan",
    "claude-opus-5",
    vec![Played::answered(reply("s1", DESIGN)), Played::answered(reply("s1", PLAN))],
  )
  .await;
  let prompts = bench.agent.prompts();

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  assert_eq!(ran.report, PLAN);
  assert!(prompts[1].contains(SHAPE), "{}", prompts[1]);
  assert!(prompts[1].contains(DESIGN), "{}", prompts[1]);
  assert!(!prompts.iter().any(|prompt| prompt.contains(CORRECTION)), "{prompts:?}");
  assert!(!kept.exists());
}

#[tokio::test]
async fn a_synthesis_step_is_shaped_the_same_way_after_it_has_said_what_there_is_to_build() {
  let tasks = r#"{"tasks":[{"instruction":"Put in the format turn and add a regression test for it"}]}"#;
  let (bench, ran, _) = shaped(
    "synthesis",
    "claude-opus-5",
    vec![Played::answered(reply("s1", DESIGN)), Played::answered(reply("s1", tasks))],
  )
  .await;
  let prompts = bench.agent.prompts();

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  assert_eq!(ran.report, tasks);
  assert!(prompts[1].contains(SHAPE), "{}", prompts[1]);
  assert!(prompts[1].contains(DESIGN), "{}", prompts[1]);
  assert!(!prompts.iter().any(|prompt| prompt.contains(r#"{"stages""#)), "{prompts:?}");
}

#[tokio::test]
async fn a_malformed_shaped_plan_is_corrected_and_validated_before_the_step_succeeds() {
  let (bench, ran, kept) = shaped(
    "plan",
    "claude-opus-5",
    vec![
      Played::answered(reply("s1", DESIGN)),
      Played::answered(reply("s1", unclosed_plan())),
      Played::answered(reply("s2", PLAN)),
    ],
  )
  .await;

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  assert!(bench.agent.prompts()[2].contains(CORRECTION), "{}", bench.agent.prompts()[2]);
  assert_eq!(std::fs::read_to_string(kept.join("0.json")).unwrap(), unclosed_plan());
  assert_eq!(std::fs::read_to_string(kept.join("design.txt")).unwrap(), DESIGN);
}

#[tokio::test]
async fn three_invalid_shaped_replies_leave_the_step_failed_rather_than_reported_as_succeeding() {
  let (_bench, ran, kept) = shaped(
    "plan",
    "claude-opus-5",
    vec![Played::answered(reply("s1", DESIGN)), Played::answered(reply("s1", r#"{"stages":["#))],
  )
  .await;

  assert!(!ran.success && ran.ok == Some(0), "{ran:?}");
  assert_eq!(std::fs::read_to_string(kept.join("2.json")).unwrap(), r#"{"stages":["#);
  assert_eq!(std::fs::read_to_string(kept.join("design.txt")).unwrap(), DESIGN);
}

// A Codex answer on the thread it keeps.
fn codex(thread: &str, text: &str) -> Reply {
  Reply {
    mode: "codex".into(),
    model: Some("gpt-5.6-terra".into()),
    spend: vec![],
    ..reply(thread, text)
  }
}

// The shape is asked of Codex on the design's own thread, which is sent the request alone.
#[tokio::test]
async fn a_codex_format_turn_on_the_design_s_thread_is_not_sent_the_job_again() {
  let (bench, ran, _) = shaped(
    "plan",
    "codex::gpt-5.6-terra",
    vec![Played::answered(codex("thread-1", DESIGN)), Played::answered(codex("thread-1", PLAN))],
  )
  .await;
  let prompts = bench.agent.prompts();
  let task = "There is one thing to do in this repository.";

  assert!(ran.ended(JOB_WORK_STEP, true), "{ran:?}");
  assert_eq!(ran.report, PLAN);
  assert_eq!(bench.agent.asked()[1].provider, "codex");
  assert!(prompts[0].contains(task), "{}", prompts[0]);
  assert!(prompts[1].contains(SHAPE), "{}", prompts[1]);
  assert!(!prompts[1].contains(task), "{}", prompts[1]);
}
