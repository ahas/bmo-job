//! The settings a deployment gives the service in one TOML file: the database it keeps its rows in and
//! the names it goes by (docs/settings.md).

use std::fmt;
use std::ops::Deref;
use std::path::{Path, PathBuf};

use sea_orm::{DatabaseConnection, DbBackend, DbErr};
use serde::Deserialize;

/// The variable naming the settings file's path.
pub const PLACE_VARIABLE: &str = "BMO_JOB_SETTINGS";

/// The settings file read where the variable names none, in the working directory.
pub const DEFAULT_FILE: &str = "settings.toml";

/// Everything the settings file holds.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
  pub database: DatabaseSettings,
  pub deployment: Deployment,
}

/// The `[database]` section.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DatabaseSettings {
  pub url: Address,
}

/// The `[deployment]` section: the names the deployment goes by.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Deployment {
  /// The deployment's owner, where nothing else gives the owner a name.
  pub owner_name: Name,
  /// The deployment's agent, as a room calls it.
  pub agent_name: Name,
  /// The deployment's agent, as the records name it.
  pub agent_key: Name,
  /// The sender the agent's own messages are stored under.
  pub sender_name: Name,
  /// The service unit the chat receiver runs as.
  pub receiver_unit: Name,
}

/// Why the settings file did not load.
#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
  #[error("{}: the settings file cannot be read: {source}", path.display())]
  Unreadable {
    path: PathBuf,
    source: std::io::Error,
  },
  #[error("{}: {source}", path.display())]
  Malformed {
    path: PathBuf,
    source: toml::de::Error,
  },
}

impl Settings {
  /// Where the settings file is: the path the variable names, or the default file in the working
  /// directory.
  pub fn place() -> PathBuf {
    match std::env::var(PLACE_VARIABLE) {
      Ok(named) if !named.trim().is_empty() => PathBuf::from(named.trim()),
      _ => PathBuf::from(DEFAULT_FILE),
    }
  }

  /// The settings in the file at `path`.
  pub fn load(path: &Path) -> Result<Self, SettingsError> {
    let text = std::fs::read_to_string(path).map_err(|source| SettingsError::Unreadable {
      path: path.to_owned(),
      source,
    })?;

    Self::read(&text).map_err(|source| SettingsError::Malformed {
      path: path.to_owned(),
      source,
    })
  }

  /// The settings in a file's text.
  pub fn read(text: &str) -> Result<Self, toml::de::Error> {
    toml::from_str(text)
  }
}

/// A database's connection address, of a kind the service keeps rows in.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Address {
  text: String,
  backend: DbBackend,
}

impl Address {
  pub fn as_str(&self) -> &str {
    &self.text
  }

  /// SQLite or PostgreSQL.
  pub fn backend(&self) -> DbBackend {
    self.backend
  }

  /// The database at this address, with at most `connections` connections open.
  pub async fn connect(&self, connections: u32) -> Result<DatabaseConnection, DbErr> {
    crate::database::connect(self.backend, &self.text, connections).await
  }
}

impl TryFrom<String> for Address {
  type Error = String;

  fn try_from(text: String) -> Result<Self, String> {
    let scheme = url::Url::parse(&text).map(|read| read.scheme().to_owned());
    let backend = match scheme.as_deref() {
      Ok("sqlite") => DbBackend::Sqlite,
      Ok("postgres" | "postgresql") => DbBackend::Postgres,
      _ => return Err(format!("`{text}` is not a SQLite or PostgreSQL address")),
    };

    Ok(Self { text, backend })
  }
}

/// A name the deployment goes by, never empty.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct Name(String);

impl TryFrom<String> for Name {
  type Error = &'static str;

  fn try_from(text: String) -> Result<Self, &'static str> {
    if text.trim().is_empty() {
      return Err("a deployment name is empty");
    }

    Ok(Self(text))
  }
}

impl Deref for Name {
  type Target = str;

  fn deref(&self) -> &str {
    &self.0
  }
}

impl fmt::Display for Name {
  fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    out.write_str(&self.0)
  }
}
