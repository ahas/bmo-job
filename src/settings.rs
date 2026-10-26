//! The settings file (docs/settings.md).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bmo_job_client::Labels;
use serde::Deserialize;

use crate::access::{Access, Caller};

/// The variable naming the settings file.
pub const SETTINGS_VARIABLE: &str = "BMO_JOB_SETTINGS";

/// The service's settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
  pub server: Server,
  pub database: Database,
  /// The callers with tokens of their own, by name.
  #[serde(default)]
  pub callers: BTreeMap<String, CallerSettings>,
  /// The labels whose items open by themselves.
  #[serde(default)]
  pub unattended: Unattended,
}

/// The labels whose items need no approval: an item raised carrying one opens at once (docs/api.md, "Unattended
/// labels").
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unattended {
  /// Each a key an item's labels hold, or `key=value`.
  #[serde(default)]
  pub labels: Vec<String>,
}

impl Unattended {
  /// The first of the labels `labels` holds, as the `label` filter reads one: a key, or `key=value`.
  pub fn matched(&self, labels: &Labels) -> Option<&str> {
    self.labels.iter().map(String::as_str).find(|label| match label.split_once('=') {
      Some((key, value)) => labels.get(key).is_some_and(|held| held == value),
      None => labels.contains_key(*label),
    })
  }
}

/// Where the service listens and the token it takes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
  /// The address and port to listen on.
  pub listen: String,
  /// The file holding the shared bearer token, which names no caller.
  pub token_file: PathBuf,
}

/// One caller with a token of its own.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CallerSettings {
  /// The file holding the caller's bearer token.
  pub token_file: PathBuf,
  /// Whether the caller is an approver, whose approval opens a draft.
  #[serde(default)]
  pub approves: bool,
}

/// Where the service keeps what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Database {
  /// A SQLite database's address, `sqlite:` and its file.
  pub url: String,
}

impl Settings {
  /// Where the settings file stands: the path the variable names, or `settings.toml` in the working
  /// directory.
  pub fn place() -> PathBuf {
    match std::env::var(SETTINGS_VARIABLE) {
      Ok(path) if !path.is_empty() => PathBuf::from(path),
      _ => PathBuf::from("settings.toml"),
    }
  }

  /// The settings in the file at `path`.
  pub fn load(path: &Path) -> Result<Self, String> {
    let text = std::fs::read_to_string(path)
      .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
    let settings: Self =
      toml::from_str(&text).map_err(|error| format!("{} does not read: {error}", path.display()))?;
    if !settings.database.url.starts_with("sqlite:") {
      return Err(format!("{}: the database url is not a SQLite address", path.display()));
    }
    for name in settings.callers.keys() {
      let fits = (1..=64).contains(&name.len())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
      if !fits {
        return Err(format!(
          "{}: the caller name {name:?} is not 1 to 64 letters, digits, dots, underscores and hyphens",
          path.display()
        ));
      }
    }
    for label in &settings.unattended.labels {
      if label.split_once('=').map_or(label.as_str(), |(key, _)| key).is_empty() {
        return Err(format!("{}: the unattended label {label:?} names no key", path.display()));
      }
    }
    Ok(settings)
  }

  /// The shared token in its file, its surrounding whitespace dropped. An empty token is refused.
  pub fn token(&self) -> Result<String, String> {
    read_token(&self.server.token_file)
  }

  /// Every token the service takes, the shared one and each caller's, with the caller it names. A token that
  /// is another's is refused.
  pub fn access(&self) -> Result<Access, String> {
    let mut access = Access::shared(&self.token()?);
    for (name, caller) in &self.callers {
      access = access.with(&read_token(&caller.token_file)?, Caller::named(name, caller.approves))?;
    }
    Ok(access)
  }
}

/// The token in the file at `path`, its surrounding whitespace dropped. An empty token is refused.
fn read_token(path: &Path) -> Result<String, String> {
  let token = std::fs::read_to_string(path)
    .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
  let token = token.trim().to_owned();
  if token.is_empty() {
    return Err(format!("{} holds no token", path.display()));
  }
  Ok(token)
}
