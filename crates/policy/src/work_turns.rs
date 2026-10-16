//! What the bot tells the turns of a work order it runs: the line every turn that decides what gets built
//! is held to, and the paragraphs and orders each turn is asked in.

use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkTurns {
  /// The structural-fix line, with `{owner}`, `{sender}`, `{unfinished}`, `{agent_mark}` and
  /// `{owner_mark}`.
  pub structural_fixes: String,
  /// The planning turn's paragraphs, joined by a space, with `{roles}` and `{structural_fixes}`.
  pub plan: Vec<String>,
  /// The synthesis turn's order, with `{structural_fixes}`.
  pub synthesis: String,
  /// How a builder turn adds a step to its own order, with `{tree}` and `{todo}`.
  pub add_step: String,
  /// The final turn's order, with `{structural_fixes}`, `{sender}`, `{owner}` and `{add_step}`.
  pub r#final: String,
  /// What a branch may and may not do and what it reports, with `{structural_fixes}`.
  pub branch_rules: String,
  /// The three sections a branch's result opens with.
  pub branch_result_shape: String,
  /// What a builder turn is told about the results before it.
  pub settled_results: String,
  /// The block a builder turn is handed the files a merged-back change left conflicted in, with `{files}`.
  pub merge_conflicts: String,
  /// The head of the block a reopened order's builder turn is told its earlier rounds in, with `{round}`.
  pub earlier_rounds: String,
  /// The line a builder turn's prompt closes on, with `{sender}`, `{owner}` and `{add_step}`.
  pub step_rules: String,
}

impl crate::Part for WorkTurns {
  const NAME: &'static str = "work-turns";
  const TEXT: &'static str = crate::policy_file!("work-turns");
}
