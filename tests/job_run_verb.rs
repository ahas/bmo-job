//! The `job-run` verb as a job's unit runs it (docs/runs.md, "What the runner is handed"): the binary started
//! on the runner's words over the settings, its ground built against the stand-ins of the units, the job
//! run to its end and the verb ending with the run's code; and words that name no job refused before
//! anything is held.

mod support;

use std::path::Path;
use std::process::Output;

use job_base::{Settings, one, run};
use migration::{Migrator, MigratorTrait};
use serde_json::json;
use support::server::{DONE, host_text, settings_text};
use support::{agency, gate, receiver};

/// The owner's chat the test settings give.
const OWNER_CHAT: i64 = 100;

async fn job_run(settings: &Path, words: &[&str]) -> Output {
  tokio::process::Command::new(env!("CARGO_BIN_EXE_bmo-job"))
    .env("BMO_JOB_SETTINGS", settings)
    .arg("job-run")
    .args(words)
    .output()
    .await
    .expect("the binary runs")
}

#[tokio::test]
async fn a_job_whose_worker_stands_on_a_node_with_no_road_ends_as_an_error_with_its_row_closed() {
  let scratch = tempfile::tempdir().unwrap();
  let (agency, gate, receiver) =
    (agency::Stub::start().await, gate::Stub::start().await, receiver::Stub::start().await);
  let url = format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display());
  let text = settings_text(&url, &agency.address(), &gate.address(), &receiver.address())
    + &host_text(scratch.path(), Path::new(DONE));
  let place = scratch.path().join("settings.toml");
  let prompt = scratch.path().join("prompt.txt");

  std::fs::write(&place, &text).unwrap();
  std::fs::write(&prompt, "Look at the logs.").unwrap();

  let settings = Settings::read(&text).unwrap();
  let db = settings.database.url.connect(1).await.unwrap();

  Migrator::up(&db, None).await.unwrap();
  run(
    &db,
    r#"INSERT INTO "workers" ("name", "label", "cwd", "builtin", "support_count", "node", "created_at")
       VALUES ('bench', 'bench', '/srv/bench', 0, 0, 'n1', 1)"#,
    vec![],
  )
  .await
  .unwrap();
  run(
    &db,
    r#"INSERT INTO "jobs" ("id", "chat_id", "unit", "launched_at", "prompt", "worker") VALUES (7, $1, 'bmo-task-7', 1, 'Look at the logs.', 'bench')"#,
    vec![OWNER_CHAT.into()],
  )
  .await
  .unwrap();

  // bmo-agency knows no SSH road to the node, and bmo-receiver takes what the agent is told.
  agency.answer(
    "GET /nodes/{name}/reach",
    404,
    json!({ "error": "There is no such node.", "code": "nothing-held" }),
  );
  receiver.answer("POST /instructions", 201, json!({ "id": 1 }));

  let prompt = prompt.display().to_string();
  let ran =
    job_run(&place, &["--", "100", &prompt, "7", "auto", "bmo-task-7", "bench", "/srv/bench"]).await;
  let said = String::from_utf8_lossy(&ran.stderr);

  assert_eq!(ran.status.code(), Some(1), "{said}");
  assert!(said.contains("there is no SSH road to the node n1 that bench stands on"), "{said}");
  assert!(
    agency.calls().iter().any(|call| call.target == "/nodes/n1/reach"),
    "the run asked bmo-agency for the node's road: {:?}",
    agency.calls()
  );

  let row = one(&db, r#"SELECT "finished_at", "outcome" FROM "jobs" WHERE "id" = 7"#, vec![])
    .await
    .unwrap()
    .unwrap();

  assert!(
    row.try_get::<Option<i64>>("", "finished_at").unwrap().is_some(),
    "the job's row was closed"
  );
  assert_eq!(row.try_get::<Option<String>>("", "outcome").unwrap().as_deref(), Some("error"));
}

#[tokio::test]
async fn words_that_name_no_job_are_refused_with_the_usage() {
  let scratch = tempfile::tempdir().unwrap();
  let url = format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display());
  let unit = url::Url::parse("http://127.0.0.1:1/").unwrap();
  let place = scratch.path().join("settings.toml");

  std::fs::write(
    &place,
    settings_text(&url, &unit, &unit, &unit) + &host_text(scratch.path(), Path::new(DONE)),
  )
  .unwrap();

  let ran = job_run(&place, &["--", "100"]).await;
  let said = String::from_utf8_lossy(&ran.stderr);

  assert_eq!(ran.status.code(), Some(2), "{said}");
  assert!(said.contains("usage: job-run --"), "{said}");
}
