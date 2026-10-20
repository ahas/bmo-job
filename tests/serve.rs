//! `bmo-job serve` as a unit runs it: it reads its settings file, refuses one it cannot stand on, serves behind
//! the token, and stops on a termination signal even with a stream open (docs/settings.md, docs/cli.md).

mod support;

use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use bmo_job::Settings;
use bmo_job_client::JobClient;
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

#[tokio::test]
async fn the_service_serves_by_its_settings_and_stops_on_a_signal() {
  let dir = tempfile::tempdir().expect("a scratch directory");
  let port = std::net::TcpListener::bind("127.0.0.1:0")
    .expect("a free port")
    .local_addr()
    .expect("an address")
    .port();
  let token = written(dir.path(), "token", "served-token\n");
  let settings = format!(
    "[server]\nlisten = \"127.0.0.1:{port}\"\ntoken_file = \"{}\"\n[database]\nurl = \"sqlite://{}?mode=rwc\"\n",
    token.display(),
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
