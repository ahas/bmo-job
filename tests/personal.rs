//! No personal data anywhere in the tree (docs/checks.md, "No personal data"): no home directory, no name of
//! this machine or of the work node the settings name, and none of the deployment's names from its own
//! settings file. The check names nobody itself: every name it looks for is read where it stands.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use job_base::Settings;
use regex::{Regex, RegexBuilder};

use support::{ROOT, files_under, shown};

/// One thing the tree may not hold, and what a failure calls it.
struct Sought {
  what: String,
  pattern: Regex,
}

/// A home directory as a user's is written: the folder followed by a name.
fn home_directory() -> Sought {
  Sought {
    what: "a home directory".to_owned(),
    pattern: Regex::new(r"/home/[\w.-]").expect("the home pattern builds"),
  }
}

/// A name standing as a whole word, as it is spelled or in any case. Where `hyphen_joins`, a word joined to
/// others by a hyphen is an identifier of its own and not the name.
fn whole_word(what: &str, name: &str, any_case: bool, hyphen_joins: bool) -> Sought {
  let apart = if hyphen_joins { r"[^\w-]" } else { r"[^\w]" };
  let pattern = RegexBuilder::new(&format!(r"(?:^|{apart}){}(?:{apart}|$)", regex::escape(name)))
    .case_insensitive(any_case)
    .build()
    .expect("a name's pattern builds");

  Sought {
    what: format!("{what} {name:?}"),
    pattern,
  }
}

/// The name of the machine the check runs on, as the system gives it.
fn this_machine() -> Option<String> {
  let mut buffer = [0u8; 256];
  // gethostname writes at most the buffer's length into memory this frame owns.
  let written = unsafe { libc::gethostname(buffer.as_mut_ptr().cast(), buffer.len()) };

  if written != 0 {
    return None;
  }

  let end = buffer.iter().position(|byte| *byte == 0).unwrap_or(buffer.len());
  let name = String::from_utf8_lossy(&buffer[..end]).trim().to_owned();

  (!name.is_empty()).then_some(name)
}

/// The deployment's settings file, where it stands, and the settings it holds; a file that stands and does not
/// load fails the check.
fn deployment_settings() -> Option<(PathBuf, Settings)> {
  let place = Settings::place();
  let place = if place.is_relative() { Path::new(ROOT).join(place) } else { place };

  if !place.exists() {
    return None;
  }

  let settings = Settings::load(&place).unwrap_or_else(|why| panic!("the deployment's settings: {why}"));

  Some((place, settings))
}

/// A machine's name and, where it is written with its domain, its own first part.
fn machine_names(name: &str) -> Vec<String> {
  let mut names = vec![name.to_owned()];

  if let Some((first, _)) = name.split_once('.')
    && !first.is_empty()
  {
    names.push(first.to_owned());
  }

  names
}

/// Everything the tree may not hold here, with the settings file the names were read from.
fn sought() -> (Vec<Sought>, Option<PathBuf>) {
  let mut sought = vec![home_directory()];
  let settings = deployment_settings();
  let mut machines: Vec<String> = this_machine().iter().flat_map(|name| machine_names(name)).collect();

  if let Some((_, settings)) = &settings {
    let deployment = &settings.deployment;

    for (what, name) in [
      ("the owner's name", &deployment.owner_name),
      ("the agent's name", &deployment.agent_name),
      ("the sender's name", &deployment.sender_name),
    ] {
      sought.push(whole_word(what, name, false, true));
    }

    if let Some(node) = &settings.node {
      machines.extend(machine_names(&node.name));
    }
  }

  machines.sort();
  machines.dedup();
  sought.extend(machines.iter().map(|name| whole_word("a machine's name", name, true, false)));

  (sought, settings.map(|(place, _)| place))
}

/// Each line of `text` holding one of `sought`, as `path:line: what`.
fn found_lines(path: &Path, text: &str, sought: &[Sought]) -> Vec<String> {
  text
    .lines()
    .enumerate()
    .flat_map(|(number, line)| {
      sought
        .iter()
        .filter(|one| one.pattern.is_match(line))
        .map(move |one| format!("{}:{}: {}", shown(path), number + 1, one.what))
    })
    .collect()
}

#[test]
fn no_home_directory_machine_name_or_deployment_name_anywhere_in_the_tree() {
  let (sought, settings) = sought();
  let skipped = settings.and_then(|place| place.canonicalize().ok());
  let found: Vec<String> = files_under(Path::new(ROOT))
    .into_iter()
    .filter(|path| path.canonicalize().ok() != skipped)
    .flat_map(|path| {
      let bytes = fs::read(&path).expect("a file is readable");

      found_lines(&path, &String::from_utf8_lossy(&bytes), &sought)
    })
    .collect();

  assert!(found.is_empty(), "these lines hold personal data: {found:#?}");
}

#[test]
fn a_home_is_a_folder_with_a_name_and_a_name_is_a_whole_word() {
  // The folder is put together here, so this file holds no home directory of its own.
  let home = format!("/{}/", "home");
  let sought = [
    home_directory(),
    whole_word("the agent's name", "Ada", false, true),
    whole_word("a machine's name", "box", true, false),
  ];
  let text = [
    format!("cd {home}someone/work"),
    format!(r"rm (?:{home}[^/\s]+)"),
    "Ada's report".to_owned(),
    "ADA_SETTINGS and Adamant and ada".to_owned(),
    "ssh BOX.local".to_owned(),
    "a boxed answer".to_owned(),
    "X-Ada-Deploy=bot".to_owned(),
    "the box-two checkout".to_owned(),
  ]
  .join("\n");

  assert_eq!(
    found_lines(Path::new("sample"), &text, &sought),
    [
      "sample:1: a home directory",
      "sample:3: the agent's name \"Ada\"",
      "sample:5: a machine's name \"box\"",
      "sample:8: a machine's name \"box\""
    ]
  );
  assert_eq!(machine_names("box.example.org"), ["box.example.org", "box"]);
}
