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
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
  pub database: DatabaseSettings,
  pub deployment: Deployment,
  /// The `[server]` section, which the server requires and the libraries read without.
  pub server: Option<ServerSettings>,
  /// The `[units]` section, which the server requires and the libraries read without.
  pub units: Option<Units>,
  /// The `[host]` section, which the server requires and the libraries read without.
  pub host: Option<Host>,
  /// The `[node]` section, which stands only on a work node.
  pub node: Option<NodeSettings>,
  /// The `[words]` section: the deployment's own words, one table to a policy part, which the policy
  /// reads and refuses at its own load where no part takes them; empty where the section is left out.
  #[serde(default)]
  pub words: toml::Table,
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
  /// The owner's private chat with the deployment's agent, where the service tells the agent its facts.
  pub owner_chat: Chat,
  /// The owner's wording document, which the terms the owner has set aside are read out of, where one is
  /// named.
  #[serde(default)]
  pub wording: Option<AbsolutePath>,
}

/// The `[server]` section: where the server stands.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerSettings {
  /// The socket address the server binds.
  pub listen: std::net::SocketAddr,
  /// The address other units reach the server at.
  pub address: HttpAddress,
  /// The service unit the server runs as.
  pub unit: Name,
}

/// The `[units]` section: the address of each unit the server calls on.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Units {
  pub agency: HttpAddress,
  /// The file holding bmo-agency's control token.
  pub agency_token: AbsolutePath,
  pub gate: HttpAddress,
  pub receiver: HttpAddress,
  pub web: HttpAddress,
  pub graph: HttpAddress,
  pub brain: HttpAddress,
  /// The file holding bmo-brain's token.
  pub brain_token: AbsolutePath,
  /// The cache siren a co-work branch's answer is told to, where one is given.
  pub siren: Option<HttpAddress>,
}

/// The `[host]` section: what the queue stands on of the machine it runs on.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Host {
  /// The checkout the built-in worker works in where its row names none.
  pub source: AbsolutePath,
  /// bmo-testbed's program.
  pub testbed: AbsolutePath,
  /// The program the user's systemd manager is asked and told through.
  pub systemctl: AbsolutePath,
  /// The program a job's unit is started through.
  pub systemd_run: AbsolutePath,
  /// The program whose `job-run` verb a job's unit runs.
  pub runner: AbsolutePath,
  /// The directory a job's unit runs in.
  pub runner_directory: AbsolutePath,
  /// The search path a job's unit is started with.
  pub runner_path: String,
  /// The file of variables a job's unit reads, where one is given.
  pub runner_environment: Option<AbsolutePath>,
  /// The directory the checkouts' job locks stand in.
  pub locks: AbsolutePath,
  /// The directory a job's text is written to for its unit.
  pub prompts: AbsolutePath,
  /// How long a launch waits for the last checkout's lock.
  pub lock_wait_seconds: WaitSeconds,
  /// The program a runner is launched through.
  pub launcher: AbsolutePath,
  /// The directory a runner's launcher runs in.
  pub launcher_directory: AbsolutePath,
  /// The directory a work order's prompts, result documents and check logs are written under.
  pub documents: AbsolutePath,
  /// The directory whose `bin/` holds the platform's command line.
  pub tree: AbsolutePath,
  /// The directory bmo-testbed writes its runs' logs in, where one is given.
  pub testbed_logs: Option<AbsolutePath>,
  /// The directory the brain's documents stand in, where one is given.
  pub brain: Option<AbsolutePath>,
  /// The directory a worker added with no path is given its own folder under, where one is given.
  pub worker_homes: Option<AbsolutePath>,
}

/// An absolute path on the machine the service runs on.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "PathBuf")]
pub struct AbsolutePath(PathBuf);

impl TryFrom<PathBuf> for AbsolutePath {
  type Error = String;

  fn try_from(path: PathBuf) -> Result<Self, String> {
    if path.is_absolute() {
      Ok(Self(path))
    } else {
      Err(format!("`{}` is not an absolute path", path.display()))
    }
  }
}

impl Deref for AbsolutePath {
  type Target = Path;

  fn deref(&self) -> &Path {
    &self.0
  }
}

/// A wait in whole seconds, above 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "u64")]
pub struct WaitSeconds(u64);

impl WaitSeconds {
  pub fn duration(self) -> std::time::Duration {
    std::time::Duration::from_secs(self.0)
  }
}

impl TryFrom<u64> for WaitSeconds {
  type Error = &'static str;

  fn try_from(seconds: u64) -> Result<Self, &'static str> {
    if seconds == 0 {
      return Err("a wait is a whole number of seconds above 0");
    }

    Ok(Self(seconds))
  }
}

/// An `http:` address a path can be added to.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "String")]
pub struct HttpAddress(url::Url);

impl HttpAddress {
  pub fn url(&self) -> &url::Url {
    &self.0
  }
}

impl TryFrom<String> for HttpAddress {
  type Error = String;

  fn try_from(text: String) -> Result<Self, String> {
    match url::Url::parse(&text) {
      Ok(read) if read.scheme() == "http" && !read.cannot_be_a_base() => Ok(Self(read)),
      _ => Err(format!("`{text}` is not an http address")),
    }
  }
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

/// The `[node]` section: the work node this machine is, and how the operations machine's binary is reached.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSettings {
  /// The node, as the workers' rows name it.
  pub name: Name,
  /// The command that runs this service's binary on the operations machine, a verb's own words after it.
  pub home: Command,
}

/// A command as the words it is run with, never none.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "Vec<String>")]
pub struct Command(Vec<String>);

impl TryFrom<Vec<String>> for Command {
  type Error = &'static str;

  fn try_from(words: Vec<String>) -> Result<Self, &'static str> {
    if words.first().is_none_or(|program| program.trim().is_empty()) {
      return Err("a node's home command names no word");
    }

    Ok(Self(words))
  }
}

impl Command {
  /// The program the command starts.
  pub fn program(&self) -> &str {
    &self.0[0]
  }

  /// The words the program is started with before a verb's own.
  pub fn args(&self) -> &[String] {
    &self.0[1..]
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

/// A chat by its number, which is never 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(try_from = "i64")]
pub struct Chat(i64);

impl Chat {
  pub fn number(self) -> i64 {
    self.0
  }
}

impl TryFrom<i64> for Chat {
  type Error = &'static str;

  fn try_from(number: i64) -> Result<Self, &'static str> {
    if number == 0 {
      return Err("the owner's chat is 0, which names no chat");
    }

    Ok(Self(number))
  }
}

impl fmt::Display for Name {
  fn fmt(&self, out: &mut fmt::Formatter<'_>) -> fmt::Result {
    out.write_str(&self.0)
  }
}
