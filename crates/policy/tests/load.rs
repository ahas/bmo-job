//! Every part of the policy loads, with the deployment's words laid over its own, and words no part takes
//! are refused (docs/policy.md).

mod support;

use std::collections::BTreeSet;
use std::path::Path;

use job_base::Settings;
use job_policy::research::opens;
use job_policy::{Policy, PolicyError};
use support::{EXAMPLE, load, policy, refusal};

#[test]
fn every_part_loads_from_the_checked_in_example() {
  let settings = Settings::load(Path::new(EXAMPLE)).expect("the example loads");
  let policy = Policy::load(&settings).expect("the policy loads");

  assert_eq!(policy.queue.question_stop_seconds, 30 * 60);
  assert!(policy.queue.word_files > 0);
  assert!(policy.job_prompt.preamble.heading.starts_with("[Worker preamble"));
  assert!(policy.research.holds("browse"));
  assert!(!policy.work_keys.profile.is_empty());
  assert!(!policy.work_turns.plan.is_empty());
  assert!(!policy.worker_words.paragraphs.task_open.is_empty());
  assert!(policy.workers.name_pattern.starts_with('^'));
  assert!(!policy.close.usage.close.is_empty());
  assert!(!policy.closing.words.proposing.is_empty());
  assert!(!policy.landing.refusal.open.is_empty());
  assert_eq!(policy.runaway.max_starts, 12);
  assert!(!policy.hold_commits.passed.is_empty());
}

#[test]
fn every_commit_message_names_its_item() {
  let commits = &policy().hold_commits;

  for message in [&commits.passed, &commits.kept_off, &commits.released, &commits.stopped, &commits.taken] {
    assert!(message.contains("{item}"), "{message}");
  }
}

#[test]
fn a_run_circles_past_twelve_starts_and_stalls_past_forty_five_minutes() {
  let runaway = &policy().runaway;

  assert_eq!(runaway.max_starts, 12);
  assert_eq!(runaway.stop_after_minutes, 45);
  assert!(runaway.lines.told.contains("{owner}"));
}

#[test]
fn the_rules_read_and_the_first_support_worker_follows_the_base() {
  let workers = &policy().workers;

  assert!(workers.name_pattern.starts_with('^'));
  assert_eq!(workers.first_support_slot, 2);
}

#[test]
fn a_word_said_to_a_worker_is_labelled_and_told_as_the_owner_said_it() {
  let words = &policy().worker_words;
  let said = &words.paragraphs;

  assert!(words.block_label.contains("{owner}"));
  assert!(
    [&said.aside_open, &said.task_open, &said.more_open]
      .iter()
      .all(|open| open.contains("{owner}"))
  );
}

#[test]
fn a_step_is_told_to_commit_its_own_work_and_names_its_dev_tab_by_the_words() {
  let prompt = &policy().job_prompt;

  assert!(prompt.work_step.told.contains("a build-list task commits its own step"));
  assert!(!prompt.work_step.told.contains("Do only this task"));
  assert!(!prompt.work_step.told.contains("co-workers"));
  assert!(
    prompt.preamble.dev_support.contains("{dev_tab}") && prompt.preamble.dev_own.contains("{dev_tab}")
  );
  assert!(prompt.impact.heading.contains("{agent}"));
  assert_eq!(prompt.words.dev_tab, "dev server");
}

#[test]
fn every_work_key_is_declared_once_with_a_label_key() {
  let keys = &policy().work_keys;
  let declared: BTreeSet<&str> = keys.profile.iter().map(|one| one.key.as_str()).collect();

  assert_eq!(declared.len(), keys.profile.len());

  for profile in &keys.profile {
    let label = &profile.label;

    assert!(
      !label.is_empty()
        && label.chars().all(|character| character.is_ascii_lowercase() || character == '_'),
      "{label}"
    );
    assert!(keys.profile(&profile.key).is_some());
  }
}

#[test]
fn the_planning_turn_names_the_roles_and_holds_the_structural_line() {
  let turns = &policy().work_turns;

  assert!(turns.plan.iter().any(|paragraph| paragraph.contains("{roles}")));
  assert!(turns.plan.iter().any(|paragraph| paragraph.contains("{structural_fixes}")));

  for mark in ["{agent_mark}", "{owner_mark}", "{unfinished}", "{owner}"] {
    assert!(turns.structural_fixes.contains(mark), "{mark}");
  }
}

#[test]
fn the_step_texts_hold_the_marks_they_are_filled_by() {
  let turns = &policy().work_turns;

  for text in [&turns.synthesis, &turns.r#final, &turns.branch_rules] {
    assert!(text.contains("{structural_fixes}"));
  }

  assert!(turns.merge_conflicts.contains("{files}"));
  assert!(turns.earlier_rounds.contains("{round}"));
  assert!(turns.step_rules.contains("{sender}") && turns.step_rules.contains("{owner}"));

  for text in [&turns.r#final, &turns.step_rules] {
    assert!(text.contains("{add_step}"));
  }

  assert!(turns.add_step.contains("{tree}") && turns.add_step.contains("--todo {todo} --add "));
}

// Every builder turn closes on the step rules, so the plan, synthesis and final orders leave what they say
// to them; a branch has none, and its own rules carry its language line.
#[test]
fn what_the_step_rules_say_no_builder_order_says_again() {
  let turns = &policy().work_turns;
  let plan = turns.plan.join(" ");
  let said = [
    "Write your own prose in English",
    "ever translate material you did not write",
    "why the enumeration missed it",
    "Only work separate from this item",
    "Do not launch another agent",
  ];

  for line in said {
    assert!(turns.step_rules.contains(line), "{line}");

    for order in [&plan, &turns.synthesis, &turns.r#final] {
      assert!(!order.contains(line), "{line} is said again");
    }
  }

  assert!(turns.branch_rules.contains("Write your own prose in English"));
  assert!(turns.branch_rules.contains("Never translate material you did not write"));
}

#[test]
fn a_setting_opens_its_capability_where_it_names_something_and_no_setting_opens_none() {
  let research = &policy().research;
  let set = |name: &str| match name {
    "BMO_RESEARCH_IMAGES" => Some("1"),
    "BMO_RESEARCH_TOOLS" => Some("0"),
    "BMO_RESEARCH_RULEBOOKS" => Some("a,,b"),
    _ => None,
  };

  assert_eq!(research.withheld(true, set), ["web.branches"]);
  assert_eq!(research.withheld(false, set), ["web.branches", "web.images", "web.rulebooks"]);
  assert!(!opens("") && !opens(",0,") && opens("0,goods"));

  for key in &research.clocked {
    assert!(research.holds(key), "{key}");
  }

  for key in &research.keys {
    assert!(!research.strategies(key).is_empty(), "{key}");
  }
}

#[test]
fn a_deployment_word_takes_the_place_of_the_parts_own_whole() {
  let policy = load(
    "[words.closing]\nshare = \"part\"\nleft_headings = [\"Remaining\"]\n\n[words.job-prompt]\ndev_tab = \"dev\"\n",
  );

  assert_eq!(policy.closing.words.share, "part");
  assert_eq!(policy.closing.words.left_headings, ["Remaining"]);
  assert_eq!(policy.closing.words.unfinished, "Unfinished");
  assert_eq!(policy.closing.strands.agent_mark, "Agent part:");
  assert_eq!(policy.job_prompt.words.dev_tab, "dev");
}

#[test]
fn a_word_no_part_takes_is_refused() {
  let unknown_key = refusal("[words.closing]\ncolour = \"blue\"\n");
  let wrong_type = refusal("[words.closing]\nshare = 3\n");
  let no_words = refusal("[words.queue]\nword_files = 2\n");
  let no_part = refusal("[words.nowhere]\nshare = \"part\"\n");
  let not_a_table = refusal("[words]\nclosing = \"part\"\n");

  assert!(
    matches!(
      unknown_key,
      PolicyError::Malformed {
        part: "closing",
        ..
      }
    ),
    "{unknown_key}"
  );
  assert!(unknown_key.to_string().contains("colour"), "{unknown_key}");
  assert!(
    matches!(
      wrong_type,
      PolicyError::Malformed {
        part: "closing",
        ..
      }
    ),
    "{wrong_type}"
  );
  assert!(matches!(no_words, PolicyError::TakesNoWords { part: "queue" }), "{no_words}");
  assert!(matches!(&no_part, PolicyError::NoSuchPart { part } if part == "nowhere"), "{no_part}");
  assert!(matches!(not_a_table, PolicyError::WordsNotATable { part: "closing" }), "{not_a_table}");
}
