//! The terms the owner has set aside, found in a text outside the spans it quotes: what a filing's title and
//! parts are held to, and what a job's report is named under for. The glossary itself is the deployment's:
//! read here once out of the owner's wording document its settings name (docs/settings.md), and handed to
//! each caller.

use std::sync::LazyLock;

use job_base::Settings;
use pulldown_cmark::{Event, HeadingLevel, Parser, Tag, TagEnd};
use regex::Regex;

use crate::js::units;

/// A span a written text names a wrong form or quotes a command or somebody's words in: inside backticks or
/// a pair of quotation marks, on one line.
pub static QUOTED_SPAN: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r#"`[^`\n]*`|"[^"\n]*"|'[^'\n]*'|“[^”\n]*”|‘[^’\n]*’|「[^」\n]*」|『[^』\n]*』"#)
    .expect("a quoted span")
});

/// A term the owner has set aside: the language its line is in, the term, what to write instead, and the
/// phrases in which it keeps its own sense and passes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BannedTerm {
  pub lang: String,
  pub term: String,
  pub instead: String,
  pub keeps: Vec<String>,
}

// Whether a character is a Hangul jamo, compatibility jamo or syllable.
fn hangul(character: char) -> bool {
  matches!(character, '\u{1100}'..='\u{11FF}' | '\u{3130}'..='\u{318F}' | '\u{AC00}'..='\u{D7AF}')
}

fn hangul_syllable(character: char) -> bool {
  matches!(character, '\u{AC00}'..='\u{D7A3}')
}

/// The language a line is written in, told by its script: Korean where it holds Hangul, Japanese where it
/// holds kana alone, English otherwise.
pub fn line_language(line: &str) -> &'static str {
  let japanese = |character: char| matches!(character, '\u{3040}'..='\u{30FF}');

  if line.chars().any(hangul) {
    "ko"
  } else if line.chars().any(japanese) {
    "ja"
  } else {
    "en"
  }
}

// Whether the term stands on its own where it was found, rather than being the tail of a longer word: a
// Korean term another syllable runs into from the left belongs to that other word.
fn stands_alone(line: &str, at: usize, term: &BannedTerm) -> bool {
  if at == 0 || !term.term.chars().next().is_some_and(hangul_syllable) {
    return true;
  }

  !line[..at].chars().next_back().is_some_and(hangul_syllable)
}

// Whether the term, where it was found, is part of a phrase in which it keeps its own sense.
fn keeps_own_sense(line: &str, at: usize, term: &BannedTerm) -> bool {
  term.keeps.iter().any(|phrase| {
    phrase
      .find(&term.term)
      .and_then(|inside| at.checked_sub(inside))
      .is_some_and(|start| line.get(start..).is_some_and(|rest| rest.starts_with(phrase.as_str())))
  })
}

// Whether a line uses the term anywhere it neither runs into a longer word nor keeps its own sense.
fn used_in(line: &str, term: &BannedTerm) -> bool {
  let mut from = 0;

  while let Some(found) = line[from..].find(&term.term) {
    let at = from + found;

    if stands_alone(line, at, term) && !keeps_own_sense(line, at, term) {
      return true;
    }

    from = at + line[at..].chars().next().map_or(1, char::len_utf8);
  }

  false
}

// The sections of the wording document the terms are read out of.
const BANNED_SECTION: &str = "Banned terms";
const OWN_SENSE_SECTION: &str = "Uses in the word's own sense";

// The languages an entry may name, as a line is told to be in one by its script.
const KNOWN_LANGUAGES: [&str; 3] = ["ko", "ja", "en"];

// What separates the parts of an entry, in the document's own syntax.
const PART: &str = " · ";
const INSTEAD: &str = " → ";
const AFTER: &str = " — ";

// An entry's language and the rest of it, where the item opens on a language code at all.
fn language_of(item: &str) -> Option<(&str, &str)> {
  let (lang, rest) = item.split_once(PART)?;

  (2..=3).contains(&lang.len()).then_some(())?;
  lang.bytes().all(|byte| byte.is_ascii_lowercase()).then_some((lang, rest))
}

fn known(lang: &str, item: &str) -> Result<String, String> {
  if KNOWN_LANGUAGES.contains(&lang) {
    Ok(lang.to_owned())
  } else {
    Err(format!("unknown language code \"{lang}\" in: {item}"))
  }
}

// Every list item of the document by the level-two heading it stands under, its text joined onto one line.
fn sectioned_items(text: &str) -> Vec<(String, String)> {
  let mut items = vec![];
  let mut section = String::new();
  let mut heading: Option<String> = None;
  let mut open: Vec<String> = vec![];

  for event in Parser::new(text) {
    match event {
      Event::Start(Tag::Heading {
        level: HeadingLevel::H2,
        ..
      }) => heading = Some(String::new()),
      Event::End(TagEnd::Heading(HeadingLevel::H2)) => {
        section = heading.take().unwrap_or_default().trim().to_owned()
      }
      Event::Start(Tag::Item) => open.push(String::new()),
      Event::End(TagEnd::Item) => {
        if let Some(item) = open.pop() {
          items.push((section.clone(), item.split_whitespace().collect::<Vec<_>>().join(" ")));
        }
      }
      Event::Text(said) | Event::Code(said) => match (&mut heading, open.last_mut()) {
        (Some(heading), _) => heading.push_str(&said),
        (None, Some(item)) => item.push_str(&said),
        (None, None) => {}
      },
      Event::SoftBreak | Event::HardBreak => {
        if let Some(item) = open.last_mut() {
          item.push(' ');
        }
      }
      _ => {}
    }
  }

  items
}

/// The terms the owner has set aside, out of the wording document's text (docs/settings.md, "The owner's
/// wording document"): each banned term with what to write instead, and the phrases in which it keeps its
/// own sense. An item naming a language no line can be told in, or a phrase of a word's own sense that names
/// no banned term listed above it or does not hold it, is a fault in the document.
pub fn read_set_aside(text: &str) -> Result<Vec<BannedTerm>, String> {
  let mut banned: Vec<BannedTerm> = vec![];

  for (section, item) in sectioned_items(text) {
    let Some((lang, rest)) = language_of(&item) else { continue };

    if section == BANNED_SECTION {
      let Some((term, instead)) = rest.split_once(INSTEAD) else { continue };
      let instead = instead.split_once(AFTER).map_or(instead, |(instead, _)| instead);

      banned.push(BannedTerm {
        lang: known(lang, &item)?,
        term: term.to_owned(),
        instead: instead.to_owned(),
        keeps: vec![],
      });
    } else if section == OWN_SENSE_SECTION {
      let Some((term, phrase)) = rest.split_once(PART) else { continue };
      let Some((phrase, _)) = phrase.split_once(AFTER) else { continue };
      let lang = known(lang, &item)?;

      match banned.iter_mut().find(|one| one.lang == lang && one.term == term) {
        Some(entry) if phrase.contains(term) => entry.keeps.push(phrase.to_owned()),
        _ => {
          return Err(format!(
            "a phrase of a word's own sense must hold a banned term listed above it: {item}"
          ));
        }
      }
    }
  }

  Ok(banned)
}

/// The terms the owner has set aside, read once out of the wording document the settings name: none where
/// they name none, and none, said in the log, where the document cannot be read or does not read as the
/// list, since nothing is held to a document nobody can read.
pub fn set_aside_terms(settings: &Settings) -> Vec<BannedTerm> {
  let Some(path) = settings.deployment.wording.as_deref() else { return vec![] };
  let read = std::fs::read_to_string(path)
    .map_err(|why| why.to_string())
    .and_then(|text| read_set_aside(&text));

  read.unwrap_or_else(|why| {
    eprintln!("bmo-job: no term is set aside: the wording document {}: {why}", path.display());
    vec![]
  })
}

/// The terms a text uses outside its quoted spans, each once and in the glossary's order, each on the lines
/// of its own language.
pub fn banned_terms_in(text: &str, glossary: &[BannedTerm]) -> Vec<BannedTerm> {
  let bare = QUOTED_SPAN.replace_all(text, |found: &regex::Captures| " ".repeat(units(&found[0])));
  let lines: Vec<(&str, &str)> = bare.split('\n').map(|line| (line_language(line), line)).collect();

  glossary
    .iter()
    .filter(|term| lines.iter().any(|(lang, line)| *lang == term.lang && used_in(line, term)))
    .cloned()
    .collect()
}
