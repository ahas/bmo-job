//! The binary `bmo-job` as a unit runs it (docs/api.md, "Routes"; docs/settings.md): it reads the settings
//! file the variable names, migrates its database up, declares itself a holder to bmo-receiver before it
//! serves, and serves its routes on the service's verbs.

mod support;

use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};

use serde_json::json;
use support::server::{DONE, code, host_text, settings_text};
use support::{agency, gate, receiver};

/// The running binary, taken down with the test.
struct Running(Child);

impl Drop for Running {
  fn drop(&mut self) {
    let _ = self.0.kill();
    let _ = self.0.wait();
  }
}

#[tokio::test]
async fn the_binary_migrates_declares_itself_a_holder_and_serves() {
  let scratch = tempfile::tempdir().unwrap();
  let (agency, gate, receiver) =
    (agency::Stub::start().await, gate::Stub::start().await, receiver::Stub::start().await);

  receiver.answer_empty("PUT /room-holders/{unit}", 204);
  receiver.answer("GET /owner", 200, json!({ "sender": 5, "chat": 100 }));

  let url = format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display());
  let settings = settings_text(&url, &agency.address(), &gate.address(), &receiver.address())
    + &host_text(scratch.path(), std::path::Path::new(DONE));
  let place = scratch.path().join("settings.toml");

  std::fs::write(&place, settings).unwrap();

  let mut child = Command::new(env!("CARGO_BIN_EXE_bmo-job"))
    .env("BMO_JOB_SETTINGS", &place)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .spawn()
    .expect("the binary starts");
  let said = child.stderr.take().unwrap();
  let running = Running(child);
  // The work-order runner beside the server may speak first, of a unit its tick could not reach; what the
  // binary says goes on being read, so its error stream never closes under it.
  let (serving, heard) = std::sync::mpsc::channel();

  std::thread::spawn(move || {
    for line in BufReader::new(said).lines().map_while(Result::ok) {
      if line.starts_with("bmo-job: serving on ") {
        let _ = serving.send(line);
      }
    }
  });

  let line = tokio::task::spawn_blocking(move || heard.recv().unwrap_or_default()).await.unwrap();
  let address = line
    .trim()
    .strip_prefix("bmo-job: serving on ")
    .unwrap_or_else(|| panic!("the binary said: {line}"));
  let http = reqwest::Client::new();
  let health = http.get(format!("http://{address}/health")).send().await.unwrap();

  assert_eq!(health.status().as_u16(), 200);
  assert_eq!(health.json::<serde_json::Value>().await.unwrap()["version"], env!("CARGO_PKG_VERSION"));

  let refused = http
    .post(format!("http://{address}/todo"))
    .json(&json!({ "action": "top", "id": 1 }))
    .send()
    .await
    .unwrap();

  assert_eq!(refused.status().as_u16(), 404, "the TODO's verb is carried");
  assert_eq!(code(&refused.json().await.unwrap()), "no_such_item");

  let holds = http.get(format!("http://{address}/queue/holds")).send().await.unwrap();

  assert_eq!(holds.status().as_u16(), 200, "the database was migrated up");
  let declared = json!({
    "address": "http://127.0.0.1:1/",
    "holdings": [
      { "name": "items", "effect": "kept" }, { "name": "questions", "effect": "kept" },
      { "name": "open_items", "effect": "warns" }, { "name": "waiting_questions", "effect": "warns" },
    ],
  });

  assert!(
    receiver
      .calls()
      .iter()
      .any(|call| call.target == "/room-holders/job" && call.body.as_ref() == Some(&declared)),
    "the service declared itself a holder: {:?}",
    receiver.calls()
  );

  drop(running);
}

#[test]
fn the_binary_refuses_to_start_without_its_server_section() {
  let scratch = tempfile::tempdir().unwrap();
  let place = scratch.path().join("settings.toml");
  let url = format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display());

  std::fs::write(
    &place,
    format!(
      "[database]\nurl = \"{url}\"\n\n[deployment]\nowner_name = \"Owner\"\nagent_name = \"Agent\"\nagent_key = \"agent\"\nsender_name = \"Agent\"\nreceiver_unit = \"receiver\"\nowner_chat = 100\n"
    ),
  )
  .unwrap();

  let ran = Command::new(env!("CARGO_BIN_EXE_bmo-job"))
    .env("BMO_JOB_SETTINGS", &place)
    .output()
    .unwrap();

  assert!(!ran.status.success());
  assert!(
    String::from_utf8_lossy(&ran.stderr).contains("no [server] section"),
    "{}",
    String::from_utf8_lossy(&ran.stderr)
  );
}
