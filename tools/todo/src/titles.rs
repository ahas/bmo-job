//! The titles a reading shows (docs/tools.md, 「The todo tool」): the chat agent's as a person filed them, and the
//! worker's in the worker's language, each read out of the translations the gate keeps by the title's digest.
//! The reading never asks the gate's translator, and waits for the gate no longer than its bound.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use serde_json::Value;
use sha2::{Digest, Sha256};
use tool_call::Service;

/// The worker's language, by the tag the gate keeps its translations under.
pub const LANGUAGE: &str = "en";

/// What follows a title shown as written in another language.
pub const UNTRANSLATED: &str = "[not translated]";

/// The longest a reading waits for the gate's kept translations before it shows its titles as written.
pub const GATE_WAIT: Duration = Duration::from_secs(5);

/// How a reading shows a title.
pub trait Titles {
  fn shown(&self, title: &str) -> String;
}

/// Every title as a person filed it: the chat agent's readings.
pub struct AsWritten;

impl Titles for AsWritten {
  fn shown(&self, title: &str) -> String {
    title.to_owned()
  }
}

/// The translations the gate keeps for some titles, by each title's digest: the worker's readings.
#[derive(Debug, Default)]
pub struct Kept {
  readings: BTreeMap<String, String>,
}

impl Kept {
  /// The kept translations of `titles` the gate answers within `GATE_WAIT`; none where no gate is named.
  pub async fn read(gate: Option<&Service>, titles: &[&str]) -> Self {
    match gate {
      Some(gate) => Self::read_within(gate, titles, GATE_WAIT).await,
      None => Self::default(),
    }
  }

  // The kept translations of the titles in another script, read in one call that waits no longer than `wait`;
  // none where the gate cannot be reached, refuses, answers out of shape or does not answer in time.
  async fn read_within(gate: &Service, titles: &[&str], wait: Duration) -> Self {
    let digests: BTreeSet<String> =
      titles.iter().filter(|title| foreign(title)).map(|title| digest(title)).collect();

    if digests.is_empty() {
      return Self::default();
    }

    let query = [
      ("language", LANGUAGE.to_owned()),
      ("digests", digests.into_iter().collect::<Vec<_>>().join(",")),
    ];
    let page = match tokio::time::timeout(wait, tool_call::get(gate, "/translations/kept", &query)).await {
      Ok(Ok(page)) => page,
      Ok(Err(why)) => {
        eprintln!(
          "todo: the gate's kept translations were not read, so the titles stand as written: {why}"
        );
        return Self::default();
      }
      Err(_) => {
        eprintln!(
          "todo: the gate did not answer within {} ms, so the titles stand as written",
          wait.as_millis()
        );
        return Self::default();
      }
    };
    let readings = page
      .get("items")
      .and_then(Value::as_array)
      .into_iter()
      .flatten()
      .filter_map(|kept| {
        Some((kept.get("digest")?.as_str()?.to_owned(), kept.get("text")?.as_str()?.to_owned()))
      })
      .collect();

    Self { readings }
  }
}

impl Titles for Kept {
  /// A title in the Latin script as written; any other as its kept translation, else as written and marked.
  fn shown(&self, title: &str) -> String {
    if !foreign(title) {
      return title.to_owned();
    }

    match self.readings.get(&digest(title)) {
      Some(english) => english.clone(),
      None => format!("{title} {UNTRANSLATED}"),
    }
  }
}

/// The name the gate keeps a text's translation under: the SHA-256 of the text exactly as written, in lowercase
/// hex.
pub fn digest(text: &str) -> String {
  format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Whether a text holds a letter outside the Latin script, and so has a translation to show: the gate keeps none
/// for a text it leaves as written.
pub fn foreign(text: &str) -> bool {
  text.chars().any(|letter| letter.is_alphabetic() && !latin(letter))
}

// Whether a letter is one of the Latin script's: the basic letters, the Latin-1 and extended blocks, the phonetic
// and modifier letters written beside them, and the Latin ligatures.
fn latin(letter: char) -> bool {
  matches!(
    letter,
    'a'..='z'
      | 'A'..='Z'
      | '\u{AA}'
      | '\u{B5}'
      | '\u{BA}'
      | '\u{C0}'..='\u{2FF}'
      | '\u{1D00}'..='\u{1DBF}'
      | '\u{1E00}'..='\u{1EFF}'
      | '\u{2C60}'..='\u{2C7F}'
      | '\u{A720}'..='\u{A7FF}'
      | '\u{FB00}'..='\u{FB06}'
  )
}
