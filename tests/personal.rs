//! No personal data anywhere in the tree (docs/checks.md, "No personal data"): no home directory and no name of
//! the machine the check runs on. The check names nobody itself: the machine's name is read where it stands.

mod support;

use std::fs;
use std::path::Path;

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

/// A machine's name standing as a whole word, in any case.
fn machine(name: &str) -> Sought {
  let pattern = RegexBuilder::new(&format!(r"(?:^|[^\w]){}(?:[^\w]|$)", regex::escape(name)))
    .case_insensitive(true)
    .build()
    .expect("a name's pattern builds");
  Sought {
    what: format!("a machine's name {name:?}"),
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
fn no_home_directory_or_machine_name_anywhere_in_the_tree() {
  let mut sought = vec![home_directory()];
  sought.extend(this_machine().iter().flat_map(|name| machine_names(name)).map(|name| machine(&name)));
  let found: Vec<String> = files_under(Path::new(ROOT))
    .into_iter()
    .filter(|path| path.file_name().is_none_or(|name| name != "settings.toml"))
    .flat_map(|path| {
      let bytes = fs::read(&path).expect("a file is readable");
      found_lines(&path, &String::from_utf8_lossy(&bytes), &sought)
    })
    .collect();
  assert!(found.is_empty(), "these lines hold personal data: {found:#?}");
}

#[test]
fn a_home_is_a_folder_with_a_name_and_a_machine_is_a_whole_word() {
  // The folder is put together here, so this file holds no home directory of its own.
  let home = format!("/{}/", "home");
  let sought = [home_directory(), machine("box")];
  let text = [
    format!("cd {home}someone/work"),
    format!(r"rm (?:{home}[^/\s]+)"),
    "ssh BOX.local".to_owned(),
    "a boxed answer".to_owned(),
    "the box-two checkout".to_owned(),
  ]
  .join("\n");
  assert_eq!(
    found_lines(Path::new("sample"), &text, &sought),
    [
      "sample:1: a home directory",
      "sample:3: a machine's name \"box\"",
      "sample:5: a machine's name \"box\""
    ]
  );
  assert_eq!(machine_names("box.example.org"), ["box.example.org", "box"]);
}
