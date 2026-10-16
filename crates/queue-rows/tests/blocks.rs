//! An item's instruction composed and rewritten (docs/reading.md, "The blocks of an instruction"), and a text
//! read against the terms the owner has set aside (docs/rows.md, "The verbs of filing").

use queue_read::blocks::segments;
use queue_rows::blocks::{OwnBlock, clip_instruction, own_block_text, rewrite_own_block};
use queue_rows::wording::{BannedTerm, banned_terms_in, line_language};

fn term(lang: &str, term: &str, keeps: &[&str]) -> BannedTerm {
  BannedTerm {
    lang: lang.into(),
    term: term.into(),
    instead: "other".into(),
    keeps: keeps.iter().map(|keep| (*keep).to_owned()).collect(),
  }
}

#[test]
fn a_part_of_the_filers_own_is_read_and_written_again_under_its_own_head() {
  let instruction =
    "What to do (written by Agent as it filed the row):\n```text\nFix it.\n```\n\nLoose words.";

  assert_eq!(own_block_text(instruction, OwnBlock::ToDo).as_deref(), Some("Fix it."));
  assert_eq!(own_block_text(instruction, OwnBlock::Conclusion), None);

  let rewritten =
    rewrite_own_block(instruction, OwnBlock::ToDo, "Fix it ``` well.").expect("the part stands");

  assert!(
    rewritten
      .starts_with("What to do (written by Agent as it filed the row):\n````text\nFix it ``` well.\n````"),
    "{rewritten}"
  );
  assert!(rewritten.ends_with("\n\nLoose words."));
  assert_eq!(segments(&rewritten).len(), segments(instruction).len());
  assert_eq!(rewrite_own_block(instruction, OwnBlock::Judgement, "x"), None);
}

#[test]
fn an_instruction_is_clipped_to_one_line() {
  assert_eq!(clip_instruction("one\n  two\tthree", 60), "one two three");
  assert_eq!(clip_instruction("abcdef", 3), "abc\u{2026}");
}

#[test]
fn a_set_aside_term_is_found_outside_quotes_on_the_lines_of_its_language_where_it_keeps_no_own_sense() {
  let glossary = [term("en", "launcher", &["launcher script"]), term("en", "absent", &[])];

  assert_eq!(banned_terms_in("Fix the launcher.", &glossary), [glossary[0].clone()]);
  assert!(banned_terms_in("Fix the `launcher`, \"absent\" too.", &glossary).is_empty());
  assert!(banned_terms_in("Run the launcher script.", &glossary).is_empty());

  let korean = term("ko", "\u{AC00}\u{AC01}", &[]);
  let found = banned_terms_in("\u{AC00}\u{AC01} here", std::slice::from_ref(&korean));

  assert_eq!(found, std::slice::from_ref(&korean));
  assert!(
    banned_terms_in("\u{B098}\u{AC00}\u{AC01}", std::slice::from_ref(&korean)).is_empty(),
    "the tail of a longer word passes"
  );
  assert_eq!(
    (line_language("plain"), line_language("\u{3042}"), line_language("\u{AC00}")),
    ("en", "ja", "ko")
  );
}
