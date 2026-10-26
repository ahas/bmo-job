//! `bmo-job serve` as a unit runs it: it reads its settings file, refuses one it cannot stand on, serves behind
//! the tokens, records an approval as its caller's and of the kind the settings give that caller, and stops on a
//! termination signal even with a stream open (docs/settings.md, docs/cli.md).

mod support;

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use bmo_job::Settings;
use bmo_job_client::{ApprovalGiven, ApprovalKind, ItemChange, ItemState, JobClient, NewItem};
use tokio::process::Command;

use support::ROOT;

/// How long a test lets the service take to come up or go down before it fails; a guard, never a wait.
const PATIENCE: Duration = Duration::from_secs(30);

fn written(dir: &Path, name: &str, text: &str) -> std::path::PathBuf {
  let path = dir.join(name);
  std::fs::write(&path, text).expect("a scratch file is written");
  path
}

#[test]
fn the_example_loads_and_a_wrong_file_is_refused() {
  let example = Settings::load(&Path::new(ROOT).join("settings.example.toml")).expect("the example loads");
  assert!(example.database.url.starts_with("sqlite:"));

  let dir = tempfile::tempdir().expect("a scratch directory");
  let unknown = written(
    dir.path(),
    "unknown.toml",
    "[server]\nlisten = \"x\"\ntoken_file = \"t\"\ncolour = 1\n[database]\nurl = \"sqlite://a\"\n",
  );
  assert!(Settings::load(&unknown).expect_err("an unknown key").contains("colour"));
  let other = written(
    dir.path(),
    "other.toml",
    "[server]\nlisten = \"x\"\ntoken_file = \"t\"\n[database]\nurl = \"postgres://a\"\n",
  );
  assert!(Settings::load(&other).expect_err("not SQLite").contains("SQLite"));

  let empty = written(dir.path(), "empty-token", " \n");
  let settings = format!(
    "[server]\nlisten = \"x\"\ntoken_file = \"{}\"\n[database]\nurl = \"sqlite://a\"\n",
    empty.display()
  );
  let settings = Settings::load(&written(dir.path(), "empty.toml", &settings)).expect("loads");
  assert!(settings.token().expect_err("no token").contains("no token"));
}

#[test]
fn the_unattended_labels_are_read_and_one_naming_no_key_is_refused() {
  let example = Settings::load(&Path::new(ROOT).join("settings.example.toml")).expect("the example loads");
  assert_eq!(example.unattended.labels, ["pr-review"]);

  let dir = tempfile::tempdir().expect("a scratch directory");
  let settings = |unattended: &str| {
    format!("[server]\nlisten = \"x\"\ntoken_file = \"t\"\n{unattended}[database]\nurl = \"sqlite://a\"\n")
  };
  let none = Settings::load(&written(dir.path(), "none.toml", &settings(""))).expect("loads");
  assert!(none.unattended.labels.is_empty(), "left out, no label is unattended");
  for (name, label) in [("empty.toml", ""), ("keyless.toml", "=fast")] {
    let refused = written(dir.path(), name, &settings(&format!("[unattended]\nlabels = [\"{label}\"]\n")));
    assert!(Settings::load(&refused).expect_err("a label naming no key").contains("names no key"));
  }
  let unknown = written(dir.path(), "unknown.toml", &settings("[unattended]\nqueues = []\n"));
  assert!(Settings::load(&unknown).expect_err("an unknown key").contains("queues"));
}

#[test]
fn the_callers_are_read_and_a_token_names_one_caller() {
  let example = Settings::load(&Path::new(ROOT).join("settings.example.toml")).expect("the example loads");
  let approvers: Vec<&str> = example
    .callers
    .iter()
    .filter(|(_, caller)| caller.approves)
    .map(|(name, _)| name.as_str())
    .collect();
  assert_eq!(approvers, ["qa-admin"]);
  assert!(!example.callers["tester"].approves, "a caller left unmarked is no approver");

  let dir = tempfile::tempdir().expect("a scratch directory");
  let shared = written(dir.path(), "shared", "shared-token\n");
  let own = written(dir.path(), "own", "own-token\n");
  let settings = |caller: &str| {
    format!(
      "[server]\nlisten = \"x\"\ntoken_file = \"{}\"\n{caller}[database]\nurl = \"sqlite://a\"\n",
      shared.display()
    )
  };

  let taken = format!("[callers.tester]\ntoken_file = \"{}\"\n", shared.display());
  let taken = Settings::load(&written(dir.path(), "taken.toml", &settings(&taken))).expect("loads");
  assert!(taken.access().expect_err("the shared token again").contains("another caller"));

  let misnamed = format!("[callers.\"two words\"]\ntoken_file = \"{}\"\n", own.display());
  let misnamed = written(dir.path(), "misnamed.toml", &settings(&misnamed));
  assert!(Settings::load(&misnamed).expect_err("a name with a space").contains("caller name"));

  let unknown = format!("[callers.tester]\ntoken_file = \"{}\"\nowns = true\n", own.display());
  let unknown = written(dir.path(), "unknown.toml", &settings(&unknown));
  assert!(Settings::load(&unknown).expect_err("an unknown key").contains("owns"));

  let approver = format!("[callers.qa-admin]\ntoken_file = \"{}\"\napproves = true\n", own.display());
  let approver =
    Settings::load(&written(dir.path(), "approver.toml", &settings(&approver))).expect("loads");
  let access = approver.access().expect("the tokens differ");
  let caller = access.caller("own-token").expect("the caller's token");
  assert_eq!((caller.name.as_deref(), caller.approves), (Some("qa-admin"), true));
  let shared = access.caller("shared-token").expect("the shared token");
  assert_eq!((shared.name.as_deref(), shared.approves), (None, false));
  assert!(access.caller("another-token").is_none());
}

#[tokio::test]
async fn the_service_serves_by_its_settings_and_stops_on_a_signal() {
  let dir = tempfile::tempdir().expect("a scratch directory");
  let port = std::net::TcpListener::bind("127.0.0.1:0")
    .expect("a free port")
    .local_addr()
    .expect("an address")
    .port();
  let token = written(dir.path(), "token", "served-token\n");
  let approver = written(dir.path(), "approver", "approver-token\n");
  let caller = written(dir.path(), "caller", "caller-token\n");
  let settings = format!(
    "[server]\nlisten = \"127.0.0.1:{port}\"\ntoken_file = \"{}\"\n\
     [callers.qa-admin]\ntoken_file = \"{}\"\napproves = true\n\
     [callers.tester]\ntoken_file = \"{}\"\n\
     [database]\nurl = \"sqlite://{}?mode=rwc\"\n",
    token.display(),
    approver.display(),
    caller.display(),
    dir.path().join("job.db").display()
  );
  let settings = written(dir.path(), "settings.toml", &settings);
  let mut child = Command::new(env!("CARGO_BIN_EXE_bmo-job"))
    .arg("serve")
    .env("BMO_JOB_SETTINGS", &settings)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .kill_on_drop(true)
    .spawn()
    .expect("the service starts");

  let client = JobClient::new(&format!("http://127.0.0.1:{port}"), "served-token").expect("a client");
  let ready = tokio::time::timeout(PATIENCE, async {
    loop {
      if let Ok(health) = client.health().await {
        return health;
      }
      tokio::time::sleep(Duration::from_millis(20)).await;
    }
  })
  .await
  .expect("the service comes up");
  assert!(ready.ready);

  // Whose approval opens a draft is the settings' word: the caller marked an approver opens it, another
  // caller's is a recommendation.
  let url = format!("http://127.0.0.1:{port}");
  let item = client
    .create_item(&NewItem {
      title: "served".to_owned(),
      queue_name: "build".to_owned(),
      ..NewItem::default()
    })
    .await
    .expect("raised");
  let approval = ItemChange {
    approval: Some(ApprovalGiven::default()),
    ..ItemChange::default()
  };
  let by_caller = JobClient::new(&url, "caller-token").expect("a client");
  let recommended = by_caller.change_item(item.id, &approval).await.expect("recorded");
  assert_eq!(recommended.state, ItemState::Draft);
  assert_eq!(
    (recommended.approvals[0].kind, recommended.approvals[0].caller_name.as_deref()),
    (ApprovalKind::Recommendation, Some("tester"))
  );
  let by_approver = JobClient::new(&url, "approver-token").expect("a client");
  let approved = by_approver.change_item(item.id, &approval).await.expect("approved");
  assert_eq!(approved.state, ItemState::Open);
  assert_eq!(
    (approved.approvals[1].kind, approved.approvals[1].caller_name.as_deref()),
    (ApprovalKind::Approval, Some("qa-admin"))
  );

  let stream = client
    .event_stream(&bmo_job_client::EventQuery::default(), None)
    .await
    .expect("a stream opens");
  let pid = i32::try_from(child.id().expect("the service runs")).expect("a pid");
  // The signal goes to the one process this test started.
  assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);
  let status = tokio::time::timeout(PATIENCE, child.wait())
    .await
    .expect("the service stops")
    .expect("an exit");
  assert!(status.success(), "the service stops cleanly: {status}");
  drop(stream);
}
