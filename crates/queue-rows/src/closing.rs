//! What the closing of an item reads and settles beside the row itself (docs/rows.md, "The closing's
//! readings"): the strands of its unfinished note and whose each is, the improvement a report or a strand
//! proposes instead of making, a check left undone, a page shown outside the archive, the live-Claude
//! openings carried into the report, the notes a closing settles or opens again, and the agent's strands
//! handed over to it.

use std::sync::LazyLock;

use job_base::{Refusal, all, run};
use job_brain::MemoryAdd;
use job_codes::TodoRefusal;
use job_policy::Names;
use job_policy::closing::{Closing, Pattern};
use queue_read::worker_lines::without_fences;
use regex::Regex;
use sea_orm::ConnectionTrait;

use crate::blocks::PickedStrand;
use crate::instructions::{Order, queue_order};
use crate::wording::QUOTED_SPAN;
use crate::{Ground, js, refused, store};

/// The heading the live-Claude openings stand under in a report.
pub const LIVE_CLAUDE_HEADING: &str = "Measured runs (kept by the live-Claude guard)";

// The longest action a memory holds, in UTF-16 units.
const MEMORY_ACTION_CAP: usize = 2_000;

// One of the policy's patterns compiled with its flags.
fn compiled(pattern: &str, flags: &str) -> Regex {
  let prefix = if flags.contains('i') { "(?i)" } else { "" };

  Regex::new(&format!("{prefix}{pattern}")).expect("a pattern of the closing's policy compiles")
}

fn each(patterns: &[Pattern]) -> Vec<Regex> {
  patterns.iter().map(|one| compiled(&one.pattern, &one.flags)).collect()
}

/// The closing's patterns and words, compiled once from the policy and the deployment's names.
pub struct Reader {
  opener: Regex,
  mark: Regex,
  owner_only: Regex,
  raised_row: Regex,
  host_work: Regex,
  beyond_reach: Regex,
  proposing: Vec<Regex>,
  unchecked: Vec<Regex>,
  set_aside: Regex,
  latin_per_hangul: usize,
  agent: String,
  owner: String,
  agent_mark: String,
  owner_mark: String,
  unfinished: String,
  left_headings: Vec<String>,
  unchecked_refusal: String,
}

impl Reader {
  pub fn new(closing: &Closing, names: &Names) -> Self {
    let words = &closing.words;

    Self {
      opener: compiled(&closing.strands.opener, ""),
      mark: compiled(&closing.strands.mark, ""),
      owner_only: compiled(&words.owner_only, &closing.leftovers.owner_only_flags),
      raised_row: compiled(&words.raised_row, &closing.leftovers.raised_row_flags),
      host_work: compiled(&words.host_work, &closing.strands.host_work_flags),
      beyond_reach: compiled(&words.beyond_reach, &closing.unchecked.beyond_reach_flags),
      proposing: each(&words.proposing),
      unchecked: each(&words.unchecked),
      set_aside: compiled(&closing.english.set_aside, ""),
      latin_per_hangul: closing.english.latin_per_hangul,
      agent: names.agent.clone(),
      owner: names.owner.clone(),
      agent_mark: closing.strands.agent_mark.clone(),
      owner_mark: closing.strands.owner_mark.clone(),
      unfinished: words.unfinished.clone(),
      left_headings: words.left_headings.clone(),
      unchecked_refusal: job_policy::fill_marks(&closing.unchecked.refusal, &[("owner", &names.owner)]),
    }
  }

  /// What the note listing a closed item's open strands is called.
  pub fn unfinished_word(&self) -> &str {
    &self.unfinished
  }
}

/// Whose a strand of an unfinished note is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
  Agent,
  Person,
}

/// One strand of an unfinished note: its words, and whose it is where its mark says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Strand {
  pub text: String,
  pub owner: Option<Owner>,
}

impl Reader {
  fn owner_of(&self, text: &str) -> (Option<Owner>, String) {
    match self.mark.captures(text) {
      None => (None, text.to_owned()),
      Some(found) => {
        let owner = if found[1] == self.agent { Owner::Agent } else { Owner::Person };

        (Some(owner), js::trim(&text[found.get(0).map_or(0, |whole| whole.end())..]).to_owned())
      }
    }
  }

  /// The strands of one unfinished note: a listed note read as its list, whatever stands above the first
  /// strand its lead; a note that lists nothing one strand whole.
  pub fn unfinished_strands(&self, note: Option<&str>) -> Vec<Strand> {
    let note = note.unwrap_or_default().replace("\r\n", "\n");
    let mut found: Vec<String> = vec![];
    let mut current: Option<Vec<String>> = None;
    let mut lead: Vec<&str> = vec![];

    for line in note.split('\n') {
      if self.opener.is_match(line) {
        if let Some(done) = current.take() {
          found.push(done.join("\n"));
        }

        current = Some(vec![self.opener.replace(line, "").into_owned()]);
      } else if let Some(open) = current.as_mut() {
        open.push(line.to_owned());
      } else {
        lead.push(line);
      }
    }

    if let Some(done) = current {
      found.push(done.join("\n"));
    }

    let whole = if found.is_empty() { vec![lead.join("\n")] } else { found };

    whole
      .iter()
      .map(|strand| js::trim(strand).to_owned())
      .filter(|strand| !strand.is_empty())
      .map(|strand| {
        let (owner, rest) = self.owner_of(&strand);

        Strand {
          text: if rest.is_empty() { strand } else { rest },
          owner,
        }
      })
      .collect()
  }

  /// The strands of a note that carry no mark of whose they are.
  pub fn unmarked_strands(&self, note: Option<&str>) -> Vec<String> {
    self
      .unfinished_strands(note)
      .into_iter()
      .filter(|strand| strand.owner.is_none())
      .map(|strand| strand.text)
      .collect()
  }

  /// A strand with the mark it was written with put back.
  pub fn marked_strand(&self, strand: &Strand) -> String {
    match strand.owner {
      None => strand.text.clone(),
      Some(Owner::Agent) => format!("{} {}", self.agent_mark, strand.text),
      Some(Owner::Person) => format!("{} {}", self.owner_mark, strand.text),
    }
  }

  /// A note cut into its strands as written, the lead above the first strand a piece of its own; joined
  /// with line breaks, the pieces are the note.
  pub fn strand_pieces(&self, note: &str) -> Vec<StrandPiece> {
    let mut groups: Vec<Vec<&str>> = vec![];

    for line in note.split('\n') {
      match groups.last_mut() {
        Some(last) if !self.opener.is_match(line) => last.push(line),
        _ => groups.push(vec![line]),
      }
    }

    groups
      .into_iter()
      .map(|group| {
        let text = group.join("\n");
        let opener = self.opener.find(&text).map_or(0, |found| found.as_str().len());
        let mark = self.mark.find(&text[opener..]).map_or(0, |found| found.as_str().len());
        let (syntax, words) = text.split_at(opener + mark);

        StrandPiece {
          syntax: syntax.to_owned(),
          words: words.to_owned(),
        }
      })
      .collect()
  }

  /// The strand of a note a row is raised out of, by its number; no number stands only for a note of a
  /// single strand.
  pub fn pick_strand(
    &self,
    todo_id: i64,
    title: &str,
    note: &str,
    strand: Option<usize>,
  ) -> Result<PickedStrand, Refusal> {
    let strands = self.unfinished_strands(Some(note));
    let count = u32::try_from(strands.len()).unwrap_or(u32::MAX);
    let picked = match strand {
      None if strands.len() == 1 => Some((1, &strands[0])),
      None => {
        return Err(refused(TodoRefusal::StrandNotNamed {
          id: todo_id,
          strands: count,
        }));
      }
      Some(number) => number.checked_sub(1).and_then(|at| strands.get(at)).map(|found| (number, found)),
    };

    match picked {
      Some((number, found)) => Ok(PickedStrand {
        todo_id,
        title: title.to_owned(),
        strand: number,
        text: self.marked_strand(found),
      }),
      None => Err(refused(TodoRefusal::NoSuchStrand {
        id: todo_id,
        strand: strand.and_then(|number| u32::try_from(number).ok()).unwrap_or(u32::MAX),
        strands: count,
      })),
    }
  }

  /// Why an unfinished note written with strands that say nobody's is refused, naming them.
  pub fn strand_format_refusal(&self, unmarked: &[String]) -> String {
    let (unfinished, owner) = (&self.unfinished, &self.owner);

    std::iter::once(format!(
      "an {unfinished} says whose each strand is. Number the strands, one to a line, and open each with {} for the host work no repository's service owns (a machine or OS setting, a mount, a backup, the router, a package tied to no service) or {} for what only {owner} can give (their password, an account of theirs, their own taste), never host work. What a repository owns is no {unfinished} at all, its own service's infrastructure with it (the name it answers on, its DNS record, its proxy host, the unit that runs it): that is the job's own work, and separate work found there is raised as a row of its own.",
      self.agent_mark, self.owner_mark
    ))
    .chain(std::iter::once("These strands carry neither mark:".to_owned()))
    .chain(listed(unmarked))
    .collect::<Vec<_>>()
    .join("\n")
  }

  /// The owner's strands of a note that read as host work and name nothing only the owner can give.
  pub fn owner_host_strands(&self, note: Option<&str>) -> Vec<String> {
    self
      .unfinished_strands(note)
      .into_iter()
      .filter(|strand| strand.owner == Some(Owner::Person))
      .map(|strand| strand.text)
      .filter(|text| {
        self.host_work.is_match(text)
          && !self.owner_only.is_match(text)
          && !self.beyond_reach.is_match(text)
      })
      .collect()
  }

  /// Why an unfinished note that hands host work to the owner is refused, naming the strands.
  pub fn owner_host_refusal(&self, strands: &[String]) -> String {
    let owner = &self.owner;

    std::iter::once(format!(
      "host work is never {owner}'s strand. Work done on a machine (a setting, a package, a unit, a mount, a backup, the router, whatever sudo on this machine or a node's login reaches) is {} with the exact steps, even where it needs root, a restart or {owner}'s word first: the agent asks for the word and does it. The infrastructure this repository's own service stands on is the job's own work, done now. A strand stays {} only where it names what only {owner} can give: a password or an account of theirs, a judgement of their own taste, a hand on hardware no machine here reaches.",
      self.agent_mark, self.owner_mark
    ))
    .chain(std::iter::once(format!("These strands of {} are host work:", self.owner_mark)))
    .chain(listed(strands))
    .collect::<Vec<_>>()
    .join("\n")
  }

  /// Whether a line proposes or defers an improvement instead of making it.
  pub fn proposes(&self, line: &str) -> bool {
    if self.owner_only.is_match(line) || self.raised_row.is_match(line) {
      return false;
    }

    self.proposing.iter().any(|pattern| pattern.is_match(line))
  }

  /// The strands of a report's section of what the job left, under its heading.
  pub fn left_section(&self, report: Option<&str>) -> Vec<String> {
    let report = report.unwrap_or_default().replace("\r\n", "\n");
    let lines: Vec<&str> = report.split('\n').collect();
    let Some(start) = lines.iter().position(|line| self.left_headings.contains(&bare(line))) else {
      return vec![];
    };
    let mut body: Vec<&str> = vec![];

    for at in start + 1..lines.len() {
      let line = lines[at];

      if js::trim(line).is_empty() {
        match lines[at + 1..].iter().find(|one| !js::trim(one).is_empty()) {
          Some(next) if CONTINUES.is_match(next) => continue,
          _ => break,
        }
      }

      body.push(line);
    }

    self
      .unfinished_strands(Some(&body.join("\n")))
      .into_iter()
      .map(|strand| strand.text)
      .collect()
  }

  /// Why a closing whose section of what it left or whose unfinished note proposes an improvement instead
  /// of making it is refused, or none.
  pub fn leftover_refusal(&self, report: Option<&str>, unfinished: Option<&str>) -> Option<String> {
    let left: Vec<String> =
      self.left_section(report).into_iter().filter(|line| self.proposes(line)).collect();
    let strands: Vec<String> = self
      .unfinished_strands(unfinished)
      .into_iter()
      .map(|strand| strand.text)
      .filter(|line| self.proposes(line))
      .collect();

    if left.is_empty() && strands.is_empty() {
      return None;
    }

    let (owner, unfinished) = (&self.owner, &self.unfinished);
    let mut lines = vec![format!(
      "what a job leaves is only what its hands could not reach, and an improvement never is. Make an improvement inside the item now; raise one beyond it as its own row (the TODO's add verb with a title, a judgement and the item that raised it) and name that row by its number; leave with {owner} only what only {owner} can give (a password, an account of theirs, a judgement of their own taste). A fact the next reader needs, such as a commit waiting to go live, is said as a fact rather than as a suggestion."
    )];

    if !left.is_empty() {
      let heading = self.left_headings.first().map_or("Left", String::as_str);

      lines.push(format!("These lines of {heading} propose or defer an improvement:"));
      lines.extend(listed(&left));
    }

    if !strands.is_empty() {
      lines.push(format!("These strands of the {unfinished} propose or defer an improvement:"));
      lines.extend(listed(&strands));
    }

    Some(lines.join("\n"))
  }

  /// The lines of a text that say a check was left undone, outside their quoted spans, with nothing putting
  /// the check beyond a job's reach; each trimmed and without its list opener, in the text's order.
  pub fn unchecked_lines(&self, text: &str) -> Vec<String> {
    text
      .split('\n')
      .filter(|line| {
        let bare = QUOTED_SPAN.replace_all(line, " ");

        self.unchecked.iter().any(|pattern| pattern.is_match(&bare)) && !self.beyond_reach.is_match(&bare)
      })
      .map(|line| self.opener.replace(js::trim(line), "").into_owned())
      .collect()
  }

  /// Why a closing whose report or unfinished note leaves a check undone is refused, or none.
  pub fn unchecked_refusal(&self, report: Option<&str>, unfinished: Option<&str>) -> Option<String> {
    let lines: Vec<String> = [report, unfinished]
      .into_iter()
      .flatten()
      .flat_map(|text| self.unchecked_lines(text))
      .collect();

    if lines.is_empty() {
      return None;
    }

    Some(
      std::iter::once(self.unchecked_refusal.clone())
        .chain(lines.iter().enumerate().map(|(at, line)| format!("  {}. {line}", at + 1)))
        .collect::<Vec<_>>()
        .join("\n"),
    )
  }

  /// Whether a text is a worker's English, by the closing's own reading of English.
  pub fn written_in_english(&self, text: &str) -> bool {
    reads_as_english(&self.set_aside, self.latin_per_hangul, text)
  }

  /// The resolution an unfinished note is settled with once every row it was broken into is finished.
  pub fn settled(&self, rows: &[(i64, String)]) -> String {
    let named: Vec<String> = rows.iter().map(|(id, title)| format!("#{id} {title}")).collect();

    format!(
      "Settled: every row raised out of this {} is finished: {}.",
      self.unfinished,
      named.join(", ")
    )
  }

  /// The instruction a strand handed to the agent is told with.
  fn strand_instruction(&self, id: i64, title: &str, strand: &str, memory: i64, chat: i64) -> String {
    let (owner, unfinished) = (&self.owner, &self.unfinished);

    [
      format!(
        "Item #{id} {title} has been closed, and its {unfinished} marked this strand as yours: work outside that repository, which no job may do and nobody else will pick up."
      ),
      String::new(),
      "What the job left, in its own words:".to_owned(),
      strand.to_owned(),
      String::new(),
      format!(
        "It is already among your memories for this chat as #{memory}, so it stands in front of you every turn until you check it off."
      ),
      format!(
        "Do it now with your own hands where it can be done now. Where it needs something only {owner} holds (a password, an account of theirs, a judgement of their taste), ask for that one thing and go on holding the item. Where reading it shows the work is inside a repository after all, file it as a row instead and check the memory off saying which row took it."
      ),
      format!("Check it off however it ends: memory_done with chat {chat}, memory {memory} and text '<how it ended>'."),
      "Do not announce the instruction itself. What you say in the chat is about the work you did.".to_owned(),
    ]
    .join("\n")
  }
}

/// One strand of a note as it is written: the number or bullet it opens with and its mark of whose it is,
/// and the words after them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrandPiece {
  pub syntax: String,
  pub words: String,
}

fn first_line(text: &str) -> &str {
  text.split('\n').next().unwrap_or_default()
}

// The lines a refusal names, numbered, each by its first line.
fn listed(lines: &[String]) -> impl Iterator<Item = String> + '_ {
  lines.iter().enumerate().map(|(at, line)| format!("  {}. {}", at + 1, first_line(line)))
}

static BARE_MARKS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[*_#`]").expect("a heading's marks"));
static TRAILING_COLON: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"[:：]\s*$").expect("a heading's colon"));
static CONTINUES: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"^\s+\S|^\s*(\d+[.)]|[-*•])\s+").expect("a line that carries a list on"));
static INLINE_CODE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"`[^`\n]*`").expect("inline code"));
static LINK: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://\S+").expect("a link"));

/// Whether a text reads as English by a part's reading of English (docs/policy.md): code and links read
/// past, the words `set_aside` matches read past, and the Hangul syllables left read against the Latin
/// letters left, `latin_per_hangul` to each, or none at all where it is 0.
pub fn reads_as_english(set_aside: &Regex, latin_per_hangul: usize, text: &str) -> bool {
  let prose = without_fences(text);
  let prose = INLINE_CODE.replace_all(&prose, "");
  let prose = LINK.replace_all(&prose, "");
  let prose = set_aside.replace_all(&prose, "");
  let hangul = prose.chars().filter(|character| ('\u{AC00}'..='\u{D7A3}').contains(character)).count();

  if latin_per_hangul == 0 {
    return hangul == 0;
  }

  let latin = prose.chars().filter(char::is_ascii_alphabetic).count();

  latin > 0 && hangul * latin_per_hangul < latin
}

fn bare(line: &str) -> String {
  let unmarked = BARE_MARKS.replace_all(line, "");

  js::trim(&TRAILING_COLON.replace(&unmarked, "")).to_owned()
}

static OUTSIDE: LazyLock<Vec<Regex>> = LazyLock::new(|| {
  [
    r#"(?i)https?://(?:www\.)?claude\.ai/(?:[\w-]+/)*artifacts?/[^\s)>\]'"`]+"#,
    r#"(?i)https?://(?:[\w-]+\.)*claude\.site(?:/[^\s)>\]'"`]*)?"#,
    r#"(?i)https?://(?:[\w-]+\.)*claudeusercontent\.com(?:/[^\s)>\]'"`]*)?"#,
    r#"(?i)https?://gist\.github(?:usercontent)?\.com/[^\s)>\]'"`]+"#,
    r#"(?i)https?://(?:www\.)?pastebin\.com/[^\s)>\]'"`]+"#,
  ]
  .map(|pattern| Regex::new(pattern).expect("an outside host's address"))
  .to_vec()
});

fn outside_addresses(text: &str) -> Vec<String> {
  let mut found: Vec<(usize, String)> = OUTSIDE
    .iter()
    .flat_map(|pattern| {
      pattern
        .find_iter(text)
        .map(|one| (one.start(), one.as_str().trim_end_matches(['.', ',', ';', ':', '!', '?']).to_owned()))
        .collect::<Vec<_>>()
    })
    .collect();

  found.sort_by_key(|(at, _)| *at);

  let mut addresses: Vec<String> = vec![];

  for (_, address) in found {
    if !addresses.contains(&address) {
      addresses.push(address);
    }
  }

  addresses
}

/// Why a closing that hands the owner a page hosted outside the archive is refused, or none.
pub fn outside_hosting_refusal(texts: &[Option<&str>], owner: &str) -> Option<String> {
  let mut addresses: Vec<String> = vec![];

  for address in texts.iter().flatten().flat_map(|text| outside_addresses(text)) {
    if !addresses.contains(&address) {
      addresses.push(address);
    }
  }

  if addresses.is_empty() {
    return None;
  }

  Some(
    std::iter::once(format!(
      "a document shown to {owner} goes into the deployment's own archive, never onto a page hosted outside it. File it with the archive's document verb, and put the console address it answers with where this address stands. These addresses open a page outside the archive:"
    ))
    .chain(addresses.iter().enumerate().map(|(at, address)| format!("  {}. {address}", at + 1)))
    .collect::<Vec<_>>()
    .join("\n"),
  )
}

/// The openings an item's jobs made through the live-Claude guard, each with what it was for and how many
/// runs it let through.
pub async fn live_claude_openings(
  db: &impl ConnectionTrait,
  todo: i64,
) -> Result<Vec<(String, usize)>, Refusal> {
  let openings = all(
    db,
    r#"SELECT "id", "purpose" FROM "live_claude_openings" WHERE "todo_id" = $1 ORDER BY "id""#,
    vec![todo.into()],
  )
  .await?;
  let mut found = vec![];

  for opening in openings {
    let id: i64 = opening.try_get("", "id")?;
    let runs =
      all(db, r#"SELECT "id" FROM "live_claude_runs" WHERE "opening_id" = $1"#, vec![id.into()]).await?;

    found.push((opening.try_get("", "purpose")?, runs.len()));
  }

  Ok(found)
}

/// An item's report with the openings its jobs made carried into it; a report of an item whose jobs opened
/// nothing is left as it was, apart from a section an earlier closing wrote.
pub fn report_with_live_claude(report: Option<&str>, openings: &[(String, usize)]) -> Option<String> {
  let own = report.map(|report| {
    js::trim_end(report.find(LIVE_CLAUDE_HEADING).map_or(report, |at| &report[..at])).to_owned()
  });

  if openings.is_empty() {
    return own.filter(|own| !own.is_empty());
  }

  let section = std::iter::once(LIVE_CLAUDE_HEADING.to_owned())
    .chain(
      openings
        .iter()
        .enumerate()
        .map(|(at, (purpose, runs))| format!("{}. {purpose}: {runs} real Claude runs", at + 1)),
    )
    .collect::<Vec<_>>()
    .join("\n");

  Some(match own {
    Some(own) if !own.is_empty() => format!("{own}\n\n{section}"),
    _ => section,
  })
}

/// The report an item merged into `target` is closed with when that item is.
pub fn merged_report(target: i64) -> String {
  format!("Closed with #{target}, which it was merged into; how it was done is in the report of #{target}.")
}

/// Every note its own rows now settle, resolved, with what resolved each.
pub async fn resolve_settled_sources(
  db: &impl ConnectionTrait,
  reader: &Reader,
  sources: &[i64],
  now: i64,
) -> Result<Vec<(i64, String)>, Refusal> {
  let mut resolved = vec![];

  for &source in sources {
    let raised = store::raised_from(db, source).await?;

    if raised.is_empty() || raised.iter().any(|(_, _, done)| !done) {
      continue;
    }

    let rows: Vec<(i64, String)> = raised.into_iter().map(|(id, title, _)| (id, title)).collect();
    let resolution = reader.settled(&rows);

    if crate::todos::resolve_unfinished(db, source, &resolution, now).await? {
      resolved.push((source, resolution));
    }
  }

  Ok(resolved)
}

/// Every note a row now open again leaves outstanding, its resolution taken back; the numbers of those.
pub async fn reopen_settled_sources(
  db: &impl ConnectionTrait,
  sources: &[i64],
) -> Result<Vec<i64>, Refusal> {
  let mut reopened = vec![];

  for &source in sources {
    let raised = store::raised_from(db, source).await?;
    let settled = !raised.is_empty() && raised.iter().all(|(_, _, done)| *done);

    if !settled
      && run(
        db,
        r#"UPDATE "todos" SET "unfinished_resolved_at" = NULL, "unfinished_resolution" = NULL WHERE "id" = $1 AND "unfinished_resolved_at" IS NOT NULL"#,
        vec![source.into()],
      )
      .await?
        > 0
    {
      reopened.push(source);
    }
  }

  Ok(reopened)
}

/// The action a strand handed to the agent is kept as among its memories.
pub fn strand_memory_action(strand: &Strand) -> String {
  if js::units(&strand.text) <= MEMORY_ACTION_CAP {
    return strand.text.clone();
  }

  js::cut(&strand.text, MEMORY_ACTION_CAP - 1)
}

/// Every strand of an item's unfinished note that is the agent's own handed to it: one memory each in the
/// owner's chat through bmo-brain, tied to the item, and one quiet instruction each; a strand the item handed
/// over already is left alone. What was handed, each memory's number beside its action.
pub async fn hand_over(
  ground: &Ground<'_>,
  id: i64,
  title: &str,
  unfinished: Option<&str>,
) -> Result<Vec<(i64, String)>, Refusal> {
  let chat = ground.owner_chat;
  let mut already: Vec<String> = ground
    .units
    .memories(chat)
    .await?
    .into_iter()
    .filter(|memory| memory.todo_id == Some(id))
    .map(|memory| memory.action)
    .collect();
  let mut handed = vec![];

  for strand in ground.reader.unfinished_strands(unfinished) {
    if strand.owner != Some(Owner::Agent) {
      continue;
    }

    let action = strand_memory_action(&strand);

    if already.contains(&action) {
      continue;
    }

    already.push(action.clone());
    ground
      .units
      .add_memory(&MemoryAdd {
        kind: "followup".to_owned(),
        action: action.clone(),
        todo: Some(id),
        chat: Some(chat),
        slug: None,
      })
      .await?;

    // bmo-brain answers a filing with its lines alone, so the memory's number is read back from the room.
    let memory = ground
      .units
      .memories(chat)
      .await?
      .into_iter()
      .filter(|memory| memory.todo_id == Some(id) && memory.action == action)
      .map(|memory| memory.id)
      .max()
      .unwrap_or_default();

    queue_order(
      ground.units,
      Order {
        chat: Some(chat),
        text: ground.reader.strand_instruction(id, title, &action, memory, chat),
        sort: "unfinished",
        summary: Some(format!("#{id} {title}")),
        events: vec![],
      },
    )
    .await?;
    handed.push((memory, action));
  }

  Ok(handed)
}
