//! How a part reads a text for whether it is written in English, declared by each part in its own file:
//! the words read past as standing in a line of another language and an English one alike, and how much
//! Hangul the rest may hold.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct English {
  /// A pattern of the words read past: names and terms a line of another language and an English one
  /// carry alike.
  pub set_aside: String,
  /// The Latin letters each Hangul syllable left past the set-aside words is read past against; none
  /// means a single Hangul syllable left is not English.
  pub latin_per_hangul: usize,
}
