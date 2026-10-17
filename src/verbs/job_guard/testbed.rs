//! A heavy test or build command a job starts outside the testbed, refused with the line that runs it there
//! (docs/verbs.md, "The shell hook"). Whether a command is heavy is bmo-testbed's answer: its languages
//! claim their own heavy commands.

use job_testbed::Classified;

/// Why the command may not run as written, with the line that runs it in the testbed, or nothing.
pub fn testbed_refusal(classified: &Classified) -> Option<String> {
  if !classified.heavy {
    return None;
  }

  let words = classified
    .command
    .as_ref()
    .map_or_else(|| "this command".to_owned(), |words| words.join(" "));
  let claims = classified
    .language
    .as_ref()
    .map(|language| format!(" ({language})"))
    .unwrap_or_default();
  let instead = classified.instead.as_deref().unwrap_or("the testbed's own run");

  Some(format!(
    "refused: {words}{claims} is a heavy command, and a job runs a heavy command in the testbed, which waits for the machine's turn: {instead} (docs/verbs.md, \"The shell hook\")"
  ))
}
