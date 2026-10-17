//! The queue's ground built from the settings in one place (docs/queue.md, "Where the queue stands"): the
//! host a job's unit is started on, the telling's own unit and source, and a settings file missing a
//! section the queue needs builds none.

mod support;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use bmo_job::Settings;
use bmo_job::queue::ground::{GroundError, QueueGround};
use sea_orm::DatabaseConnection;
use support::queue::{Fake, policy};

const HOST: &str = r#"
[host]
source = "/srv/source"
testbed = "/srv/bin/testbed"
systemctl = "/usr/bin/systemctl"
systemd_run = "/usr/bin/systemd-run"
runner = "/srv/bin/bmo-job"
runner_directory = "/srv"
runner_path = "/usr/bin:/bin"
runner_environment = "/srv/job.env"
locks = "/srv/locks"
prompts = "/srv/prompts"
lock_wait_seconds = 7
launcher = "/srv/bin/launcher"
launcher_directory = "/srv/launch"
documents = "/srv/work-orders"
tree = "/srv/platform"
"#;

fn base() -> String {
  let url = url::Url::parse("http://127.0.0.1:1/").unwrap();

  support::server::settings_text("sqlite::memory:", &url, &url, &url)
}

#[test]
fn the_ground_takes_the_host_and_the_server_unit_from_the_settings() {
  let settings = Settings::read(&format!("{}{HOST}", base())).unwrap();
  let environment = BTreeMap::from([
    ("XDG_RUNTIME_DIR".to_owned(), "/run/user/7".to_owned()),
    ("DBUS_SESSION_BUS_ADDRESS".to_owned(), "disabled:".to_owned()),
  ]);
  let ground = QueueGround::of(&settings, environment).unwrap();
  let host = &ground.host;

  assert_eq!(host.program, Path::new("/srv/bin/bmo-job"));
  assert_eq!(host.cwd, Path::new("/srv"));
  assert_eq!(host.setenv, [("PATH".to_owned(), "/usr/bin:/bin".to_owned())]);
  assert_eq!(host.environment_file.as_deref(), Some(Path::new("/srv/job.env")));
  assert_eq!(
    (host.locks.as_path(), host.prompts.as_path()),
    (Path::new("/srv/locks"), Path::new("/srv/prompts"))
  );
  assert_eq!(host.systemd_run, Path::new("/usr/bin/systemd-run"));
  assert_eq!(host.lock_wait, Duration::from_secs(7));
  assert_eq!(host.env.get("XDG_RUNTIME_DIR").map(String::as_str), Some("/run/user/7"));
  assert!(
    !host.env.contains_key("DBUS_SESSION_BUS_ADDRESS"),
    "a bus a session disabled is taken away"
  );
  assert_eq!(ground.launcher, Path::new("/srv/bin/launcher"));
  assert_eq!(ground.launcher_directory, Path::new("/srv/launch"));
  assert_eq!(ground.units().agent_key, "agent");

  let db = DatabaseConnection::Disconnected;
  let fake = Fake::default();
  let policy = policy();
  let teller = ground.teller(&db, &fake, &policy, 100, None);

  assert_eq!(teller.source, Path::new("/srv/source"));
  assert_eq!(teller.service_unit, "job");
  assert_eq!(teller.systemctl.as_deref(), Some(Path::new("/usr/bin/systemctl")));
  assert_eq!(ground.running(&db, &fake).systemctl.as_deref(), Some(Path::new("/usr/bin/systemctl")));
  assert_eq!(ground.stops(&db, &fake, Duration::from_millis(5)).settle, Duration::from_millis(5));
}

#[test]
fn a_settings_file_missing_a_section_the_queue_needs_builds_no_ground() {
  let no_host = Settings::read(&base()).unwrap();
  let no_units = Settings::read(&format!("{}{HOST}", base().split("[units]").next().unwrap())).unwrap();
  let no_server = Settings::read(&format!(
    "{}{HOST}",
    base().replace(
      "[server]\nlisten = \"127.0.0.1:0\"\naddress = \"http://127.0.0.1:1/\"\nunit = \"job\"\n",
      ""
    )
  ))
  .unwrap();

  assert!(matches!(QueueGround::of(&no_host, BTreeMap::new()), Err(GroundError::NoHost)));
  assert!(matches!(QueueGround::of(&no_units, BTreeMap::new()), Err(GroundError::NoUnits)));
  assert!(matches!(QueueGround::of(&no_server, BTreeMap::new()), Err(GroundError::NoServer)));
}
