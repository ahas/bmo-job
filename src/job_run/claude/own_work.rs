//! Whether a command a job's pane stopped on stays inside the job's own places, so the runner allows it
//! itself instead of asking the owner (docs/runs.md, "A command the runner allows itself"). The
//! command is read the way the shell splits it; a script file it runs is read as the job running it, which
//! it may do unasked.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::LazyLock;

use brush_parser::word::{self, Parameter, ParameterExpr, TildeExpr, WordPiece, WordPieceWithSource};
use brush_parser::{ParserOptions, Token};
use regex::Regex;

/// The job's own places: the directories its work stays in, the files outside them a command may name,
/// where the command starts and the home a `~` stands for.
#[derive(Clone, Debug)]
pub struct Places {
  pub dirs: Vec<String>,
  pub files: Vec<String>,
  pub cwd: String,
  pub home: String,
}

impl Places {
  /// A job's places: its checkout, its item's scratch folder where it has an item, the temporary directory
  /// its scratchpad is in and the testbed's log folder where one is named; the platform's command line,
  /// which the testbed is run by, the null device and the standard streams.
  pub fn of_job(
    checkout: &Path,
    item: Option<&Path>,
    testbed_logs: Option<&Path>,
    tree: &Path,
    home: &Path,
  ) -> Self {
    let text = |path: &Path| path.to_string_lossy().into_owned();

    Self {
      dirs: [checkout]
        .into_iter()
        .chain(item)
        .map(text)
        .chain([text(&std::env::temp_dir())])
        .chain(testbed_logs.map(text))
        .collect(),
      files: [text(&tree.join("bin").join("bmo"))]
        .into_iter()
        .chain(["/dev/null", "/dev/stdin", "/dev/stdout", "/dev/stderr"].map(str::to_owned))
        .collect(),
      cwd: text(checkout),
      home: text(home),
    }
  }
}

static BOX_RULE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^\s*╌{3,}\s*$").expect("a box rule pattern"));
static BORDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*│ ?").expect("a box border pattern"));
static ASSIGNMENT: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^([A-Za-z_][A-Za-z0-9_]*)\+?=").expect("an assignment pattern"));
// A removal inside a word's text, which is a removal this reading does not see into.
static HIDDEN_REMOVAL: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r#"(?:^|[\s;&|(`'"])(?:rm|rmdir|unlink|shred)(?:\s|$)"#).expect("a hidden removal pattern")
});
static CLIMBS: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?:^|/)\.\.(?:/|$)").expect("a climb pattern"));
static SHELL_C: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^-[A-Za-z]*c[A-Za-z]*$").expect("a -c pattern"));

const REMOVERS: [&str; 4] = ["rm", "rmdir", "unlink", "shred"];
const SHELLS: [&str; 4] = ["sh", "bash", "zsh", "dash"];
// Programs that run what follows as somebody else or somewhere else, which no reading here follows.
const ELSEWHERE: [&str; 5] = ["sudo", "su", "doas", "pkexec", "ssh"];
// What may stand in front of a program without being it: runners, and the words of the shell's own
// grammar that open a command.
const RUNNERS: [&str; 14] = [
  "env", "command", "exec", "nohup", "time", "sudo", "doas", "nice", "ionice", "stdbuf", "setsid",
  "timeout", "builtin", "busybox",
];
const GRAMMAR: [&str; 10] = ["if", "then", "else", "elif", "do", "while", "until", "!", "{", "}"];
// Programs that only read and write nowhere but their output, the one place a value this reading cannot
// resolve may be handed.
const READERS: [&str; 21] = [
  "echo", "printf", "cat", "head", "tail", "wc", "cut", "tr", "uniq", "basename", "dirname", "rg", "grep",
  "ls", "stat", "test", "[", "jq", "realpath", "readlink", "diff",
];
// What may open a command before a loop does.
const OPENERS: [&str; 6] = ["do", "then", "else", "elif", "{", "!"];
const DECLARERS: [&str; 5] = ["export", "local", "declare", "readonly", "typeset"];
const REDIRECTS: [&str; 12] = ["<", ">", ">>", ">&", "<&", "<<", "<<-", "<<<", "&>", "&>>", "<>", ">|"];

/// The command a `Bash command` question shows in its box, its rows joined since the program wraps a long
/// command, or none for a question in any other shape.
pub fn command_of(text: &str) -> Option<String> {
  let rows: Vec<&str> = text.split('\n').collect();

  if rows.first().map(|row| row.trim()) != Some("Bash command") {
    return None;
  }

  let first = rows.iter().position(|row| BOX_RULE.is_match(row))?;
  let last = first + 1 + rows[first + 1..].iter().position(|row| BOX_RULE.is_match(row))?;
  let command = rows[first + 1..last]
    .iter()
    .map(|row| BORDER.replace(row, "").into_owned())
    .collect::<Vec<_>>()
    .join(" ");
  let command = command.trim();

  (!command.is_empty()).then(|| command.to_owned())
}

/// Whether the question is a command that stays inside the job's own places.
pub fn own_work(text: &str, places: &Places) -> bool {
  let Some(command) = command_of(text) else { return false };
  let mut reading = Reading {
    places,
    cwd: places.cwd.clone(),
    variables: HashMap::from([(
      "HOME".to_owned(),
      Value {
        text: places.home.clone(),
        open: false,
      },
    )]),
    exported: HashSet::from(["HOME".to_owned()]),
  };

  reading.inside(&command, 0)
}

// A word's value as the shell would make it with the variables it is given; what cannot be known from
// them stays as it was written, and `open` says some part did.
#[derive(Clone, Debug)]
struct Value {
  text: String,
  open: bool,
}

// One simple command: its words as written and the words its redirects name.
#[derive(Default)]
struct Simple {
  words: Vec<String>,
  redirects: Vec<String>,
}

fn simple_commands(script: &str) -> Option<Vec<Simple>> {
  let tokens = brush_parser::tokenize_str(script).ok()?;
  let mut commands = vec![];
  let mut current = Simple::default();
  let mut redirect = false;

  for token in tokens {
    match token {
      Token::Word(word, _) if redirect => {
        current.redirects.push(word);
        redirect = false;
      }
      Token::Word(word, _) => current.words.push(word),
      Token::Operator(operator, _) if REDIRECTS.contains(&operator.as_str()) => redirect = true,
      Token::Operator(..) => {
        commands.push(std::mem::take(&mut current));
        redirect = false;
      }
    }
  }

  commands.push(current);
  Some(commands)
}

// Where the program stands among a command's words, past assignments, runners and grammar.
fn program_at(values: &[Value], words: &[String]) -> Option<usize> {
  let mut at = 0;

  while at < words.len() {
    let name = base_of(&values[at].text);

    if ASSIGNMENT.is_match(&words[at]) || GRAMMAR.contains(&words[at].as_str()) {
      at += 1;
    } else if RUNNERS.contains(&name) {
      at += 1;

      while at < words.len() && values[at].text.starts_with('-') {
        at += 1;
      }

      if name == "timeout"
        && values
          .get(at)
          .is_some_and(|value| value.text.starts_with(|ch: char| ch.is_ascii_digit()))
      {
        at += 1;
      }
    } else {
      return Some(at);
    }
  }

  None
}

fn base_of(text: &str) -> &str {
  text.rsplit('/').next().unwrap_or(text)
}

// A path read with its `.` and `..` taken out, from where the command is.
fn resolve(cwd: &str, path: &str) -> String {
  let joined = if path.starts_with('/') { path.to_owned() } else { format!("{cwd}/{path}") };
  let mut parts: Vec<&str> = vec![];

  for part in joined.split('/') {
    match part {
      "" | "." => {}
      ".." => {
        parts.pop();
      }
      part => parts.push(part),
    }
  }

  format!("/{}", parts.join("/"))
}

fn under(path: &str, dir: &str) -> bool {
  path.strip_prefix(dir.trim_end_matches('/')).is_some_and(|rest| rest.starts_with('/'))
}

struct Reading<'p> {
  places: &'p Places,
  cwd: String,
  variables: HashMap<String, Value>,
  exported: HashSet<String>,
}

impl Reading<'_> {
  fn fork(&self, variables: HashMap<String, Value>) -> Self {
    Reading {
      places: self.places,
      cwd: self.cwd.clone(),
      variables,
      exported: self.exported.clone(),
    }
  }

  // A place a command may name: inside one of the places, one of them, or one of the files.
  fn named(&self, path: &str) -> bool {
    self.places.files.iter().any(|file| file == path)
      || self.places.dirs.iter().any(|dir| path == dir.trim_end_matches('/') || under(path, dir))
  }

  // Every path a word names lies in the places: an absolute one, the one after an option's `=`, and a
  // relative one that climbs, read from where the command is.
  fn paths_inside(&self, value: &Value) -> bool {
    let after = value.text.split_once('=').map(|(_, after)| after);

    std::iter::once(value.text.as_str()).chain(after).all(|candidate| {
      if candidate.starts_with('/') {
        self.named(&resolve("/", candidate))
      } else {
        !CLIMBS.is_match(candidate) || self.named(&resolve(&self.cwd, candidate))
      }
    })
  }

  // A word's value, with every command substitution in it read as commands of their own.
  fn expand(&self, word: &str, depth: u32) -> Option<Value> {
    let pieces = word::parse(word, &ParserOptions::default()).ok()?;
    let mut value = Value {
      text: String::new(),
      open: false,
    };

    self.pieces(word, &pieces, &mut value, depth).then_some(value)
  }

  fn pieces(&self, word: &str, pieces: &[WordPieceWithSource], value: &mut Value, depth: u32) -> bool {
    for piece in pieces {
      let source = word.get(piece.start_index..piece.end_index).unwrap_or_default();

      match &piece.piece {
        WordPiece::Text(text) | WordPiece::SingleQuotedText(text) => value.text.push_str(text),
        WordPiece::EscapeSequence(text) => value.text.push_str(text.strip_prefix('\\').unwrap_or(text)),
        WordPiece::DoubleQuotedSequence(inner) | WordPiece::GettextDoubleQuotedSequence(inner) => {
          // The pieces inside count from the word's own start.
          if !self.pieces(word, inner, value, depth) {
            return false;
          }
        }
        WordPiece::TildeExpansion(TildeExpr::Home) => value.text.push_str(&self.places.home),
        WordPiece::ParameterExpansion(expr) => match self.known(expr) {
          Some(known) => {
            value.text.push_str(&known.text);
            value.open |= known.open;
          }
          None => {
            value.text.push_str(source);
            value.open = true;
          }
        },
        WordPiece::CommandSubstitution(inner) | WordPiece::BackquotedCommandSubstitution(inner) => {
          // A command substitution runs with every variable of the shell it stands in.
          if !self.fork(self.variables.clone()).inside(inner, depth + 1) {
            return false;
          }

          value.text.push_str(source);
          value.open = true;
        }
        _ => {
          value.text.push_str(source);
          value.open = true;
        }
      }
    }

    true
  }

  // The value of a plain or guarded parameter this reading was given.
  fn known(&self, expr: &ParameterExpr) -> Option<Value> {
    let parameter = match expr {
      ParameterExpr::Parameter {
        parameter,
        indirect: false,
      }
      | ParameterExpr::IndicateErrorIfNullOrUnset {
        parameter,
        indirect: false,
        ..
      } => parameter,
      _ => return None,
    };

    match parameter {
      Parameter::Named(name) => self.variables.get(name).cloned(),
      _ => None,
    }
  }

  fn inside(&mut self, script: &str, depth: u32) -> bool {
    if depth > 3 {
      return false;
    }

    let Some(commands) = simple_commands(script) else { return false };

    commands.iter().all(|command| self.command_inside(command, depth))
  }

  fn command_inside(&mut self, command: &Simple, depth: u32) -> bool {
    let words = &command.words;
    let mut values: Vec<Value> = Vec::with_capacity(words.len());

    // Assignments take effect in order, so each word is read with the ones before it.
    let mut assigned_at = HashSet::new();
    let mut program = None;

    for (at, word) in words.iter().enumerate() {
      let declares = program.is_some_and(|at: usize| DECLARERS.contains(&base_of(&values[at].text)));

      if let Some(found) = ASSIGNMENT.captures(word)
        && (program.is_none() || declares)
      {
        let Some(value) = self.expand(&word[found[0].len()..], depth) else { return false };

        if !self.paths_inside(&value) || HIDDEN_REMOVAL.is_match(&value.text) {
          return false;
        }

        let name = found[1].to_owned();

        if program.is_some_and(|at: usize| base_of(&values[at].text) == "export") {
          self.exported.insert(name.clone());
        }

        self.variables.insert(name, value);
        values.push(Value {
          text: String::new(),
          open: false,
        });
        assigned_at.insert(at);
        continue;
      }

      let Some(value) = self.expand(word, depth) else { return false };

      values.push(value);

      if program.is_none() {
        program = program_at(&values, &words[..=at]).filter(|found| *found == at);
      }
    }

    // A redirect opens the file it names, so its target has to resolve.
    for target in &command.redirects {
      match self.expand(target, depth) {
        Some(value) if !value.open && self.paths_inside(&value) => {}
        _ => return false,
      }
    }

    self.bind_loop(words, &values);

    let reads = program.is_some_and(|at| READERS.contains(&base_of(&values[at].text)));
    let mut read = assigned_at;

    for at in 0..values.len() {
      if read.contains(&at) {
        continue;
      }

      let value = &values[at];
      let base = base_of(&value.text);

      if ELSEWHERE.contains(&base) {
        return false;
      }

      if REMOVERS.contains(&base) {
        let targets = targets_after(&values, at);

        if targets.is_empty()
          || targets
            .iter()
            .any(|target| target.open || !self.removable(&resolve(&self.cwd, &target.text)))
        {
          return false;
        }

        continue;
      }

      if SHELLS.contains(&base)
        && values.get(at + 1).is_some_and(|flag| SHELL_C.is_match(&flag.text))
        && let Some(script) = values.get(at + 2)
      {
        // A shell started anew knows only what it was exported.
        let variables = self
          .variables
          .iter()
          .filter(|(name, _)| self.exported.contains(*name))
          .map(|(name, value)| (name.clone(), value.clone()))
          .collect();

        if !self.fork(variables).inside(&script.text, depth + 1) {
          return false;
        }

        read.extend([at + 1, at + 2]);
        continue;
      }

      if base == "eval" && program == Some(at) {
        let script = values[at + 1..].iter().map(|one| one.text.as_str()).collect::<Vec<_>>().join(" ");

        return self.inside(&script, depth + 1);
      }

      // A value this reading cannot resolve may stand only where nothing is written: handed to a program
      // that only reads.
      if value.open && !(reads && program.is_some_and(|program| at > program)) {
        return false;
      }

      if !self.paths_inside(value) || HIDDEN_REMOVAL.is_match(&value.text) {
        return false;
      }
    }

    if let Some(at) = program
      && matches!(base_of(&values[at].text), "cd" | "pushd")
    {
      let target = values[at + 1..].iter().find(|arg| !arg.text.starts_with('-') || arg.text == "-");

      match target {
        Some(target) if !target.open && target.text != "-" => self.cwd = resolve(&self.cwd, &target.text),
        _ => return false,
      }
    }

    self
      .places
      .dirs
      .iter()
      .any(|dir| self.cwd == dir.trim_end_matches('/') || under(&self.cwd, dir))
  }

  // A loop over plain names, or over paths inside the places, gives its variable the first of them: any
  // of them is judged the same. A loop over anything else leaves its variable unresolved.
  fn bind_loop(&mut self, words: &[String], values: &[Value]) {
    let at = words.iter().take_while(|word| OPENERS.contains(&word.as_str())).count();

    let Some(name) = words.get(at + 1).filter(|name| {
      words.get(at).map(String::as_str) == Some("for")
        && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    }) else {
      return;
    };
    let list = if words.get(at + 2).map(String::as_str) == Some("in") {
      values.get(at + 3..).unwrap_or_default()
    } else {
      &[]
    };
    let plain = list
      .iter()
      .all(|one| !one.open && !one.text.contains('/') && !CLIMBS.is_match(&one.text));
    let paths = list
      .iter()
      .all(|one| !one.open && one.text.starts_with('/') && self.named(&resolve("/", &one.text)));
    let bound = match list.first() {
      Some(first) if plain || paths => Value {
        text: first.text.clone(),
        open: false,
      },
      _ => Value {
        text: format!("${name}"),
        open: true,
      },
    };

    self.variables.insert(name.clone(), bound);
  }

  // A removal reaches only what lies strictly inside a place, never a place itself.
  fn removable(&self, path: &str) -> bool {
    self.places.dirs.iter().any(|dir| under(path, dir))
  }
}

// The targets after a remover, options left out until `--`.
fn targets_after(values: &[Value], from: usize) -> Vec<&Value> {
  let mut options = true;

  values[from + 1..]
    .iter()
    .filter(|value| {
      if options && value.text == "--" {
        options = false;
        false
      } else {
        !options || !value.text.starts_with('-')
      }
    })
    .collect()
}
