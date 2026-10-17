//! What a command writes, as a reading of its text: into a place the guard keeps — the structure document,
//! the brain, another worker's checkout — and into the job's own checkout with an inline script, an
//! in-place editor or a redirect (docs/verbs.md, "The shell hook"). A
//! command that only reads — `cat`, `grep`, `git diff` — writes nothing.

use std::sync::LazyLock;

use regex::Regex;

use super::reading::{self, expand_assigned, program_of, read_command};
use super::records::{IN_PLACE_EDITORS, INNER_TEXT_DEPTH, WRITER_WORDS, in_place_flag, spellings};
use super::shell::{Heredoc, SHELLS};

/// The document only the owner changes, however a path to it is spelled.
pub const STRUCTURE_DOCUMENT: &str = "STRUCTURE.md";

// Programs that run a script handed to them inline.
const INTERPRETERS: [&str; 7] = ["python", "python3", "node", "bun", "perl", "ruby", "deno"];

/// A place a command may write into, as a reading of its text: what it is called in a refusal, what names
/// it, the git subcommands that count as changing it, and whether a `cd` into it makes every relative path
/// it.
pub trait WriteTarget {
  fn label(&self) -> String;
  fn names(&self, text: &str) -> bool;
  fn git_writers(&self) -> &'static [&'static str];
  fn follows_cd(&self) -> bool;
}

/// The structure document.
pub struct Structure;

impl WriteTarget for Structure {
  fn label(&self) -> String {
    STRUCTURE_DOCUMENT.into()
  }

  fn names(&self, text: &str) -> bool {
    text.contains(STRUCTURE_DOCUMENT)
  }

  fn git_writers(&self) -> &'static [&'static str] {
    &["rm", "mv", "checkout", "restore", "apply"]
  }

  fn follows_cd(&self) -> bool {
    false
  }
}

/// The brain's directory (docs/settings.md, `[host] brain`), named by its absolute root.
pub struct Brain {
  pub root: String,
}

impl WriteTarget for Brain {
  fn label(&self) -> String {
    "the brain".into()
  }

  fn names(&self, text: &str) -> bool {
    !self.root.is_empty() && text.contains(&self.root)
  }

  fn git_writers(&self) -> &'static [&'static str] {
    &[
      "add",
      "commit",
      "rm",
      "mv",
      "checkout",
      "restore",
      "reset",
      "apply",
      "stash",
      "merge",
      "revert",
      "cherry-pick",
    ]
  }

  fn follows_cd(&self) -> bool {
    true
  }
}

/// Another worker's checkout (docs/verbs.md, "Borrowing a checkout"): a job writes
/// in its own worker's checkout and in one it has borrowed, and in no other registered worker's. A path is
/// matched at a boundary, so a support worktree whose path begins with its base's is never read as the
/// base.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForeignCheckout {
  pub worker: String,
  pub base: String,
  pub cwd: String,
}

struct Foreign<'a> {
  checkout: &'a ForeignCheckout,
  patterns: Vec<Regex>,
}

impl<'a> Foreign<'a> {
  fn of(checkout: &'a ForeignCheckout, home: &str) -> Self {
    let patterns = spellings(&checkout.cwd, home)
      .iter()
      .filter_map(|spelling| Regex::new(&format!(r#"{}(?:[/\s"';&|)]|$)"#, regex::escape(spelling))).ok())
      .collect();

    Self { checkout, patterns }
  }
}

impl WriteTarget for Foreign<'_> {
  fn label(&self) -> String {
    format!("{} ({}'s checkout)", self.checkout.cwd, self.checkout.worker)
  }

  fn names(&self, text: &str) -> bool {
    self.patterns.iter().any(|pattern| pattern.is_match(text))
  }

  fn git_writers(&self) -> &'static [&'static str] {
    &[
      "add",
      "commit",
      "rm",
      "mv",
      "checkout",
      "switch",
      "restore",
      "reset",
      "apply",
      "stash",
      "merge",
      "rebase",
      "revert",
      "cherry-pick",
      "pull",
    ]
  }

  fn follows_cd(&self) -> bool {
    true
  }
}

// The letters and the words an interpreter carries a script under: python's `-c`, and the evaluate and
// print flags of node, bun, deno, perl and ruby.
const SCRIPT_LETTERS: [char; 3] = ['c', 'e', 'p'];
const SCRIPT_WORDS: [&str; 2] = ["--eval", "--print"];

/// Every script an interpreter's command line hands it inline. Perl and Ruby take their flags combined —
/// `-ne`, `-pe`, `-nlae` — so a cluster is read letter by letter rather than matched whole, under two
/// rules. A letter that carries a script takes the rest of its cluster as that script, or the next word
/// where the cluster ends at it, and the reading goes on through the cluster either way: a letter that means
/// something else to the interpreter it was handed to must not hide the letter behind it. And in an in-place
/// editor's cluster an `i` takes everything behind it as the backup suffix, so the reading of that cluster
/// ends there; for an interpreter whose `-i` takes no suffix it is one flag among the others.
fn inline_scripts(program: &str, args: &[String]) -> Vec<String> {
  let mut scripts = vec![];
  let suffix_after_i = IN_PLACE_EDITORS.contains(&program);

  for (at, arg) in args.iter().enumerate() {
    if let Some(long) = arg.strip_prefix("--") {
      let (name, given) = match long.split_once('=') {
        Some((name, given)) => (name, Some(given)),
        None => (long, None),
      };

      if SCRIPT_WORDS.contains(&format!("--{name}").as_str()) {
        scripts.push(given.map_or_else(|| args.get(at + 1).cloned().unwrap_or_default(), str::to_owned));
      }

      continue;
    }

    let Some(cluster) = arg.strip_prefix('-') else {
      continue;
    };

    for (letter, ch) in cluster.char_indices() {
      if ch == 'i' && suffix_after_i {
        break;
      }

      if SCRIPT_LETTERS.contains(&ch) {
        let rest = &cluster[letter + ch.len_utf8()..];

        scripts.push(if rest.is_empty() {
          args.get(at + 1).cloned().unwrap_or_default()
        } else {
          rest.to_owned()
        });
      }
    }
  }

  scripts
}

// Whether an interpreter reads its script from where a heredoc or a pipe puts it. A script file standing
// among the arguments is a file of its own, out of this reading's sight; `-` standing there is the
// interpreter's own name for standing input, and a `<` target is no argument at all.
fn reads_script_from_stdin(args: &[String]) -> bool {
  args
    .iter()
    .find(|arg| *arg == "-" || !arg.starts_with('-'))
    .is_none_or(|first| first == "-")
}

// Whether a script writes into the target: one of its write calls names it, or it names the target and
// makes a write call whose file the reading cannot resolve — refused rather than guessed at, since what is
// guarded here is guarded because a wrong guess costs too much. A script that opens the target only to
// read it makes no write call, and is not a write.
fn script_writes_target(script: &str, target: &dyn WriteTarget) -> bool {
  let calls = write_calls(script);

  calls.paths.iter().any(|path| target.names(path)) || (calls.unresolved && target.names(script))
}

// A path spelled relative to wherever the shell stands.
fn relative_path(word: &str) -> bool {
  !word.is_empty() && !word.starts_with(['-', '/', '~', '$', '&'])
}

fn is_shell_or_eval(run: &reading::Run) -> bool {
  SHELLS.contains(&run.program.as_str()) || run.program == "eval"
}

// The text a shell is handed with `-c`, or the text given to `eval`.
fn handed_text(run: &reading::Run) -> Option<String> {
  if run.program == "eval" {
    Some(run.args.join(" "))
  } else {
    reading::shell_text(run).cloned()
  }
}

/// Why this command would write into the target, or nothing.
pub fn write_reason(original: &str, target: &dyn WriteTarget) -> Option<String> {
  reason_at(original, target, 0)
}

fn reason_at(original: &str, target: &dyn WriteTarget, depth: usize) -> Option<String> {
  let command = expand_assigned(original);

  if !target.names(&command) {
    return None;
  }

  let read = read_command(&command);
  let label = target.label();
  let mut inside = false;

  for segment in &read.commands {
    let named = |word: &String| target.names(word) || (inside && relative_path(word));

    if segment.redirects.iter().any(named) {
      return Some(format!("it writes into {label}"));
    }

    let Some(run) = program_of(&segment.words) else {
      continue;
    };

    if target.follows_cd() && (run.program == "cd" || run.program == "pushd") {
      inside = run.args.iter().any(|arg| target.names(arg))
        || (inside && run.args.iter().any(|arg| relative_path(arg)));
      continue;
    }

    let arg_named = run.args.iter().any(named);
    let program = run.program.as_str();

    if arg_named && WRITER_WORDS.contains(&program) {
      return Some(format!("it copies, moves, creates or removes files in {label}"));
    }

    if arg_named && IN_PLACE_EDITORS.contains(&program) && run.args.iter().any(|arg| in_place_flag(arg)) {
      return Some(format!("it edits {label} in place"));
    }

    if (arg_named || inside)
      && program == "git"
      && run.args.iter().any(|arg| target.git_writers().contains(&arg.as_str()))
    {
      return Some(format!("it changes {label} through git"));
    }

    if INTERPRETERS.contains(&program) {
      let scripts = inline_scripts(program, &run.args);
      let writes = if scripts.is_empty() {
        reads_script_from_stdin(&run.args)
          && read.heredocs.iter().any(|held| script_writes_target(&held.text, target))
      } else {
        scripts.iter().any(|script| script_writes_target(script, target))
      };

      if writes {
        return Some(format!("it runs a script that names {label} and writes"));
      }
    }

    if depth < INNER_TEXT_DEPTH
      && is_shell_or_eval(&run)
      && let Some(inner) = handed_text(&run)
      && let Some(refused) = reason_at(&inner, target, depth + 1)
    {
      return Some(refused);
    }
  }

  None
}

/// Why this command would change the structure document, or nothing.
pub fn structure_command_refusal(command: &str) -> Option<String> {
  write_reason(command, &Structure)
}

/// Why a file tool's call would change the structure document, or nothing.
pub fn structure_file_refusal(path: &str) -> Option<String> {
  (path == STRUCTURE_DOCUMENT || path.ends_with(&format!("/{STRUCTURE_DOCUMENT}")))
    .then(|| format!("it edits {STRUCTURE_DOCUMENT}"))
}

/// Why this command writes into the brain, or nothing.
pub fn brain_command_write(command: &str, root: &str) -> Option<String> {
  write_reason(
    command,
    &Brain {
      root: root.to_owned(),
    },
  )
}

/// Whether a file tool's target lies inside the brain.
pub fn brain_file_write(path: &str, root: &str) -> bool {
  if root.is_empty() {
    return false;
  }

  let base = if root.ends_with('/') { root.to_owned() } else { format!("{root}/") };

  path.starts_with(&base)
}

/// What a refusal of a write into another worker's checkout says.
pub fn foreign_checkout_message(checkout: &ForeignCheckout, why: &str) -> String {
  format!(
    "refused: {why} — {cwd} is {worker}'s checkout, and a job writes only in its own checkout and in one it has borrowed, since what is open in a checkout is parked as its holder's own when that holder stops. To change that repository, borrow one of its checkouts for this job with the platform's job borrow {base} — it hands you a free one, on a branch of this item where it is a support worker's (docs/verbs.md, \"Borrowing a checkout\").",
    cwd = checkout.cwd,
    worker = checkout.worker,
    base = checkout.base,
  )
}

/// The checkout a command would write into that is another worker's, with why, or nothing.
pub fn foreign_command_write<'a>(
  command: &str,
  checkouts: &'a [ForeignCheckout],
  home: &str,
) -> Option<(&'a ForeignCheckout, String)> {
  checkouts
    .iter()
    .find_map(|checkout| write_reason(command, &Foreign::of(checkout, home)).map(|why| (checkout, why)))
}

/// The checkout a file tool's target lies in that is another worker's, or nothing.
pub fn foreign_file_write<'a>(path: &str, checkouts: &'a [ForeignCheckout]) -> Option<&'a ForeignCheckout> {
  checkouts.iter().find(|checkout| {
    path == checkout.cwd || path.starts_with(&format!("{}/", checkout.cwd.trim_end_matches('/')))
  })
}

// The text tools whose redirect this reads. Their reading side — a `sed -n` range, a `cat`, an `awk` summing
// a column — is most of what the shell is for here and is not touched.
const TEXT_TOOLS: [&str; 9] = ["cat", "printf", "echo", "tee", "sed", "awk", "jq", "tr", "cut"];

/// What a command was doing to a file, in the shapes the record's own branches came to. `Unread` is the
/// shape of a write this reading can see happening and cannot name — and the shape no verb answers, so it
/// is the one that is never refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteShape {
  Phrase,
  Block,
  Pattern,
  Anchors,
  Lines,
  Cut,
  Insert,
  Append,
  Whole,
  Unread,
}

/// By what means a command writes a file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Means {
  Script,
  InPlace,
  Redirect,
}

/// A write into this checkout as the reading came to it: what was being done, by what means, to which file,
/// and the program word it was read in, as the command spelled it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckoutWrite {
  pub shape: WriteShape,
  pub means: Means,
  pub path: String,
  pub word: String,
}

// A path inside the checkout, as a command spells it: the root itself, a `~` spelling of it, or a relative
// path while the shell has not left the checkout. A spelling carrying a variable, a substitution or a `..`
// could be anywhere, and what could be anywhere is left alone.
fn inside_checkout(word: &str, root: &str, home: &str, cwd_inside: bool) -> bool {
  if word.is_empty()
    || word.contains('$')
    || word.contains('`')
    || word.contains("..")
    || word.starts_with('&')
  {
    return false;
  }

  let spelled = if !home.is_empty() && (word == "~" || word.starts_with("~/")) {
    format!("{home}{}", &word[1..])
  } else {
    word.to_owned()
  };

  if spelled.starts_with('/') {
    return spelled == root || spelled.starts_with(&format!("{root}/"));
  }

  cwd_inside && !spelled.starts_with('~') && !spelled.starts_with('-')
}

// Where a `cd` leaves the shell: inside the checkout, or somewhere this reading cannot name. `cd` with
// nothing after it goes home, which is not the checkout.
fn moved_inside(args: &[String], root: &str, home: &str, was: bool) -> bool {
  args
    .iter()
    .find(|arg| !arg.starts_with('-'))
    .is_some_and(|to| inside_checkout(to, root, home, was))
}

// An in-place editor's expression and the files it edits. `sed` takes its expression as the first word no
// flag asked for; `perl` and `ruby` take theirs under an evaluate flag, and then every plain word is a file.
fn in_place_parts(program: &str, args: &[String]) -> (String, Vec<String>) {
  let scripts = inline_scripts(program, args);
  let plain: Vec<String> = args
    .iter()
    .filter(|arg| !arg.starts_with('-') && !scripts.contains(arg))
    .cloned()
    .collect();

  if scripts.is_empty() && !plain.is_empty() {
    return (plain[0].clone(), plain[1..].to_vec());
  }

  (scripts.join("\n"), plain)
}

macro_rules! pattern {
  ($name:ident, $text:expr) => {
    static $name: LazyLock<Regex> = LazyLock::new(|| Regex::new($text).expect("a write-shape pattern"));
  };
}

pattern!(LINE_CUT, r"^\s*\d+(?:\s*,\s*\d+)?\s*d\s*$");
pattern!(LINE_INSERT, r"^\s*\d+\s*[ia]\\");
pattern!(SUBSTITUTES, r"\bre\.subn?\s*\(|\.g?sub!?\s*\(");
pattern!(READS, r"\.read\s*\(|read_text\s*\(|readlines\s*\(|\.read\b|readFileSync|Bun\.file");
pattern!(DELETES, r"\bdel\s+\w+\s*\[");
pattern!(SPLITS_LINES, r#"readlines|splitlines|\.split\s*\(\s*['"]\\n['"]"#);
pattern!(REPLACES, r"\.replace\s*\(");
pattern!(INSERTS, r"\.insert\s*\(");
pattern!(FINDS, r"\.(?:rindex|index|find)\s*\(");
pattern!(SLICES, r"\[\s*:?\s*[\w-]");
pattern!(MULTILINE, r#"'''|"""|\\n"#);
pattern!(PREPENDS, r#"(?m)=\s*['"].*['"]\s*\+\s*\w+\s*$"#);

// What an expression handed to an in-place editor was doing: a line named by number, or a substitution,
// which is what nearly every one of them is.
fn expression_shape(script: &str) -> WriteShape {
  if LINE_CUT.is_match(script) {
    WriteShape::Cut
  } else if LINE_INSERT.is_match(script) {
    WriteShape::Insert
  } else {
    WriteShape::Pattern
  }
}

// What an inline script was doing to the file it wrote. The order is the record's own: a pattern, then a
// region named by line number, then a slice between two anchors, then a block, then a phrase; a script that
// writes without reading writes the whole file. A read-modify-write whose kind the text does not show is
// `Unread`, which names no verb and so is not refused.
fn script_shape(script: &str) -> WriteShape {
  if SUBSTITUTES.is_match(script) {
    return WriteShape::Pattern;
  }

  if !READS.is_match(script) {
    return WriteShape::Whole;
  }

  if DELETES.is_match(script) {
    return WriteShape::Cut;
  }

  if SPLITS_LINES.is_match(script) && !REPLACES.is_match(script) {
    return if INSERTS.is_match(script) { WriteShape::Insert } else { WriteShape::Lines };
  }

  if FINDS.is_match(script) && SLICES.is_match(script) {
    return WriteShape::Anchors;
  }

  if REPLACES.is_match(script) {
    return if MULTILINE.is_match(script) { WriteShape::Block } else { WriteShape::Phrase };
  }

  // A line put in front of what the file already held.
  if PREPENDS.is_match(script) {
    return WriteShape::Insert;
  }

  WriteShape::Unread
}

/// The paths an inline script writes, as the script's own text spells them, and whether a write call was
/// made whose file could not be read.
struct WriteCalls {
  paths: Vec<String>,
  unresolved: bool,
}

pattern!(PY_OPEN, r#"\bopen\s*\(\s*([^,()]+?)\s*,\s*(?:mode\s*=\s*)?['"]([rwabxt+]+)['"]"#);
pattern!(PY_WRITING_MODE, r"[wax+]");
pattern!(
  PERL_OPEN_TWO,
  r#"\bopen\s*\(\s*(?:my\s+)?[\w$]+\s*,\s*['"]\s*(?:>>?|\+[<>])\s*([^'"\s][^'"]*)['"]\s*\)"#
);
pattern!(
  PERL_OPEN_THREE,
  r#"\bopen\s*\(\s*(?:my\s+)?[\w$]+\s*,\s*['"](?:>>?|\+[<>])['"]\s*,\s*([^,()]+)"#
);
pattern!(
  WRITE_TEXT,
  r#"([A-Za-z_][\w.]*(?:\(\s*['"][^'"]+['"]\s*\))?)\s*\.\s*write_(?:text|bytes)\s*\("#
);
pattern!(
  WRITE_FILE,
  r"(?:Bun\.write|(?:fs\.)?writeFileSync|(?:fs\.)?writeFile|File\.write|IO\.write|\.writeFile)\s*\(\s*([^,()]+)"
);
pattern!(LITERAL, r#"^(?:[A-Za-z_][\w.]*)?\(?\s*['"]([^'"]+)['"]\s*\)?$"#);
pattern!(NAME, r"^[A-Za-z_]\w*$");
pattern!(QUOTED, r#"['"]([^'"]+)['"]"#);

fn write_calls(script: &str) -> WriteCalls {
  let mut calls = WriteCalls {
    paths: vec![],
    unresolved: false,
  };
  let add = |calls: &mut WriteCalls, expr: &str| {
    let named = named_paths(expr, script);

    calls.unresolved |= named.is_empty();
    calls.paths.extend(named);
  };

  // Python's `open(<file>, '<mode>')`, a write where the mode writes, appends or updates.
  for call in PY_OPEN.captures_iter(script) {
    if PY_WRITING_MODE.is_match(&call[2]) {
      add(&mut calls, &call[1]);
    }
  }

  // Perl's `open(F, ">file")`, and its three-argument `open(F, ">", <file>)`.
  for call in PERL_OPEN_TWO.captures_iter(script) {
    calls.paths.push(call[1].to_owned());
  }

  for call in PERL_OPEN_THREE.captures_iter(script) {
    add(&mut calls, &call[1]);
  }

  for call in WRITE_TEXT.captures_iter(script) {
    add(&mut calls, &call[1]);
  }

  for call in WRITE_FILE.captures_iter(script) {
    add(&mut calls, &call[1]);
  }

  calls
}

// A path a write call named: a literal as it stands, or every literal the script gave that name — by an
// assignment, or by the list a loop walks.
fn named_paths(expr: &str, script: &str) -> Vec<String> {
  let trimmed = expr.trim();

  if let Some(literal) = LITERAL.captures(trimmed) {
    return vec![literal[1].to_owned()];
  }

  if !NAME.is_match(trimmed) {
    return vec![];
  }

  let mut found = vec![];
  let given = Regex::new(&format!(r#"\b{trimmed}\s*=\s*(?:[A-Za-z_][\w.]*)?\(?\s*['"]([^'"]+)['"]"#));
  let walked = Regex::new(&format!(r"for\s+{trimmed}\s+in\s*[\[(]([^\])]*)[\])]"));

  if let Ok(given) = given {
    found.extend(given.captures_iter(script).map(|given| given[1].to_owned()));
  }

  if let Ok(walked) = walked {
    for list in walked.captures_iter(script) {
      found.extend(QUOTED.captures_iter(&list[1]).map(|item| item[1].to_owned()));
    }
  }

  found
}

// The files a text tool was reading, apart from the expression the tool itself takes as its first word. A
// tool reading a file is transforming it into another one, and no verb of this repository does that, so
// such a redirect stands.
fn read_files(program: &str, args: &[String]) -> Vec<String> {
  let plain: Vec<String> = args.iter().filter(|arg| !arg.starts_with('-')).cloned().collect();

  match program {
    "tr" => vec![],
    "sed" | "awk" | "jq" => plain.into_iter().skip(1).collect(),
    _ => plain,
  }
}

/// Why a command would write a file of this checkout, or nothing. The reading follows a `cd`, the text a
/// shell is handed with `-c` and the text given to `eval`, and a heredoc's body travels with them, since
/// that is where an interpreter's script and a `cat`'s text both stand.
pub fn checkout_write(command: &str, root: &str, home: &str) -> Option<CheckoutWrite> {
  checkout_write_at(command, root, home, 0, &[])
}

fn checkout_write_at(
  command: &str,
  root: &str,
  home: &str,
  depth: usize,
  inherited: &[Heredoc],
) -> Option<CheckoutWrite> {
  let text = expand_assigned(command);
  let read = read_command(&text);
  let heredocs: Vec<Heredoc> = read.heredocs.iter().chain(inherited).cloned().collect();
  // The worker's own shell stands in the checkout, which is what makes a relative path one of its files
  // until a `cd` says otherwise.
  let mut inside = true;
  // A write seen but not named, kept in case nothing nameable turns up.
  let mut unnamed: Option<CheckoutWrite> = None;

  for segment in &read.commands {
    let Some(run) = program_of(&segment.words) else {
      continue;
    };
    let program = run.program.as_str();

    if program == "cd" || program == "pushd" {
      inside = moved_inside(&run.args, root, home, inside);
      continue;
    }

    let named = |word: &String| inside_checkout(word, root, home, inside);
    let fed_by_heredoc = heredocs.iter().any(|held| held.opener.contains(&run.word));

    if IN_PLACE_EDITORS.contains(&program) && run.args.iter().any(|arg| in_place_flag(arg)) {
      let (script, files) = in_place_parts(program, &run.args);

      if let Some(path) = files.iter().find(|file| named(file)) {
        return Some(CheckoutWrite {
          shape: expression_shape(&script),
          means: Means::InPlace,
          path: path.clone(),
          word: run.word.clone(),
        });
      }
    }

    if INTERPRETERS.contains(&program) {
      let mut scripts = inline_scripts(program, &run.args);

      if reads_script_from_stdin(&run.args) {
        scripts.extend(
          heredocs
            .iter()
            .filter(|held| held.opener.contains(&run.word))
            .map(|held| held.text.clone()),
        );
      }

      for script in &scripts {
        if let Some(path) = write_calls(script).paths.iter().find(|path| named(path)) {
          let found = CheckoutWrite {
            shape: script_shape(script),
            means: Means::Script,
            path: path.clone(),
            word: run.word.clone(),
          };

          // A write whose kind the script does not show names no verb, so the reading goes on: a command
          // holding one of those and a nameable edge beside it is refused for the edge.
          if found.shape != WriteShape::Unread {
            return Some(found);
          }

          unnamed = unnamed.or(Some(found));
        }
      }
    }

    if TEXT_TOOLS.contains(&program) {
      let target = if program == "tee" {
        run.args.iter().find(|arg| !arg.starts_with('-') && named(arg))
      } else {
        segment.redirects.iter().find(|target| named(target))
      };
      let carries = program == "echo"
        || program == "printf"
        || (fed_by_heredoc && (program == "tee" || read_files(program, &run.args).is_empty()));

      if let Some(target) = target
        && carries
      {
        let appends = if program == "tee" {
          run.args.iter().any(|arg| arg == "-a")
        } else {
          segment.appended.contains(target)
        };

        return Some(CheckoutWrite {
          shape: if appends { WriteShape::Append } else { WriteShape::Whole },
          means: Means::Redirect,
          path: target.clone(),
          word: run.word.clone(),
        });
      }
    }

    // A shell handed a command string, and the text given to `eval`, read as commands of their own.
    if depth < INNER_TEXT_DEPTH && is_shell_or_eval(&run) {
      let found =
        handed_text(&run).and_then(|inner| checkout_write_at(&inner, root, home, depth + 1, &heredocs));

      if let Some(found) = &found
        && found.shape != WriteShape::Unread
      {
        return Some(found.clone());
      }

      unnamed = unnamed.or(found);
    }
  }

  unnamed
}

/// What the command was doing, in the line a refusal opens with.
pub fn what_of(write: &CheckoutWrite) -> String {
  let path = &write.path;
  let doing = match write.shape {
    WriteShape::Phrase => format!("it swaps a phrase in {path}"),
    WriteShape::Block => format!("it replaces a block of {path}"),
    WriteShape::Pattern => format!("it runs a pattern substitution over {path}"),
    WriteShape::Anchors => format!("it replaces what stands between two anchors in {path}"),
    WriteShape::Lines => format!("it replaces lines of {path} named by number"),
    WriteShape::Cut => format!("it cuts lines out of {path}"),
    WriteShape::Insert => format!("it puts text into {path}"),
    WriteShape::Append => format!("it appends to {path}"),
    WriteShape::Whole => format!("it writes the whole of {path}"),
    WriteShape::Unread => format!("it writes {path}"),
  };
  let by = match write.means {
    Means::Script => "with a script written on the command line",
    Means::InPlace => "with an in-place editor",
    Means::Redirect => "with a redirect",
  };

  format!("{doing} {by}")
}

pattern!(GRAPH_FILE, r"(?:^|/)graph/");

/// The one call that does that work, or nothing where no verb of this repository does it — and then no
/// refusal is made at all.
pub fn verb_for(write: &CheckoutWrite) -> Option<String> {
  // A write this reading can see and cannot name has no call to hand back, and a refusal that cannot hand
  // one back is not made.
  if write.shape == WriteShape::Unread {
    return None;
  }

  // A node of the graph is not a phrase in a file: `graph/*.json` is written through the graph tool, which
  // validates and serialises what it writes, and never by editing the file.
  if GRAPH_FILE.is_match(&write.path) {
    return Some("bmo graph update <node> --replace '<before>' '<after>'".into());
  }

  let path = &write.path;

  Some(match write.shape {
    WriteShape::Phrase => format!("bmo edit {path} --replace '<before>' '<after>'"),
    WriteShape::Block => {
      format!("bmo edit {path} --stdin <<'EOF'\n@@from\n<before>\n@@to\n<after>\nEOF")
    }
    WriteShape::Pattern => format!("bmo edit {path} --replace-regex '<pattern>' '<after>'"),
    WriteShape::Anchors => format!("bmo edit {path} --between '<start>' '<end>' '<after>'"),
    WriteShape::Lines => format!("bmo edit {path} --replace-lines <a>-<b> '<after>'"),
    WriteShape::Cut => format!("bmo edit {path} --delete-lines <a>-<b>"),
    WriteShape::Insert => format!("bmo edit {path} --insert-after '<anchor>' '<text>'"),
    WriteShape::Append => format!("bmo edit {path} --insert-at <line> '<text>'"),
    WriteShape::Whole => format!("the Write tool on {path}, which writes a whole file whole"),
    WriteShape::Unread => return None,
  })
}

/// The refusal itself: what was read, the call that does it, and what this does not refuse.
pub fn checkout_write_message(write: &CheckoutWrite, verb: &str) -> String {
  format!(
    "refused: {}, read in {}. This repository has a verb for that:\n{verb}\nA write outside the checkout, a read, a pipeline that writes nothing and a committed script are not refused, and `Edit` and `Write` stand as they are (docs/verbs.md, \"The shell hook\").",
    what_of(write),
    serde_json::to_string(&write.word).unwrap_or_default(),
  )
}

/// Why this command may not write a file of the checkout, or nothing. A shape no verb answers goes through:
/// the refusal exists to hand back the call, and it is not made where it cannot.
pub fn checkout_write_refusal(command: &str, root: &str, home: &str) -> Option<String> {
  let write = checkout_write(command, root, home)?;
  let verb = verb_for(&write)?;

  Some(checkout_write_message(&write, &verb))
}
