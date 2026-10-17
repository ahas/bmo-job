//! The terms the owner has set aside, read out of the owner's wording document (docs/settings.md, "The
//! owner's wording document"), and found in a text by the lines of their own language.

use job_base::Settings;
use queue_rows::wording::{BannedTerm, banned_terms_in, read_set_aside, set_aside_terms};

const DOCUMENT: &str = "# Wording

## How this document stands

- en · ignored → because this section is not read

## Banned terms

- en · utilise → use
- en · leverage → use — when it means using a thing
  rather than lifting it
- Not an entry: a line of prose in the list.

## Uses in the word's own sense

- en · leverage · a lever's leverage — a lever does lift

## Allowed uses

- en · utilise · docs/old.md · utilise the old form — kept as a quotation
";

fn settings(wording: Option<&str>) -> Settings {
  let named = wording.map_or_else(String::new, |path| format!("wording = \"{path}\"\n"));

  Settings::read(&format!(
    "[database]\nurl = \"sqlite::memory:\"\n\n[deployment]\nowner_name = \"Owner\"\nagent_name = \"Agent\"\nagent_key = \"agent\"\nsender_name = \"Agent\"\nreceiver_unit = \"receiver\"\nowner_chat = 100\n{named}"
  ))
  .expect("the test settings read")
}

#[test]
fn the_banned_terms_and_their_own_senses_are_read_out_of_their_sections_alone() {
  assert_eq!(
    read_set_aside(DOCUMENT).unwrap(),
    vec![
      BannedTerm {
        lang: "en".into(),
        term: "utilise".into(),
        instead: "use".into(),
        keeps: vec![],
      },
      BannedTerm {
        lang: "en".into(),
        term: "leverage".into(),
        instead: "use".into(),
        keeps: vec!["a lever's leverage".into()],
      },
    ]
  );
}

#[test]
fn a_term_is_found_where_it_stands_and_passes_where_it_keeps_its_own_sense() {
  let terms = read_set_aside(DOCUMENT).unwrap();
  let found =
    |text: &str| banned_terms_in(text, &terms).into_iter().map(|term| term.term).collect::<Vec<_>>();

  assert_eq!(found("We leverage the cache and utilise the pool."), ["utilise", "leverage"]);
  assert!(found("It measured a lever's leverage.").is_empty());
  assert!(found("It says `utilise` as a quotation.").is_empty());
}

#[test]
fn a_document_naming_another_language_or_a_phrase_without_its_term_is_a_fault() {
  assert!(
    read_set_aside("## Banned terms\n\n- xx · word → other\n")
      .unwrap_err()
      .contains("unknown language code \"xx\"")
  );
  assert!(
    read_set_aside("## Banned terms\n\n- en · utilise → use\n\n## Uses in the word's own sense\n\n- en · utilise · no term here — why\n")
      .unwrap_err()
      .contains("must hold a banned term listed above it")
  );
  assert!(
    read_set_aside("## Uses in the word's own sense\n\n- en · utilise · utilise it — none listed above\n")
      .is_err(),
    "a phrase for a term no banned line lists is a fault"
  );
}

#[test]
fn the_terms_are_read_from_the_document_the_settings_name_and_none_without_one() {
  let dir = tempfile::tempdir().expect("a scratch directory");
  let path = dir.path().join("WORDING.md");

  std::fs::write(&path, DOCUMENT).expect("the document is written");

  assert_eq!(set_aside_terms(&settings(Some(&path.to_string_lossy()))).len(), 2);
  assert!(set_aside_terms(&settings(None)).is_empty());
  // A document that cannot be read sets nothing aside: no text is held to it.
  assert!(set_aside_terms(&settings(Some(&dir.path().join("gone.md").to_string_lossy()))).is_empty());
}
