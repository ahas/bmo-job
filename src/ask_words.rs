//! What a job's question may hold (docs/cli.md, 「`bmo-job job ask`」): what the job cannot settle or do itself,
//! said in words read at a glance, in a few lines, with options of one short line each. A script, a block of code or
//! a long unbroken token — an encoded payload, a hash, a long address — is no such words and is refused, as is a
//! question too long to read at a glance.

/// The most characters the question's own words may run to.
pub const QUESTION_MOST: usize = 500;
/// The most lines the question's own words may run to.
pub const QUESTION_LINES: usize = 5;
/// The most characters one option may run to.
pub const OPTION_MOST: usize = 160;
/// The most characters the reason may run to.
pub const WHY_MOST: usize = 240;
/// The longest run of characters with no space a word of a question may be.
pub const WORD_MOST: usize = 80;

// What carries code, a command or a payload rather than words: a fenced block, or one unbroken token longer than
// any word a person writes.
fn carries_code(text: &str) -> bool {
  text.contains("```") || text.split_whitespace().any(|word| word.chars().count() > WORD_MOST)
}

/// The refusal of a question's words, or none where they are a question read and settled at a glance.
pub fn words_refusal(question: &str, options: &[String], why: &str) -> Option<String> {
  let texts = std::iter::once(question)
    .chain(options.iter().map(String::as_str))
    .chain(std::iter::once(why));
  if texts.into_iter().any(carries_code) {
    return Some(
      "a question is words read at a glance: a script, a block of code or a long token does not go into it. Say in \
       words what the supervisor is to settle or do, and put a long reading into the archive (bmo archive doc) \
       with its address in the question."
        .to_owned(),
    );
  }

  if question.trim().chars().count() > QUESTION_MOST || question.trim().lines().count() > QUESTION_LINES {
    return Some(format!(
      "a question is read at a glance: at most {QUESTION_MOST} characters on {QUESTION_LINES} lines. A long reading \
       goes into the archive (bmo archive doc) and its address into the question."
    ));
  }

  if options.iter().any(|option| option.trim().chars().count() > OPTION_MOST) {
    return Some(format!("an option is one short line: at most {OPTION_MOST} characters."));
  }

  (why.trim().chars().count() > WHY_MOST)
    .then(|| format!("the reason is one line: at most {WHY_MOST} characters."))
}

/// The refusal of the job's answer to the owner's talk where it carries code, a command or a payload; its length is
/// the job's to judge, since it explains what each option would do.
pub fn say_refusal(say: &str) -> Option<String> {
  carries_code(say).then(|| {
    "an answer in a talk is words: a script, a block of code or a long token does not go into it."
      .to_owned()
  })
}
