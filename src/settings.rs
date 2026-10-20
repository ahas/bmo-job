//! The settings file (docs/settings.md).

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// The variable naming the settings file.
pub const SETTINGS_VARIABLE: &str = "BMO_JOB_SETTINGS";

/// The service's settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
  pub server: Server,
  pub database: Database,
}

/// Where the service listens and the token it takes.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Server {
  /// The address and port to listen on.
  pub listen: String,
  /// The file holding the bearer token every request carries.
  pub token_file: PathBuf,
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
    Ok(settings)
  }

  /// The token in the token file, its surrounding whitespace dropped. An empty token is refused.
  pub fn token(&self) -> Result<String, String> {
    let path = &self.server.token_file;
    let token = std::fs::read_to_string(path)
      .map_err(|error| format!("{} could not be read: {error}", path.display()))?;
    let token = token.trim().to_owned();
    if token.is_empty() {
      return Err(format!("{} holds no token", path.display()));
    }
    Ok(token)
  }
}
