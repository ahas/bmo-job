//! A tool's manifest, `tool.toml`, and the check that holds it to the entries its executable answers (docs/tools.md,
//! "The manifest").

use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::{Entry, PROTOCOL};

/// The manifest as the host reads it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
  pub tool: ToolPart,
  #[serde(default)]
  pub entry: Vec<EntryPart>,
}

/// The `[tool]` part: what the tool is and how the host builds, checks and runs it.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolPart {
  pub name: String,
  pub protocol: u32,
  pub language: String,
  pub build: String,
  pub check: String,
  pub run: Vec<String>,
  pub timeout: u64,
}

/// One `[[entry]]`.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EntryPart {
  pub namespace: String,
  pub name: String,
  pub title: Option<String>,
  pub description: String,
  pub input: String,
  pub output: Option<String>,
  #[serde(default)]
  pub hints: Hints,
  pub capabilities: Vec<String>,
  pub grant: Option<String>,
  #[serde(default)]
  pub bundle: BTreeMap<String, Bundle>,
}

/// What an entry's call does to the world.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hints {
  #[serde(default)]
  pub read_only: bool,
  #[serde(default)]
  pub destructive: bool,
  pub idempotent: Option<bool>,
  #[serde(default)]
  pub open_world: bool,
}

/// The arguments a session holding a capability is shown, and those it must give.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
  pub shown: Vec<String>,
  pub required: Option<Vec<String>>,
}

impl Manifest {
  /// The manifest of the tool in `dir`.
  pub fn read(dir: &Path) -> Result<Self, String> {
    let path = dir.join("tool.toml");
    let text = std::fs::read_to_string(&path).map_err(|why| format!("{}: {why}", path.display()))?;

    toml::from_str(&text).map_err(|why| format!("{}: {why}", path.display()))
  }
}

// A schema the manifest names, read as a JSON object; why not where it does not read.
fn schema(dir: &Path, file: &str) -> Result<Value, String> {
  let path = dir.join(file);
  let text = std::fs::read_to_string(&path).map_err(|why| format!("{file}: {why}"))?;
  let read: Value = serde_json::from_str(&text).map_err(|why| format!("{file}: {why}"))?;

  if read.get("type") != Some(&Value::from("object"))
    || !read.get("properties").is_some_and(Value::is_object)
  {
    return Err(format!("{file} is no object schema with properties"));
  }

  for required in read.get("required").and_then(Value::as_array).into_iter().flatten() {
    let named = required.as_str().unwrap_or_default();

    if read["properties"].get(named).is_none() {
      return Err(format!("{file} requires {named}, which it names no property of"));
    }
  }

  Ok(read)
}

/// Every way the manifest in `dir` differs from the tool `name` whose executable answers `entries`: none where
/// it declares each entry the executable answers and nothing else, at this protocol, each with schemas that read
/// and an entry acting on a grant taking the message it acts on.
pub fn differences(dir: &Path, name: &str, entries: &[&dyn Entry]) -> Vec<String> {
  let manifest = match Manifest::read(dir) {
    Ok(manifest) => manifest,
    Err(why) => return vec![why],
  };
  let mut found = vec![];

  if manifest.tool.name != name {
    found.push(format!("the manifest names the tool {}, not {name}", manifest.tool.name));
  }

  if manifest.tool.protocol != PROTOCOL {
    found.push(format!("the manifest speaks protocol {}, not {PROTOCOL}", manifest.tool.protocol));
  }

  let declared: Vec<String> = manifest
    .entry
    .iter()
    .map(|entry| format!("{}/{}", entry.namespace, entry.name))
    .collect();

  for entry in entries {
    if !declared.contains(&entry.full_name()) {
      found.push(format!("{} is answered and not declared", entry.full_name()));
    }
  }

  for (part, full) in manifest.entry.iter().zip(&declared) {
    if !entries.iter().any(|entry| entry.full_name() == *full) {
      found.push(format!("{full} is declared and not answered"));
    }

    match schema(dir, &part.input) {
      Err(why) => found.push(format!("{full}: {why}")),
      Ok(input) if part.grant.is_some() && input["properties"].get("message").is_none() => {
        found.push(format!("{full} acts on a grant and takes no message"));
      }
      Ok(_) => {}
    }

    if let Some(output) = &part.output
      && let Err(why) = schema(dir, output)
    {
      found.push(format!("{full}: {why}"));
    }
  }

  found
}
