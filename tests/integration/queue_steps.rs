//! The job queue's own steps (docs/queue.md, "Starting a job" and "Stopping runs"): a stopped run's cancels taken
//! back through bmo-agency, a job's unit started in the first free checkout with its row and its text, a
//! manager systemd-run could not reach taking the row back, an item's prompt kept as its brief, and a runner's
//! launcher started on the owner's words — each against a database and a host of the test's own, the
//! programs the queue starts standing in.

use std::collections::BTreeMap;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::time::Duration;

use bmo_job::queue::cancels;
use bmo_job::queue::job_start::{self, Host, JobStart, Place, Started, hold, lock_file_name};
use bmo_job::queue::runners;
use bmo_job::queue::units::Ports;
use job_receiver::ReceiverClient;
use serde_json::json;

use crate::support::program::{program, set_in};
use crate::support::queue::{OWNER, Scene};
use crate::support::{agency, testbed};

/// A host of the test's own: its folders in a scratch directory, a runtime directory whose bus stands, and
/// systemd-run standing in, writing its words to `lines`.
struct Machine {
  scratch: tempfile::TempDir,
}

impl Machine {
  fn new() -> Self {
    let scratch = tempfile::tempdir().unwrap();

    for folder in ["locks", "prompts", "runtime", "runner", "launch"] {
      std::fs::create_dir_all(scratch.path().join(folder)).unwrap();
    }

    std::fs::write(scratch.path().join("runtime").join("bus"), "").unwrap();
    Self { scratch }
  }

  fn at(&self, name: &str) -> PathBuf {
    self.scratch.path().join(name)
  }

  // The host, systemd-run answering as the variables given set it.
  fn host(&self, stand_in: &[(&str, &str)]) -> Host {
    let mut env = BTreeMap::from([
      ("XDG_RUNTIME_DIR".to_owned(), self.at("runtime").display().to_string()),
      ("STAND_IN_LINES".to_owned(), self.at("lines").display().to_string()),
    ]);

    env.extend(stand_in.iter().map(|(name, value)| ((*name).to_owned(), (*value).to_owned())));

    Host {
      program: PathBuf::from("/srv/bin/bmo-job"),
      cwd: self.at("runner"),
      setenv: vec![("PATH".to_owned(), "/usr/bin:/bin".to_owned())],
      environment_file: None,
      locks: self.at("locks"),
      prompts: self.at("prompts"),
      env,
      systemd_run: program().to_path_buf(),
      lock_wait: Duration::from_secs(1),
    }
  }

  fn read(&self, name: &str) -> String {
    std::fs::read_to_string(self.at(name)).unwrap_or_default()
  }
}

fn place(name: &str) -> Place {
  Place {
    name: name.into(),
    cwd: format!("/checkouts/{name}"),
  }
}

fn start(places: Vec<Place>) -> JobStart {
  JobStart {
    chat_id: OWNER,
    prompt: "The work.".into(),
    model: None,
    started_by: Some(format!("chat:{OWNER}")),
    todo_id: None,
    purpose: Some("A trial.".into()),
    user: Some("7".into()),
    item_model: Some("opus".into()),
    places,
  }
}

async fn count(scene: &Scene, sql: &str) -> i64 {
  bmo_job::one(&scene.db, sql, vec![])
    .await
    .unwrap()
    .unwrap()
    .try_get_by_index::<i64>(0)
    .unwrap()
}

#[tokio::test]
async fn a_stopped_run_s_cancels_are_taken_back_through_the_agency_as_it_answers_them() {
  let agency = agency::Stub::start().await;
  let client = agency.client();
  let receiver = ReceiverClient::new(url::Url::parse("http://127.0.0.1:1/").unwrap()).unwrap();
  let web = job_web::WebClient::new(url::Url::parse("http://127.0.0.1:1/").unwrap()).unwrap();
  let gate = job_gate::GateClient::new(url::Url::parse("http://127.0.0.1:1/").unwrap()).unwrap();
  let stand = testbed::Stand::new();
  let testbed = stand.client();
  let units = Ports {
    agency: &client,
    receiver: &receiver,
    web: &web,
    testbed: &testbed,
    gate: &gate,
  };

  agency.answer("DELETE /executions/{id}/cancel", 200, json!({ "dropped": ["bmo-task-1", "child"] }));

  assert_eq!(cancels::take_back(&units, "bmo-task-1").await.unwrap(), ["bmo-task-1", "child"]);
  agency.took("DELETE /executions/{id}/cancel", "/executions/bmo-task-1/cancel", None);
}

#[tokio::test]
async fn a_job_starts_in_the_first_free_checkout_and_takes_its_row_back_where_systemd_reached_nothing() {
  let scene = Scene::new().await;
  let machine = Machine::new();
  let host = machine.host(&[]);
  let start = start(vec![place("held"), place("free")]);
  let held = hold(&host.locks.join(lock_file_name("/checkouts/held"))).await;
  let Started::Job {
    job_id,
    unit,
    landed,
  } = job_start::start(&scene.db, &host, &start).await.unwrap()
  else {
    panic!("the free checkout takes the job");
  };

  assert_eq!(landed, place("free"));

  let said = machine.read("lines");

  assert!(
    said.contains(&format!("--unit={unit}")) && said.contains("--setenv=BMO_TURN_USER=7"),
    "{said}"
  );
  assert!(
    said.trim_end().ends_with(&format!("{job_id} opus {unit} free /checkouts/free")),
    "{said}"
  );

  let prompt = host.prompts.join(format!("{unit}.txt"));

  assert_eq!(std::fs::read_to_string(&prompt).unwrap(), "The work.");
  assert_eq!(std::fs::metadata(&prompt).unwrap().permissions().mode() & 0o777, 0o600);
  assert_eq!(
    count(&scene, &format!(r#"SELECT COUNT(*) FROM "jobs" WHERE "id" = {job_id} AND "worker" = 'free' AND "finished_at" IS NULL"#)).await,
    1
  );

  // Every checkout's lock held starts nothing.
  let also = hold(&host.locks.join(lock_file_name("/checkouts/free"))).await;

  assert_eq!(job_start::start(&scene.db, &host, &start).await.unwrap(), Started::Held);
  drop((held, also));

  // A manager systemd-run could not reach leaves no row behind.
  let unreached =
    machine.host(&[("STAND_IN_SAY", "Failed to connect to bus: No medium found"), ("STAND_IN_EXIT", "1")]);
  let refused = job_start::start(&scene.db, &unreached, &start).await.unwrap_err();

  assert_eq!(refused.code, "manager_unreachable");
  assert!(refused.error.contains("Failed to connect to bus"), "{}", refused.error);
  assert_eq!(count(&scene, r#"SELECT COUNT(*) FROM "jobs""#).await, 1);

  // A unit systemd-run refused closes its row as an error.
  let refusing = machine.host(&[("STAND_IN_SAY", "Unit already exists"), ("STAND_IN_EXIT", "1")]);

  assert_eq!(
    job_start::start(&scene.db, &refusing, &start).await.unwrap_err().code,
    "unit_not_started"
  );
  assert_eq!(
    count(
      &scene,
      r#"SELECT COUNT(*) FROM "jobs" WHERE "outcome" = 'error' AND "finished_at" IS NOT NULL"#
    )
    .await,
    1
  );
}

#[tokio::test]
async fn a_launched_item_s_prompt_is_kept_as_its_brief_and_a_launch_that_started_nothing_keeps_none() {
  let scene = Scene::new().await;
  let machine = Machine::new();

  scene.item(5, "Launcher").await;
  scene
    .run(r#"UPDATE "todos" SET "brief_korean" = 'an old rendering' WHERE "id" = 5"#, vec![])
    .await;

  let kept = "Item #5 launcher\n\nWhat the work is\nFix the launcher.";
  let first = JobStart {
    todo_id: Some(5),
    purpose: None,
    prompt: format!("{kept}\n"),
    ..start(vec![place("free")])
  };
  let briefed = |brief: &str| {
    format!(
      r#"SELECT COUNT(*) FROM "todos" WHERE "id" = 5 AND "brief" = '{brief}' AND "brief_korean" IS NULL"#
    )
  };

  assert!(matches!(
    job_start::start(&scene.db, &machine.host(&[]), &first).await.unwrap(),
    Started::Job { .. }
  ));
  assert_eq!(count(&scene, &briefed(kept)).await, 1);

  // A manager systemd-run could not reach starts nothing, and the brief stays the one the last launch kept.
  scene.run(r#"UPDATE "jobs" SET "finished_at" = 2"#, vec![]).await;

  let unreached =
    machine.host(&[("STAND_IN_SAY", "Failed to connect to bus: No medium found"), ("STAND_IN_EXIT", "1")]);
  let second = JobStart {
    prompt: "Item #5 launcher\n\nA different brief.".into(),
    ..first
  };

  assert!(job_start::start(&scene.db, &unreached, &second).await.is_err());
  assert_eq!(count(&scene, &briefed(kept)).await, 1);
}

#[tokio::test]
async fn a_runner_s_launcher_takes_the_words_on_its_input_and_its_failure_is_said() {
  let scene = Scene::new().await;
  let machine = Machine::new();
  let launch = machine.at("launch");
  let research = &scene.policy.research;
  let recorded = |exit: &str| {
    set_in(
      &launch,
      &[
        ("STAND_IN_LINES", &machine.at("launched").display().to_string()),
        ("STAND_IN_INPUT", &machine.at("launched").display().to_string()),
        ("STAND_IN_EXIT", exit),
      ],
    );
  };
  let launched = |runner: &'static str, depth: Option<&'static str>, text: &'static str| {
    runners::launch(research, program(), &launch, OWNER, runner, depth, text)
  };

  recorded("0");
  launched("browse", None, "  Log in and check it ").await.unwrap();
  assert_eq!(
    machine.read("launched"),
    format!("browse --chat {OWNER} --by console\nLog in and check it")
  );

  std::fs::remove_file(machine.at("launched")).unwrap();
  launched("search", None, "find it").await.unwrap();
  assert_eq!(
    machine.read("launched"),
    format!("search --chat {OWNER} --depth basic --demand explicit --by console\nfind it")
  );

  recorded("3");

  let failed = launched("search", None, "find it").await.unwrap_err();

  assert_eq!(failed.code, "runner_not_launched");
  assert!(failed.error.contains("it exited with code 3"), "{}", failed.error);
  assert_eq!(launched("search", Some("deepest"), "x").await.unwrap_err().code, "runner_depth_wrong");
  assert_eq!(launched("nobody", None, "x").await.unwrap_err().code, "runner_unknown");
}
