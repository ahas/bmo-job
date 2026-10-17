//! The settings file loads, and the database it names takes the same statements on SQLite and on
//! PostgreSQL (docs/settings.md).

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use job_base::{Settings, SettingsError, all, now_ms, one, run};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Value};

const EXAMPLE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../settings.example.toml");

const DEPLOYMENT: &str = r#"
[deployment]
owner_name = "Owner"
agent_name = "Agent"
agent_key = "agent"
sender_name = "Agent"
receiver_unit = "receiver"
"#;

fn with_database(url: &str) -> String {
  format!("[database]\nurl = \"{url}\"\n{DEPLOYMENT}")
}

/// The machine's own PostgreSQL server, as every PostgreSQL test of the service reaches it.
fn test_postgres() -> String {
  std::env::var("BMO_TEST_POSTGRES")
    .unwrap_or_else(|_| "postgres:///bmo_test?host=/var/run/postgresql".into())
}

/// One write and two reads through the library's statements, numbered values bound at any width.
async fn probe(db: &DatabaseConnection) {
  run(db, "CREATE TABLE probe (id bigint PRIMARY KEY, name text NOT NULL)", vec![])
    .await
    .unwrap();

  let written = run(
    db,
    "INSERT INTO probe (id, name) VALUES ($1, $2), ($3, $4)",
    vec![Value::Int(Some(7)), "seven".into(), Value::BigInt(Some(8)), "eight".into()],
  )
  .await
  .unwrap();

  assert_eq!(written, 2);

  let row = one(db, "SELECT name FROM probe WHERE id = $1", vec![Value::SmallInt(Some(7))])
    .await
    .unwrap();

  assert_eq!(row.unwrap().try_get::<String>("", "name").unwrap(), "seven");
  assert!(
    one(db, "SELECT name FROM probe WHERE id = $1", vec![9i64.into()])
      .await
      .unwrap()
      .is_none()
  );

  let rows = all(db, "SELECT id FROM probe ORDER BY id", vec![]).await.unwrap();
  let ids: Vec<i64> = rows.iter().map(|row| row.try_get("", "id").unwrap()).collect();

  assert_eq!(ids, [7, 8]);
}

#[test]
fn the_checked_in_example_loads_with_neutral_names() {
  let settings = Settings::load(Path::new(EXAMPLE)).expect("the example loads");
  let names = &settings.deployment;

  assert_eq!(settings.database.url.backend(), DbBackend::Sqlite);
  assert_eq!(
    [
      &*names.owner_name,
      &*names.agent_name,
      &*names.agent_key,
      &*names.sender_name,
      &*names.receiver_unit
    ],
    ["Owner", "Agent", "agent", "Agent", "receiver"]
  );

  let server = settings.server.expect("the example stands a server");
  let units = settings.units.expect("the example names the units");

  assert_eq!(server.listen.to_string(), "127.0.0.1:7841");
  assert_eq!(server.address.url().as_str(), "http://127.0.0.1:7841/");
  assert_eq!(&*server.unit, "job");
  assert_eq!(units.receiver.url().as_str(), "http://127.0.0.1:7844/");
  assert_eq!(units.web.url().as_str(), "http://127.0.0.1:7845/");
  assert_eq!(units.graph.url().as_str(), "http://127.0.0.1:7846/");
  assert_eq!(units.siren.as_ref().map(|siren| siren.url().as_str()), Some("http://127.0.0.1:7847/"));

  let host = settings.host.expect("the example names the host");

  assert_eq!(&*host.systemctl, Path::new("/usr/bin/systemctl"));
  assert_eq!(host.lock_wait_seconds.duration(), std::time::Duration::from_secs(20));
  assert_eq!(host.runner_environment.as_deref(), Some(Path::new("/srv/job/job.env")));
  assert_eq!(host.testbed_logs.as_deref(), Some(Path::new("/srv/job/testbed-logs")));
}

const HOST: &str = r#"[host]
source = "/s"
testbed = "/t"
systemctl = "/c"
systemd_run = "/r"
runner = "/j"
runner_directory = "/d"
runner_path = "/usr/bin"
locks = "/l"
prompts = "/p"
lock_wait_seconds = 5
launcher = "/x"
launcher_directory = "/d"
documents = "/w"
tree = "/p"
"#;

#[test]
fn the_host_is_read_where_given_with_absolute_paths_and_a_wait_above_nothing() {
  let read = |host: &str| Settings::read(&format!("{}\n{host}", with_database("sqlite::memory:")));
  let host = read(HOST).unwrap().host.expect("the host is read");

  assert_eq!(&*host.locks, Path::new("/l"));
  assert_eq!(host.runner_environment, None, "the environment file may be left out");
  assert_eq!(host.testbed_logs, None, "the testbed's log folder may be left out");
  assert_eq!(host.lock_wait_seconds.duration(), std::time::Duration::from_secs(5));
  assert!(
    read(&HOST.replace("\"/l\"", "\"locks\""))
      .unwrap_err()
      .to_string()
      .contains("is not an absolute path")
  );
  assert!(read(&HOST.replace("= 5", "= 0")).unwrap_err().to_string().contains("above 0"));
  assert!(read(&HOST.replace("launcher = \"/x\"\n", "")).is_err(), "the launcher is required");
  assert!(read(&format!("{HOST}spare = \"/y\"\n")).is_err(), "an unknown key is refused");
}

#[test]
fn the_server_and_its_units_are_left_out_by_the_libraries_and_read_where_given() {
  let without = Settings::read(&with_database("sqlite::memory:")).unwrap();
  let server = "[server]\nlisten = \"127.0.0.1:0\"\naddress = \"http://localhost:1/\"\nunit = \"job\"\n";
  let units = "[units]\nagency = \"http://a\"\ngate = \"http://g\"\nreceiver = \"http://r\"\nweb = \"http://w\"\ngraph = \"http://h\"\n";
  let with = Settings::read(&format!("{}\n{server}{units}", with_database("sqlite::memory:"))).unwrap();
  let https = Settings::read(&format!(
    "{}\n{server}{}",
    with_database("sqlite::memory:"),
    units.replace("http://g", "https://g")
  ));
  let no_socket = Settings::read(&format!(
    "{}\n{}",
    with_database("sqlite::memory:"),
    server.replace("127.0.0.1:0", "nowhere")
  ));

  assert!(without.server.is_none() && without.units.is_none() && without.host.is_none());

  let units = with.units.unwrap();

  assert_eq!(units.gate.url().as_str(), "http://g/");
  assert_eq!(units.web.url().as_str(), "http://w/");
  assert_eq!(units.graph.url().as_str(), "http://h/");
  assert_eq!(units.siren, None, "the siren may be left out");
  assert!(https.unwrap_err().to_string().contains("is not an http address"));
  assert!(no_socket.is_err());
}

#[test]
fn each_kind_of_address_is_read_as_its_backend() {
  let kinds = [
    ("sqlite://job.db?mode=rwc", DbBackend::Sqlite),
    ("sqlite::memory:", DbBackend::Sqlite),
    ("postgres://user@localhost/job", DbBackend::Postgres),
    ("postgresql:///job?host=/var/run/postgresql", DbBackend::Postgres),
  ];

  for (url, backend) in kinds {
    let settings = Settings::read(&with_database(url)).unwrap_or_else(|error| panic!("{url}: {error}"));

    assert_eq!(settings.database.url.backend(), backend, "{url}");
    assert_eq!(settings.database.url.as_str(), url);
  }
}

#[test]
fn an_address_of_another_kind_is_refused() {
  for url in ["mysql://localhost/job", "job.db", ""] {
    let error = Settings::read(&with_database(url)).expect_err(url).to_string();

    assert!(error.contains("is not a SQLite or PostgreSQL address"), "{url}: {error}");
  }
}

#[test]
fn a_missing_unknown_or_empty_setting_is_refused() {
  let missing = Settings::read(&with_database("sqlite::memory:").replace("agent_key = \"agent\"\n", ""));
  let unknown = Settings::read(&format!("{}\nport = 8080\n", with_database("sqlite::memory:")));
  let empty = Settings::read(&with_database("sqlite::memory:").replace("\"Owner\"", "\" \""));
  let no_deployment = Settings::read("[database]\nurl = \"sqlite::memory:\"\n");

  assert!(missing.unwrap_err().to_string().contains("agent_key"));
  assert!(unknown.unwrap_err().to_string().contains("port"));
  assert!(empty.unwrap_err().to_string().contains("a deployment name is empty"));
  assert!(no_deployment.unwrap_err().to_string().contains("deployment"));
}

#[test]
fn the_words_section_is_optional_and_read_as_it_stands() {
  let without = Settings::read(&with_database("sqlite::memory:")).unwrap();
  let with = Settings::read(&format!(
    "{}\n[words.closing]\nshare = \"part\"\nimpact = [\"reach\"]\n",
    with_database("sqlite::memory:")
  ))
  .unwrap();
  let closing = with.words["closing"].as_table().expect("a part's words are a table");

  assert!(without.words.is_empty());
  assert_eq!(closing["share"].as_str(), Some("part"));
  assert_eq!(closing["impact"].as_array().map(Vec::len), Some(1));
}

#[test]
fn a_file_that_does_not_load_is_named() {
  let scratch = tempfile::tempdir().unwrap();
  let absent = scratch.path().join("absent.toml");
  let broken = scratch.path().join("broken.toml");

  std::fs::write(&broken, "[database\n").unwrap();

  let unreadable = Settings::load(&absent).unwrap_err();
  let malformed = Settings::load(&broken).unwrap_err();

  assert!(matches!(unreadable, SettingsError::Unreadable { .. }));
  assert!(unreadable.to_string().starts_with(&absent.display().to_string()));
  assert!(matches!(malformed, SettingsError::Malformed { .. }));
  assert!(malformed.to_string().starts_with(&broken.display().to_string()));
}

#[tokio::test]
async fn a_sqlite_database_takes_the_statements() {
  let scratch = tempfile::tempdir().unwrap();
  let url = format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display());
  let settings = Settings::read(&with_database(&url)).unwrap();
  let db = settings.database.url.connect(2).await.expect("the SQLite database opens");

  assert_eq!(db.get_database_backend(), DbBackend::Sqlite);
  probe(&db).await;
}

#[tokio::test]
async fn a_postgres_database_takes_the_statements() {
  let admin_settings = Settings::read(&with_database(&test_postgres())).unwrap();
  let admin = admin_settings
    .database
    .url
    .connect(1)
    .await
    .expect("the test PostgreSQL server answers");
  let seconds = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
  let schema = format!("t_{seconds}_{}_settings", std::process::id());

  run(&admin, &format!("CREATE SCHEMA \"{schema}\""), vec![]).await.unwrap();

  let checked = async {
    let separator = if test_postgres().contains('?') { '&' } else { '?' };
    let url = format!("{}{separator}options[search_path]={schema}", test_postgres());
    let settings = Settings::read(&with_database(&url)).unwrap();
    let db = settings.database.url.connect(2).await.expect("the scratch schema opens");

    assert_eq!(db.get_database_backend(), DbBackend::Postgres);
    probe(&db).await;

    let found = one(
      &admin,
      "SELECT count(*) AS tables FROM information_schema.tables WHERE table_schema = $1 AND table_name = 'probe'",
      vec![schema.clone().into()],
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(found.try_get::<i64>("", "tables").unwrap(), 1, "the probe stands in the scratch schema");
  };

  checked.await;
  run(&admin, &format!("DROP SCHEMA \"{schema}\" CASCADE"), vec![]).await.unwrap();
}

#[test]
fn now_is_in_epoch_milliseconds() {
  let before = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis() as i64;
  let now = now_ms();

  assert!(now >= before && now - before < 60_000, "{before} then {now}");
}
