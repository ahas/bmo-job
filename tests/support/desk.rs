//! A verb's stand of the test's own (docs/verbs.md, "Where a verb stands"): a migrated database in a scratch
//! directory, bmo-agency, bmo-brain, bmo-gate, bmo-receiver and bmo-testbed stood in and the host's folders in
//! the scratch directory, built from the settings it keeps as every verb's stand is.

use std::path::{Path, PathBuf};

use bmo_job::verbs::stand::Stand;
use job_base::Settings;
use migration::{Migrator, MigratorTrait};
use serde_json::json;
use tempfile::TempDir;

use super::server::{host_text, settings_text};
use super::{agency, brain, gate, receiver, testbed};

/// The variables a job's process and an agent's turn carry, which the binary is handed only as a test sets
/// them.
const TURN_VARIABLES: [&str; 5] =
  ["CLAUDECODE", "BMO_TASK_LOCKED", "BMO_RUN_NAME", "BMO_WORK_ORDER_STEP", "BMO_TURN_AGENT"];

/// The owner's sender and private chat, as the receiver stand-in names them.
pub const OWNER_SENDER: i64 = 5;
pub const OWNER_CHAT: i64 = 100;

pub struct Desk {
  pub scratch: TempDir,
  pub agency: agency::Stub,
  pub brain: brain::Stub,
  pub gate: gate::Stub,
  pub receiver: receiver::Stub,
  pub testbed: testbed::Stand,
  pub settings: Settings,
  /// The settings as the file the binary is handed.
  pub settings_file: PathBuf,
  pub stand: Stand,
}

impl Desk {
  pub async fn new() -> Self {
    Self::with("").await
  }

  /// The stand with these settings added after the ordinary ones.
  pub async fn with(more: &str) -> Self {
    let more = more.to_owned();

    Self::edited(move |_, text| text + &more).await
  }

  /// The stand whose owner's wording document holds `wording`.
  pub async fn worded(wording: &str) -> Self {
    let wording = wording.to_owned();

    Self::edited(move |scratch, text| {
      let document = scratch.join("wording.md");

      std::fs::write(&document, wording).expect("the wording document is written");
      text.replace(
        "owner_chat = 100\n",
        &format!(
          "owner_chat = 100\nwording = {}\n",
          serde_json::to_string(&document.display().to_string()).unwrap()
        ),
      )
    })
    .await
  }

  /// The stand on the ordinary settings as `edit` rewrites them, given the scratch directory.
  pub async fn edited(edit: impl FnOnce(&Path, String) -> String) -> Self {
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let (agency, gate, receiver) =
      (agency::Stub::start().await, gate::Stub::start().await, receiver::Stub::start().await);
    let (brain, testbed) = (brain::Stub::start().await, testbed::Stand::new());
    let url = format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display());
    let text = settings_text(&url, &agency.address(), &gate.address(), &receiver.address())
      .replace("brain = \"http://127.0.0.1:1/\"", &format!("brain = \"{}\"", brain.address()))
      + &host_text(scratch.path(), &testbed.program());
    let text = edit(scratch.path(), text);
    let settings = Settings::read(&text).expect("the desk's settings read");
    let settings_file = scratch.path().join("settings.toml");

    std::fs::write(&settings_file, &text).expect("the settings file is written");
    let db = settings.database.url.connect(1).await.expect("the database opens");

    Migrator::up(&db, None).await.expect("the migrations run up");
    receiver.answer("GET /owner", 200, json!({ "sender": OWNER_SENDER, "chat": OWNER_CHAT }));

    let stand = Stand::of(&settings).await.expect("the stand is built");

    Self {
      scratch,
      agency,
      brain,
      gate,
      receiver,
      testbed,
      settings,
      settings_file,
      stand,
    }
  }

  /// The binary run in `here` on the desk's settings, a job's markers handed it only as `env` sets them.
  pub async fn binary(&self, here: &Path, words: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_bmo-job"));

    for variable in TURN_VARIABLES {
      command.env_remove(variable);
    }

    command
      .envs(env.iter().copied())
      .env("BMO_JOB_SETTINGS", &self.settings_file)
      .current_dir(here)
      .args(words)
      .stdin(std::process::Stdio::null())
      .output()
      .await
      .expect("the binary runs")
  }

  /// A statement run on the stand's database.
  pub async fn run(&self, sql: &str, values: Vec<sea_orm::Value>) {
    job_base::run(&self.stand.db, sql, values)
      .await
      .unwrap_or_else(|failed| panic!("{sql}: {failed}"));
  }

  /// The first column of the one row a statement reads, as a number.
  pub async fn count(&self, sql: &str, values: Vec<sea_orm::Value>) -> i64 {
    job_base::one(&self.stand.db, sql, values)
      .await
      .unwrap_or_else(|failed| panic!("{sql}: {failed}"))
      .unwrap_or_else(|| panic!("{sql}: no row"))
      .try_get_by_index::<i64>(0)
      .unwrap()
  }
}
