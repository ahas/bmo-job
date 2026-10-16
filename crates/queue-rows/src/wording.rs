//! The terms the owner has set aside, found in a text outside the spans it quotes: what a filing's title and
//! parts are held to. The glossary itself is the deployment's and its caller hands it over; this reads
//! a text against it.

use std::sync::LazyLock;

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
