//! Hangul going into git where the command line shows it (docs/verbs.md, "The shell hook").
//!
//! What goes into git is English: a `git commit` or `git tag` whose `-m` message, or the heredoc fed to
//! `-F -`, fails the close's English test is refused, and so is a new tag, branch or switched-to branch
//! whose name does. The test is the close's own, so the names of address its policy sets aside pass here
//! as they pass there. A message read from a file is out of this reading's sight and left to the close,
//! which refuses the commit by name before landing.

use job_policy::Policy;
use queue_rows::closing::reads_as_english;
use regex::Regex;

use super::shell::{Heredoc, program_of, shell_script, split, unquoted, value};

// The options git takes before its subcommand that are followed by a value of their own.
const GIT_VALUED: [&str; 7] =
  ["-C", "-c", "--git-dir", "--work-tree", "--namespace", "--config-env", "--super-prefix"];

/// One piece of text a git command writes into the repository, and what it is.
struct Written {
  what: &'static str,
  text: String,
}

// The subcommand and the words after it, past git's own options.
fn subcommand(args: &[String]) -> Option<(&str, &[String])> {
  let mut at = 0;

  while let Some(word) = args.get(at) {
    if !word.starts_with('-') {
      return Some((word.as_str(), &args[at + 1..]));
    }

    at += if GIT_VALUED.contains(&word.as_str()) { 2 } else { 1 };
  }

  None
}

// What an option given as `--name value`, `--name=value`, `-x value`, `-xvalue` or at the end of a short
// cluster `-ax value` carries. `short` names the short options taking a value, `long` the long ones in the
// order of the letters they stand for; each value found is handed to `found` with the letter of the option
// that carried it, and the words standing as no option's value come back in order.
fn options<'a>(
  args: &'a [String],
  short: &str,
  long: &[&str],
  mut found: impl FnMut(char, String),
) -> Vec<&'a String> {
  let mut plain = vec![];
  let mut at = 0;
  let mut options = true;

  while let Some(word) = args.get(at) {
    at += 1;

    if !options || !word.starts_with('-') || word == "-" {
      plain.push(word);
    } else if word == "--" {
      options = false;
    } else if let Some(name) = word.strip_prefix("--") {
      let (name, attached) = match name.split_once('=') {
        Some((name, attached)) => (name, Some(attached.to_owned())),
        None => (name, None),
      };

      if let Some(at_long) = long.iter().position(|one| *one == name) {
        let given = attached.or_else(|| {
          at += 1;
          args.get(at - 1).cloned()
        });

        if let Some(given) = given {
          found(short.chars().nth(at_long).unwrap_or('-'), given);
        }
      }
    } else {
      let cluster: Vec<char> = word[1..].chars().collect();

      if let Some(place) = cluster.iter().position(|letter| short.contains(*letter)) {
        let rest: String = cluster[place + 1..].iter().collect();
        let given = if rest.is_empty() {
          at += 1;
          args.get(at - 1).cloned()
        } else {
          Some(rest)
        };

        if let Some(given) = given {
          found(cluster[place], given);
        }
      }
    }
  }

  plain
}

// What a commit or a tag carries as its message: each `-m`, and the heredoc fed to `-F -`.
fn messages(
  args: &[String],
  short: &str,
  long: &[&str],
  heredocs: &[Heredoc],
  subcommand: &str,
) -> (Vec<Written>, Vec<String>) {
  let mut written = vec![];
  let mut from_input = false;
  let plain = options(args, short, long, |option, given| match option {
    'm' => written.push(Written {
      what: "the message",
      text: given,
    }),
    'F' if given == "-" => from_input = true,
    _ => {}
  });

  if from_input {
    written.extend(
      heredocs
        .iter()
        .filter(|held| held.opener.contains("git") && held.opener.contains(subcommand))
        .map(|held| Written {
          what: "the message fed on standard input",
          text: held.text.clone(),
        }),
    );
  }

  (written, plain.into_iter().cloned().collect())
}

// What one git command writes into the repository, as the command line shows it.
fn written_by(args: &[String], heredocs: &[Heredoc]) -> Vec<Written> {
  let Some((subcommand, rest)) = subcommand(args) else {
    return vec![];
  };
  let rest: Vec<String> = rest.iter().map(|word| value(word)).collect();
  let has =
    |flags: &[&str]| rest.iter().any(|word| flags.contains(&word.split('=').next().unwrap_or_default()));

  match subcommand {
    "commit" => {
      messages(
        &rest,
        "mFcCt",
        &["message", "file", "reedit-message", "reuse-message", "template"],
        heredocs,
        "commit",
      )
      .0
    }
    "tag" => {
      let (mut written, plain) =
        messages(&rest, "mFu", &["message", "file", "local-user"], heredocs, "tag");

      if !has(&["-d", "--delete", "-l", "--list", "-v", "--verify"])
        && let Some(name) = plain.first()
      {
        written.push(Written {
          what: "the tag name",
          text: name.clone(),
        });
      }

      written
    }
    "branch" => {
      if has(&[
        "-d",
        "-D",
        "--delete",
        "-l",
        "--list",
        "-a",
        "--all",
        "-r",
        "--remotes",
        "--show-current",
        "-u",
        "--set-upstream-to",
        "--unset-upstream",
        "--edit-description",
        "--contains",
        "--no-contains",
        "--merged",
        "--no-merged",
        "--points-at",
      ]) {
        return vec![];
      }

      let plain = options(&rest, "", &[], |_, _| {});
      let renamed = has(&["-m", "-M", "--move", "-c", "-C", "--copy"]);
      let name = if renamed { plain.last() } else { plain.first() };

      name
        .map(|name| Written {
          what: "the branch name",
          text: (*name).clone(),
        })
        .into_iter()
        .collect()
    }
    "switch" | "checkout" => {
      let (short, long): (&str, &[&str]) = if subcommand == "switch" {
        ("cC", &["create", "force-create", "orphan"])
      } else {
        ("bB", &["orphan"])
      };
      let mut written = vec![];

      options(&rest, short, long, |_, given| {
        written.push(Written {
          what: "the branch name",
          text: given,
        })
      });

      written
    }
    _ => vec![],
  }
}

/// The close's English test (docs/policy.md, `close.toml`): a text passes where it reads as English past the
/// names of address the close's policy sets aside.
pub struct EnglishTest {
  set_aside: Regex,
  latin_per_hangul: usize,
  names: String,
}

impl EnglishTest {
  /// The test the policy's close reads by.
  pub fn of(policy: &Policy) -> Self {
    let english = &policy.close.english;

    Self {
      set_aside: Regex::new(&english.set_aside)
        .unwrap_or_else(|_| Regex::new("$^").expect("an empty pattern")),
      latin_per_hangul: english.latin_per_hangul,
      names: policy.fill("{owner} or {agent}", &[]),
    }
  }

  /// Whether a text passes the test.
  pub fn passes(&self, text: &str) -> bool {
    reads_as_english(&self.set_aside, self.latin_per_hangul, text)
  }
}

fn refusal_for(written: &Written, english: &EnglishTest) -> String {
  format!(
    "refused: {what} holds Hangul, and what goes into git is English only: a commit message, subject and body; a branch name and a tag. The one exception is a name of address, {names}. Write it again in English, the owner's quoted words translated. A message read from a file with -F is left to the close, which refuses the commit by name.",
    what = written.what,
    names = english.names,
  )
}

/// Why this command may not run as written, or nothing.
pub fn git_language_refusal(command: &str, english: &EnglishTest) -> Option<String> {
  // A command passing the test whole has nothing in it to refuse.
  if english.passes(command) {
    return None;
  }

  refusal_at(command, english, 0)
}

fn refusal_at(command: &str, english: &EnglishTest, depth: usize) -> Option<String> {
  if depth > 3 || !command.contains("git") {
    return None;
  }

  let read = split(command);

  for one in &read.commands {
    let Some(run) = program_of(&one.words) else {
      continue;
    };

    if run.program == "git"
      && let Some(written) = written_by(&run.args, &read.heredocs)
        .into_iter()
        .find(|written| !english.passes(&written.text))
    {
      return Some(refusal_for(&written, english));
    }

    if let Some(refused) = shell_script(&run).and_then(|script| refusal_at(&script, english, depth + 1)) {
      return Some(refused);
    }

    if run.program == "eval"
      && let Some(refused) = refusal_at(
        &run.args.iter().map(|arg| unquoted(arg)).collect::<Vec<_>>().join(" "),
        english,
        depth + 1,
      )
    {
      return Some(refused);
    }
  }

  read.inner.iter().find_map(|inner| refusal_at(&inner.text, english, depth + 1))
}
