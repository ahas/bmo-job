//! A text read with JavaScript's character classes, so a row written by the console and one written here
//! are trimmed and measured alike: its white space, its trim, its lengths in UTF-16 units and its reading of
//! a leading integer.

/// Whether a character is JavaScript's white space.
pub fn is_space(character: char) -> bool {
  matches!(
    character,
    '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
      ..='\u{200a}' | '\u{2028}' | '\u{2029}' | '\u{202f}' | '\u{205f}' | '\u{3000}' | '\u{feff}'
  )
}

/// A text with JavaScript's white space taken off both ends.
pub fn trim(text: &str) -> &str {
  text.trim_matches(is_space)
}

/// A text with JavaScript's white space taken off its end.
pub fn trim_end(text: &str) -> &str {
  text.trim_end_matches(is_space)
}

/// A text's length in UTF-16 units.
pub fn units(text: &str) -> usize {
  text.encode_utf16().count()
}

/// A text cut to `length` UTF-16 units, marked with an ellipsis where it was.
pub fn cut(text: &str, length: usize) -> String {
  let units: Vec<u16> = text.encode_utf16().collect();

  if units.len() <= length {
    text.to_owned()
  } else {
    format!("{}\u{2026}", String::from_utf16_lossy(&units[..length]))
  }
}

/// JavaScript's `parseInt` of a decimal: white space, a sign and the digits that lead.
pub fn int(text: &str) -> Option<i64> {
  let text = trim(text);
  let (sign, digits) = match text.strip_prefix('-') {
    Some(rest) => (-1, rest),
    None => (1, text.strip_prefix('+').unwrap_or(text)),
  };
  let leading: String = digits.chars().take_while(char::is_ascii_digit).collect();

  leading.parse::<i64>().ok().map(|number| sign * number)
}
