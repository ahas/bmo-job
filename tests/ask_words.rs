//! What a question put to the owner may hold (docs/cli.md, 「`bmo-job job ask`」): a choice of direction read at a
//! glance, refused where it carries a command, a script or a payload, or runs too long to read at once.

use bmo_job::ask_words::{
  OPTION_MOST, QUESTION_LINES, QUESTION_MOST, WHY_MOST, say_refusal, words_refusal,
};

fn options(words: &[&str]) -> Vec<String> {
  words.iter().map(|word| (*word).to_owned()).collect()
}

#[test]
fn a_choice_of_direction_in_a_few_lines_is_put() {
  let asked = words_refusal(
    "Should the open room's own name be read from the open-chat info service?",
    &options(&["Open the read-only read", "Keep the road closed"]),
    "The road's prohibition is his to lift.",
  );
  assert_eq!(asked, None);
}

#[test]
fn a_command_a_script_or_a_payload_is_refused_wherever_it_stands() {
  let payload = "KGFzeW5jICgpID0+IHsKICBjb25zdCBjb252ID0gIjE5OmExMWZjYjU2OGVhZjQ3ZGI4NjQ4YWFjODQ3YTY3NGExQHRocmVhZC52MiI7";
  let two = options(&["Ran it", "Could not run it"]);
  assert!(words_refusal(&format!("Run this on u4: {payload}"), &two, "Only you reach u4.").is_some());
  assert!(
    words_refusal("Run the probe below.\n```\nGet-Process\n```", &two, "Only you reach u4.").is_some()
  );
  assert!(words_refusal("Run the probe?", &options(&[payload, "No"]), "Only you reach u4.").is_some());
  assert!(say_refusal(&format!("Paste this: {payload}")).is_some());
  assert_eq!(say_refusal("The first option keeps the token; the second stands the road down."), None);
}

#[test]
fn a_question_too_long_to_read_at_once_is_refused() {
  let two = options(&["One", "Another"]);
  assert!(words_refusal(&"word ".repeat(QUESTION_MOST / 5 + 1), &two, "why").is_some());
  assert!(words_refusal(&"line\n".repeat(QUESTION_LINES + 1), &two, "why").is_some());
  assert!(
    words_refusal("Which?", &options(&[&"long ".repeat(OPTION_MOST / 5 + 1), "short"]), "why").is_some()
  );
  assert!(words_refusal("Which?", &two, &"why ".repeat(WHY_MOST / 4 + 1)).is_some());
}
