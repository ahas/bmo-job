//! The service's fixed numbers and words (docs/policy.md): each file of `policy/` read as its own part,
//! the deployment's words laid over the parts that take them, the deployment's names built into the
//! patterns once, and the marks every line is filled with.

pub mod close;
pub mod closing;
pub mod english;
pub mod hold_commits;
pub mod job_prompt;
pub mod landing;
pub mod queue;
pub mod research;
pub mod runaway;
pub mod work_keys;
pub mod work_turns;
pub mod worker_words;
pub mod workers;

use job_base::{Deployment, Settings};
use serde::de::DeserializeOwned;
use toml::{Table, Value};

/// The text of a file of `policy/`, by its name without `.toml`, built into the crate.
macro_rules! policy_file {
  ($name:literal) => {
    include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../policy/", $name, ".toml"))
  };
}

pub(crate) use policy_file;

/// One part of the policy: a file of `policy/` read as its own type.
pub trait Part: DeserializeOwned {
  /// The file's name without `.toml`, which also names the part's words in the settings.
  const NAME: &'static str;
  /// The file's text.
  const TEXT: &'static str;

  /// Builds the deployment's names into the part's patterns, once, as the policy loads. A part holding
  /// no pattern stays as it is.
  fn build(&mut self, _names: &Names) {}
}

/// Why the policy did not load.
#[derive(Debug, thiserror::Error)]
pub enum PolicyError {
  #[error("the policy's {part}.toml does not read with the deployment's words: {source}")]
  Malformed {
    part: &'static str,
    source: toml::de::Error,
  },
  #[error("the settings give words for {part}, which takes none")]
  TakesNoWords { part: &'static str },
  #[error("the settings' words for {part} are not a table")]
  WordsNotATable { part: &'static str },
  #[error("the settings give words for {part}, which is no part of the policy")]
  NoSuchPart { part: String },
}

/// Every part of the policy, as one deployment reads it.
#[derive(Debug)]
pub struct Policy {
  pub queue: queue::QueueLimits,
  pub job_prompt: job_prompt::JobPrompt,
  pub research: research::Research,
  pub work_keys: work_keys::WorkKeys,
  pub work_turns: work_turns::WorkTurns,
  pub worker_words: worker_words::WorkerWords,
  pub workers: workers::WorkerRules,
  pub close: close::Close,
  pub closing: closing::Closing,
  pub landing: landing::Landing,
  pub runaway: runaway::Runaway,
  pub hold_commits: hold_commits::HoldCommits,
  names: Names,
}

impl Policy {
  /// Every part, with the settings' words laid over the parts that take them and the deployment's names
  /// built into the patterns.
  pub fn load(settings: &Settings) -> Result<Self, PolicyError> {
    let names = Names::of(&settings.deployment);
    let mut words = settings.words.clone();
    let policy = Self {
      queue: read(&mut words, &names)?,
      job_prompt: read(&mut words, &names)?,
      research: read(&mut words, &names)?,
      work_keys: read(&mut words, &names)?,
      work_turns: read(&mut words, &names)?,
      worker_words: read(&mut words, &names)?,
      workers: read(&mut words, &names)?,
      close: read(&mut words, &names)?,
      closing: read(&mut words, &names)?,
      landing: read(&mut words, &names)?,
      runaway: read(&mut words, &names)?,
      hold_commits: read(&mut words, &names)?,
      names,
    };

    match words.into_iter().next() {
      Some((part, _)) => Err(PolicyError::NoSuchPart { part }),
      None => Ok(policy),
    }
  }

  /// A line of the policy with each `{name}` it holds filled in from `values` and from the deployment's
  /// own marks (docs/policy.md, the marks), every one at once; a value given here takes the place of the
  /// deployment's mark of its name.
  pub fn fill(&self, line: &str, values: &[(&str, &str)]) -> String {
    let own = [
      ("owner", self.names.owner.as_str()),
      ("agent", self.names.agent.as_str()),
      ("sender", self.names.sender.as_str()),
      ("unfinished", self.closing.words.unfinished.as_str()),
      ("agent_mark", self.closing.strands.agent_mark.as_str()),
      ("owner_mark", self.closing.strands.owner_mark.as_str()),
    ];
    let all: Vec<(&str, &str)> = values.iter().copied().chain(own).collect();

    fill_marks(line, &all)
  }
}

/// One part from its file, with the deployment's words for it laid over its own and its patterns built.
fn read<P: Part>(words: &mut Table, names: &Names) -> Result<P, PolicyError> {
  let mut table: Table = P::TEXT.parse().map_err(|source| PolicyError::Malformed {
    part: P::NAME,
    source,
  })?;

  if let Some(given) = words.remove(P::NAME) {
    let Value::Table(given) = given else {
      return Err(PolicyError::WordsNotATable { part: P::NAME });
    };
    let Some(Value::Table(own)) = table.get_mut("words") else {
      return Err(PolicyError::TakesNoWords { part: P::NAME });
    };

    own.extend(given);
  }

  let mut part: P = Value::Table(table).try_into().map_err(|source| PolicyError::Malformed {
    part: P::NAME,
    source,
  })?;

  part.build(names);
  Ok(part)
}

/// The names the deployment goes by, as the policy's lines and patterns are filled with them.
#[derive(Clone, Debug)]
pub struct Names {
  pub owner: String,
  pub agent: String,
  pub sender: String,
}

impl Names {
  pub fn of(deployment: &Deployment) -> Self {
    Self {
      owner: deployment.owner_name.to_string(),
      agent: deployment.agent_name.to_string(),
      sender: deployment.sender_name.to_string(),
    }
  }

  /// A pattern with `{owner}`, `{agent}` and `{names}` filled in escaped, `{names}` matching either name,
  /// and each of `fragments` filled in as it stands, every mark at once.
  pub fn pattern(&self, template: &str, fragments: &[(&str, &str)]) -> String {
    let owner = escape(&self.owner);
    let agent = escape(&self.agent);
    let names = format!("{owner}|{agent}");
    let own = [("owner", owner.as_str()), ("agent", agent.as_str()), ("names", names.as_str())];
    let all: Vec<(&str, &str)> = fragments.iter().copied().chain(own).collect();

    fill_marks(template, &all)
  }
}

/// A word as a pattern matching only itself. The patterns are compiled by readers on more than one regex
/// engine, so only the characters every one of them reads as syntax are escaped: an escape of any other
/// character is refused by some of them.
pub fn escape(word: &str) -> String {
  let mut escaped = String::with_capacity(word.len());

  for character in word.chars() {
    if "\\.+*?()|[]{}^$".contains(character) {
      escaped.push('\\');
    }

    escaped.push(character);
  }

  escaped
}

/// Words as a pattern matching any one of them.
pub fn either(words: &[String]) -> String {
  words.iter().map(|word| escape(word)).collect::<Vec<_>>().join("|")
}

/// A line of the policy with each `{name}` it holds filled in, every one at once so a value that itself
/// holds a mark is left as it is; a mark nothing names stays.
pub fn fill_marks(line: &str, values: &[(&str, &str)]) -> String {
  let mut out = String::new();
  let mut rest = line;

  while let Some(open) = rest.find('{') {
    out.push_str(&rest[..open]);

    let after = &rest[open + 1..];
    let name_end = after
      .find(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
      .unwrap_or(after.len());
    let value = (name_end > 0 && after[name_end..].starts_with('}'))
      .then(|| values.iter().find(|(name, _)| *name == &after[..name_end]))
      .flatten();

    match value {
      Some((_, value)) => {
        out.push_str(value);
        rest = &after[name_end + 1..];
      }
      None => {
        out.push('{');
        rest = after;
      }
    }
  }

  out + rest
}
