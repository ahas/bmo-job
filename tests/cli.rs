//! The command line as `bmo` hands it its words (docs/cli.md): the real binary run on a settings file whose
//! server is a real server on the ported verbs, over that server's scratch database and the stand-ins of
//! the units, one test a noun, each with what the noun does and what it refuses.

mod support;

use std::path::{Path, PathBuf};
use std::process::Output;

use bmo_job::cli::NOUNS;
use bmo_job::verbs::Built;
use serde_json::{Value, json};
use support::server::{Served, host_text, settings_text};

/// The owner's chat the test settings give.
const OWNER_CHAT: i64 = 100;

/// The variables a job's process and an agent's turn carry, which no test's process may hand the binary.
const TURN_VARIABLES: [&str; 5] =
  ["CLAUDECODE", "BMO_TASK_LOCKED", "BMO_RUN_NAME", "BMO_WORK_ORDER_STEP", "BMO_TURN_AGENT"];

/// A server on the ported verbs, and the settings file the command line reads to reach it.
struct World {
  served: Served<Built>,
  settings: PathBuf,
}

async fn world() -> World {
  let served = Served::<Built>::ported().await;
  let scratch = served.scratch.path();
  let url = format!("sqlite://{}?mode=rwc", scratch.join("job.db").display());
  let text =
    (settings_text(&url, &served.agency.address(), &served.gate.address(), &served.receiver.address())
      + &host_text(scratch, &served.testbed.program()))
      .replace("address = \"http://127.0.0.1:1/\"", &format!("address = \"{}\"", served.base));
  let settings = scratch.join("cli-settings.toml");

  std::fs::write(&settings, text).unwrap();
  served.agency.answer("GET /providers", 200, json!([]));
  served.agency.answer_empty("POST /workers/{worker}/bench", 204);
  served.receiver.answer("GET /owner", 200, json!({ "sender": 5, "chat": OWNER_CHAT }));
  World { served, settings }
}

impl World {
  async fn run(&self, words: &[&str]) -> Output {
    self.run_in(self.served.scratch.path(), words).await
  }

  async fn run_in(&self, here: &Path, words: &[&str]) -> Output {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_bmo-job"));

    for variable in TURN_VARIABLES {
      command.env_remove(variable);
    }

    command
      .env("BMO_JOB_SETTINGS", &self.settings)
      .env("USER", "person")
      .current_dir(here)
      .args(words)
      .output()
      .await
      .expect("the binary runs")
  }

  async fn item(&self, id: i64) {
    self
      .served
      .seed(
        r#"INSERT INTO "todos" ("id", "title", "instruction", "requester", "chat_id", "approved", "priority", "status", "created_at")
           VALUES ($1, 'Fix the reader', 'What to do.', 'Owner', $2, 1, $1, 'waiting', 1)"#,
        vec![id.into(), OWNER_CHAT.into()],
      )
      .await;
  }
}

fn out(ran: &Output) -> String {
  String::from_utf8_lossy(&ran.stdout).into_owned()
}

fn said(ran: &Output) -> String {
  String::from_utf8_lossy(&ran.stderr).into_owned()
}

fn json_lines(ran: &Output) -> Vec<Value> {
  out(ran)
    .lines()
    .map(|line| serde_json::from_str(line).unwrap_or_else(|_| panic!("not JSON: {line}")))
    .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_nouns_are_printed_one_a_line_and_no_noun_is_refused_with_the_usage() {
  let world = world().await;
  let ran = world.run(&["nouns"]).await;

  assert_eq!(ran.status.code(), Some(0), "{}", said(&ran));
  assert_eq!(out(&ran).lines().collect::<Vec<_>>(), NOUNS);
  assert!(NOUNS.contains(&"checkout setup") && NOUNS.contains(&"room jobs"));

  let ran = world.run(&[]).await;

  assert_eq!(ran.status.code(), Some(2));
  assert!(said(&ran).contains("Usage: bmo-job"), "{}", said(&ran));
}

#[tokio::test(flavor = "multi_thread")]
async fn job_reads_what_a_job_cost_and_refuses_a_landing_off_a_job_s_branch() {
  let world = world().await;

  world.item(3).await;

  let ran = world.run(&["job", "figures", "--json", "3"]).await;

  assert_eq!(ran.status.code(), Some(0), "{}", said(&ran));
  assert!(serde_json::from_str::<Value>(&out(&ran)).is_ok(), "{}", out(&ran));

  let ran = world.run(&["job", "land"]).await;

  assert_eq!(ran.status.code(), Some(1), "{}", out(&ran));
  assert!(!said(&ran).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn todo_sets_an_item_s_place_and_refuses_an_item_that_is_none() {
  let world = world().await;

  world.item(3).await;

  let ran = world.run(&["todo", "--priority", "3", "7"]).await;

  assert_eq!(ran.status.code(), Some(0), "{}", said(&ran));
  assert_eq!(json_lines(&ran)[0]["code"], "priority_set");
  assert_eq!(json_lines(&ran)[0]["priority"], 7);

  let ran = world.run(&["todo", "--priority", "99", "7"]).await;

  assert_eq!(ran.status.code(), Some(1));
  assert!(said(&ran).contains("99"), "{}", said(&ran));
}

#[tokio::test(flavor = "multi_thread")]
async fn work_order_reads_an_item_s_draft_and_refuses_an_item_that_is_no_number() {
  let world = world().await;

  world.item(3).await;

  let ran = world.run(&["work-order", "--todo", "3", "--json"]).await;

  assert_eq!(ran.status.code(), Some(0), "{}", said(&ran));
  assert_eq!(serde_json::from_str::<Value>(&out(&ran)).unwrap()["status"], "draft");

  let ran = world.run(&["work-order", "--todo", "three"]).await;

  assert_eq!(ran.status.code(), Some(2));
  assert!(out(&ran).contains("--todo needs one item number"), "{}", out(&ran));
}

#[tokio::test(flavor = "multi_thread")]
async fn worker_adds_and_shows_a_worker_through_the_server_and_refuses_to_remove_the_built_in_one() {
  let world = world().await;
  let ran = world.run(&["worker", "add", "bench", "--label", "Bench"]).await;

  assert_eq!(ran.status.code(), Some(0), "{}", said(&ran));
  assert_eq!(json_lines(&ran)[0]["name"], "bench");

  let ran = world.run(&["worker", "show", "bench"]).await;

  assert_eq!(ran.status.code(), Some(0), "{}", said(&ran));
  assert_eq!(json_lines(&ran)[0]["label"], "Bench");

  let ran = world.run(&["worker", "list"]).await;

  assert!(json_lines(&ran).iter().any(|worker| worker["name"] == "bench"), "{}", out(&ran));

  let ran = world.run(&["worker", "remove", "worker"]).await;

  assert_eq!(ran.status.code(), Some(1));
  assert!(said(&ran).starts_with("worker: "), "{}", said(&ran));
}

#[tokio::test(flavor = "multi_thread")]
async fn overtime_lists_the_asks_and_refuses_an_ask_that_is_none() {
  let world = world().await;
  let ran = world.run(&["overtime", "--list"]).await;

  assert_eq!(ran.status.code(), Some(0), "{}", said(&ran));

  let ran = world.run(&["overtime", "--keep", "9"]).await;

  assert_eq!(ran.status.code(), Some(1));
  assert!(said(&ran).contains("no ask #9"), "{}", said(&ran));

  let ran = world.run(&["overtime", "--keep", "9", "--cut", "9"]).await;

  assert_eq!(ran.status.code(), Some(2));
}

#[tokio::test(flavor = "multi_thread")]
async fn checkout_setup_sets_up_a_plain_checkout_and_says_why_one_could_not_be() {
  let world = world().await;
  let repository = world.served.scratch.path().join("plain");

  std::fs::create_dir_all(&repository).unwrap();

  let init = std::process::Command::new("git")
    .args(["init", "-q"])
    .current_dir(&repository)
    .output()
    .unwrap();

  assert!(init.status.success());

  let ran = world.run_in(&repository, &["checkout", "setup"]).await;

  assert_eq!(ran.status.code(), Some(0), "{}{}", out(&ran), said(&ran));

  // A submodule pinned at an address that names no repository cannot be brought to its pin, which is said,
  // and the setup ends with 1.
  let missing = world.served.scratch.path().join("no-repository");
  let git = |args: &[&str]| {
    let ran = std::process::Command::new("git").args(args).current_dir(&repository).output().unwrap();

    assert!(ran.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ran.stderr));
  };

  std::fs::write(
    repository.join(".gitmodules"),
    format!("[submodule \"part\"]\n\tpath = part\n\turl = {}\n", missing.display()),
  )
  .unwrap();
  git(&[
    "update-index",
    "--add",
    "--cacheinfo",
    "160000,1111111111111111111111111111111111111111,part",
  ]);

  let ran = world.run_in(&repository, &["checkout", "setup"]).await;

  assert_eq!(ran.status.code(), Some(1), "{}{}", out(&ran), said(&ran));
  assert!(said(&ran).starts_with("checkout setup: "), "{}", said(&ran));
}

#[tokio::test(flavor = "multi_thread")]
async fn room_jobs_lists_a_room_s_jobs_newest_first_and_refuses_a_room_that_is_no_number() {
  let world = world().await;

  for unit in ["bmo-task-1", "bmo-task-2"] {
    world
      .served
      .seed(
        r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "worker") VALUES ($1, $2, 1, 'Do it.', 'worker')"#,
        vec![OWNER_CHAT.into(), unit.into()],
      )
      .await;
  }

  let ran = world.run(&["room", "jobs", "--chat", "100"]).await;
  let units: Vec<Value> = json_lines(&ran).into_iter().map(|job| job["unit"].clone()).collect();

  assert_eq!(ran.status.code(), Some(0), "{}", said(&ran));
  assert_eq!(units, [json!("bmo-task-2"), json!("bmo-task-1")]);

  let ran = world.run(&["room", "jobs", "--chat", "x"]).await;

  assert_eq!(ran.status.code(), Some(2));
}

#[tokio::test(flavor = "multi_thread")]
async fn node_land_refuses_a_worker_that_is_none() {
  let world = world().await;
  let ran = world.run(&["node-land", "--worker", "nobody"]).await;

  assert_eq!(ran.status.code(), Some(2));
  assert!(said(&ran).contains("nobody is no worker to land for"), "{}", said(&ran));
}
