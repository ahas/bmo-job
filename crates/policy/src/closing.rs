//! What the closing of a TODO item reads the words it writes by: an unfinished note's strands and whose
//! each is, the lines that propose or defer an improvement instead of making it, and the lines that leave
//! a check undone. Its patterns are built from the deployment's names and the words of `[words]` once, as
//! the policy loads.

use serde::Deserialize;

use crate::english::English;
use crate::{Names, either, escape, fill_marks};

/// The strands of an unfinished note: the opener a listed strand starts with, the mark saying whose it is
/// with the name in its first group, and the marks a strand is written back with.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Strands {
  pub opener: String,
  pub mark: String,
  pub agent_mark: String,
  pub owner_mark: String,
  /// The flags the words' host-work pattern is compiled with.
  pub host_work_flags: String,
}

/// The flags the words' owner-only and raised-row patterns are compiled with.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leftovers {
  pub owner_only_flags: String,
  pub raised_row_flags: String,
}

/// What the lines that leave a check undone are answered with, and the flags the words' beyond-reach
/// pattern is compiled with.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Unchecked {
  pub beyond_reach_flags: String,
  /// The closing's refusal, with `{owner}`.
  pub refusal: String,
  /// The note the job runner names such lines under.
  pub note: String,
}

/// A report's part naming the graph nodes its change reached, by its heading, and the tool a job is asked
/// to have called for them.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Impact {
  pub heading: String,
  pub heading_flags: String,
  pub tool: String,
}

/// One pattern with the flags it is compiled with.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pattern {
  pub pattern: String,
  pub flags: String,
}

/// The words of the deployment's language the closing reads and writes by, English where the deployment
/// gives none.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Words {
  /// The word a strand's mark names whose it is by, after the name.
  pub share: String,
  /// What the note listing a closed item's open strands is called.
  pub unfinished: String,
  /// The headings of a report's own section of what it left.
  pub left_headings: Vec<String>,
  /// The words a report's impact part is headed by.
  pub impact: Vec<String>,
  /// The names a report says it swept a changed screen against the design rules by, read lower-case with
  /// every space taken out.
  pub swept: Vec<String>,
  pub host_work: String,
  pub owner_only: String,
  pub raised_row: String,
  pub beyond_reach: String,
  pub proposing: Vec<Pattern>,
  pub unchecked: Vec<Pattern>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Closing {
  pub strands: Strands,
  pub leftovers: Leftovers,
  pub unchecked: Unchecked,
  pub english: English,
  pub impact: Impact,
  pub words: Words,
}

impl crate::Part for Closing {
  const NAME: &'static str = "closing";
  const TEXT: &'static str = crate::policy_file!("closing");

  fn build(&mut self, names: &Names) {
    let share = escape(&self.words.share);
    let unfinished = escape(&self.words.unfinished);
    let impact = either(&self.words.impact);
    let words = [("share", share.as_str()), ("unfinished", unfinished.as_str())];
    let write = [
      ("owner", names.owner.as_str()),
      ("agent", names.agent.as_str()),
      ("share", &self.words.share),
    ];

    self.strands.mark = names.pattern(&self.strands.mark, &words);
    self.strands.agent_mark = fill_marks(&self.strands.agent_mark, &write);
    self.strands.owner_mark = fill_marks(&self.strands.owner_mark, &write);
    self.english.set_aside = names.pattern(&self.english.set_aside, &words);
    self.impact.heading = names.pattern(&self.impact.heading, &[("impact", impact.as_str())]);

    for pattern in [
      &mut self.words.host_work,
      &mut self.words.owner_only,
      &mut self.words.raised_row,
      &mut self.words.beyond_reach,
    ] {
      *pattern = names.pattern(pattern, &words);
    }

    for one in self.words.proposing.iter_mut().chain(&mut self.words.unchecked) {
      one.pattern = names.pattern(&one.pattern, &words);
    }
  }
}
