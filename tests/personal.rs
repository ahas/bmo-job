//! No personal data anywhere in the tree (docs/PERSONAL-DATA.md; docs/checks.md, "No personal data"): no home
//! directory, email address, telephone number, credential, private network address and no name of the machine the
//! check runs on. The check names nobody itself: the machine's name is read where it stands.

use crate::support;

use std::fs;
use std::path::Path;

use regex::{Regex, RegexBuilder};

use support::{ROOT, files_under, shown};

/// One thing the tree may not hold, and what a failure calls it.
struct Sought {
  what: String,
  pattern: Regex,
  /// Values the pattern finds that are allowed to stand.
  allowed: &'static [&'static str],
}

/// A home directory as a user's is written: the folder followed by a name, on a Unix-like path or a
/// Windows one.
fn home_directory() -> Sought {
  Sought {
    what: "a home directory".to_owned(),
    pattern: Regex::new(r"(?:/home|/Users|/root)/[\w.-]|[A-Za-z]:\\Users\\[\w.-]")
      .expect("the home pattern builds"),
    allowed: &[],
  }
}

/// An email address, with the one address the rule allows.
fn email_address() -> Sought {
  Sought {
    what: "an email address".to_owned(),
    pattern: Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
      .expect("the email pattern builds"),
    allowed: &["dev@ahas.io"],
  }
}

/// A telephone number written in groups, with an optional country or area code.
fn phone_number() -> Sought {
  Sought {
    what: "a phone number".to_owned(),
    pattern: Regex::new(r"(?:\+\d{1,3}[ .-]?)?(?:\(\d{1,4}\)[ .-]?)?\d{2,4}[ .-]\d{3,4}[ .-]\d{4}\b")
      .expect("the phone pattern builds"),
    allowed: &[],
  }
}

/// A credential: a long value assigned to a secret's name, or the shape a vendor gives its keys.
fn credential() -> Sought {
  let pattern = concat!(
    r"(?i)(?:\b(?:api[_-]?key|access[_-]?token|refresh[_-]?token|auth[_-]?token|token|secret|key|",
    r#"client[_-]?secret|password|passwd|bearer)\b\s*[:=]\s*["']?[A-Za-z0-9_+/=.-]{16,}"#,
    r"|\bgh[pousr]_[A-Za-z0-9]{20,}|\bgithub_pat_[A-Za-z0-9_]{20,}",
    r"|\bsk-[A-Za-z0-9_-]{20,}|\bxox[abprs]-[A-Za-z0-9-]{10,}",
    r"|\bAKIA[0-9A-Z]{16}\b|\bAIza[0-9A-Za-z_-]{35}\b",
    r"|\beyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,})"
  );
  Sought {
    what: "a credential".to_owned(),
    pattern: Regex::new(pattern).expect("the credential pattern builds"),
    allowed: &[],
  }
}

/// A private or link-local network address. A loopback address is neither and passes.
fn private_address() -> Sought {
  let pattern = concat!(
    r"\b(?:10\.\d{1,3}\.\d{1,3}\.\d{1,3}",
    r"|172\.(?:1[6-9]|2[0-9]|3[01])\.\d{1,3}\.\d{1,3}",
    r"|192\.168\.\d{1,3}\.\d{1,3}",
    r"|169\.254\.\d{1,3}\.\d{1,3})\b"
  );
  Sought {
    what: "a private network address".to_owned(),
    pattern: Regex::new(pattern).expect("the private address pattern builds"),
    allowed: &[],
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
    allowed: &[],
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

/// Whether `line` holds `sought`, every occurrence of an allowed value passed over.
fn holds(sought: &Sought, line: &str) -> bool {
  sought
    .pattern
    .find_iter(line)
    .any(|found| !sought.allowed.iter().any(|allowed| allowed.eq_ignore_ascii_case(found.as_str())))
}

/// Each line of `text` holding one of `sought`, as `path:line: what`.
fn found_lines(path: &Path, text: &str, sought: &[Sought]) -> Vec<String> {
  text
    .lines()
    .enumerate()
    .flat_map(|(number, line)| {
      sought
        .iter()
        .filter(|one| holds(one, line))
        .map(move |one| format!("{}:{}: {}", shown(path), number + 1, one.what))
    })
    .collect()
}

#[test]
fn no_personal_data_anywhere_in_the_tree() {
  let mut sought = vec![home_directory(), email_address(), phone_number(), credential(), private_address()];
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
fn every_pattern_finds_its_own_and_passes_the_ordinary() {
  // Each sample is put together here, so this file holds no address of its own.
  let home = format!("/{}/", "home");
  let email = format!("{}{}", "a.person", "@example.org");
  let phone = format!("{}-{}-{}", "010", "1234", "5678");
  let private = format!("{}.{}.{}.{}", 192, 168, 0, 10);
  let key_line = format!("token = \"{}\"", "abcdefghijklmnop1234");
  let sought = [
    home_directory(),
    email_address(),
    phone_number(),
    credential(),
    private_address(),
    machine("box"),
  ];
  let text = [
    format!("cd {home}someone/work"),
    format!("mail {email}"),
    format!("call {phone}"),
    format!("serve on {private}"),
    key_line,
    "ssh BOX.local".to_owned(),
    "dev@ahas.io is allowed".to_owned(),
    "the loopback 127.0.0.1:7841 is ordinary".to_owned(),
    "version 0.1.0-rev.5".to_owned(),
    "a boxed answer".to_owned(),
  ]
  .join("\n");
  assert_eq!(
    found_lines(Path::new("sample"), &text, &sought),
    [
      "sample:1: a home directory",
      "sample:2: an email address",
      "sample:3: a phone number",
      "sample:4: a private network address",
      "sample:5: a credential",
      "sample:6: a machine's name \"box\"",
    ]
  );
  assert_eq!(machine_names("box.example.org"), ["box.example.org", "box"]);
}
