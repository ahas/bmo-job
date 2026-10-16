//! A line's marks are filled at once, the deployment's names among them, and the closing's patterns are
//! built from the names and the words and compile (docs/policy.md).

mod support;

use job_policy::{Policy, escape, fill_marks};
use regex::{Regex, RegexBuilder};
use support::{load, policy, settings};

/// A pattern compiled with the flags written beside it, as any reader of the policy compiles it.
fn compiled(pattern: &str, flags: &str) -> Regex {
  let mut builder = RegexBuilder::new(pattern);

  for flag in flags.chars() {
    match flag {
      'i' => builder.case_insensitive(true),
      'm' => builder.multi_line(true),
      'u' => builder.unicode(true),
      _ => panic!("`{flag}` is no flag a reader compiles with: {pattern}"),
    };
  }

  builder.build().unwrap_or_else(|error| panic!("{pattern} compiles: {error}"))
}

/// Every reading pattern of the policy, with its flags, as the loader built it.
fn patterns(policy: &Policy) -> Vec<(&str, &str)> {
  let closing = &policy.closing;
  let mut all = vec![
    (closing.strands.opener.as_str(), ""),
    (closing.strands.mark.as_str(), ""),
    (closing.words.host_work.as_str(), closing.strands.host_work_flags.as_str()),
    (closing.words.owner_only.as_str(), closing.leftovers.owner_only_flags.as_str()),
    (closing.words.raised_row.as_str(), closing.leftovers.raised_row_flags.as_str()),
    (closing.words.beyond_reach.as_str(), closing.unchecked.beyond_reach_flags.as_str()),
    (closing.english.set_aside.as_str(), ""),
    (closing.impact.heading.as_str(), closing.impact.heading_flags.as_str()),
    (policy.close.english.set_aside.as_str(), ""),
    (policy.workers.name_pattern.as_str(), ""),
  ];

  for one in closing.words.proposing.iter().chain(&closing.words.unchecked) {
    all.push((one.pattern.as_str(), one.flags.as_str()));
  }

  all
}

#[test]
fn every_mark_is_filled_at_once_and_an_unnamed_one_stays() {
  assert_eq!(
    fill_marks("on {branch} from {main}, {other}", &[("branch", "{main}"), ("main", "m")]),
    "on {main} from m, {other}"
  );
  assert_eq!(fill_marks("{} and {x", &[("x", "y")]), "{} and {x");
  assert_eq!(fill_marks("{0,60} {a}", &[("a", "b")]), "{0,60} b");
}

#[test]
fn the_deployments_names_fill_every_line_and_a_given_value_takes_their_place() {
  let policy = policy();

  assert_eq!(
    policy.fill(&policy.runaway.lines.told, &[]),
    "Whether to stop it is for Owner to decide, so it is told and not stopped."
  );
  assert!(policy.fill("{owner}", &[("owner", "Someone")]) == "Someone");
  assert_eq!(policy.fill("{agent} {sender} {unfinished}", &[]), "Agent Sender Unfinished");

  let fixes = policy.fill(&policy.work_turns.structural_fixes, &[]);

  assert!(fixes.contains("Agent share:") && fixes.contains("Owner share:"), "{fixes}");
  assert!(!fixes.contains('{'), "{fixes}");
}

#[test]
fn every_pattern_the_loader_builds_compiles_with_its_flags() {
  let policy = policy();

  for (pattern, flags) in patterns(&policy) {
    assert!(!pattern.contains("{names}") && !pattern.contains("{owner}"), "{pattern}");
    compiled(pattern, flags);
  }
}

#[test]
fn a_strands_mark_is_read_by_either_name_and_the_share_word() {
  let policy = policy();
  let mark = compiled(&policy.closing.strands.mark, "");
  let whose = |line: &str| mark.captures(line).map(|found| found[1].to_owned());

  assert_eq!(whose("Agent share: tidy the tests").as_deref(), Some("Agent"));
  assert_eq!(whose("**Owner share:** give the password").as_deref(), Some("Owner"));
  assert_eq!(whose("Owner share：a wide colon").as_deref(), Some("Owner"));
  assert_eq!(whose("Someone share: not a name"), None);
  assert_eq!(whose("Agent: no share word"), None);
  assert_eq!(policy.closing.strands.agent_mark, "Agent share:");
  assert_eq!(policy.closing.strands.owner_mark, "Owner share:");
}

#[test]
fn a_name_and_a_word_are_built_in_escaped() {
  let named =
    job_policy::Policy::load(&settings("A.B (c)", "Agent", "[words.closing]\nshare = \"part+\"\n"))
      .expect("the policy loads");
  let mark = compiled(&named.closing.strands.mark, "");

  assert!(mark.is_match("A.B (c) part+: a strand"));
  assert!(!mark.is_match("AxB (c) part+: a strand"));
  assert!(!mark.is_match("A.B (c) partt: a strand"));
  assert_eq!(named.closing.strands.owner_mark, "A.B (c) part+:");
  assert_eq!(escape("a.b|c{d}"), r"a\.b\|c\{d\}");
  assert_eq!(escape("a-b#c~d/e"), "a-b#c~d/e");

  for (pattern, flags) in patterns(&named) {
    compiled(pattern, flags);
  }
}

#[test]
fn the_words_read_past_for_english_and_the_impact_heading_follow_the_words() {
  let policy = load("[words.closing]\nunfinished = \"Open strands\"\nimpact = [\"impact\", \"reach\"]\n");
  let set_aside = compiled(&policy.closing.english.set_aside, "");
  let heading = compiled(&policy.closing.impact.heading, &policy.closing.impact.heading_flags);

  for said in ["Agent share", "Owner", "Open strands"] {
    assert!(set_aside.is_match(said), "{said}");
  }

  assert!(heading.is_match("report\n## Reach:\nnodes"));
  assert!(heading.is_match("**Impact**"));
  assert!(!heading.is_match("the impact of it"));
}

#[test]
fn the_reading_patterns_name_the_owner_by_the_deployments_name() {
  let policy = policy();
  let beyond = compiled(&policy.closing.words.beyond_reach, &policy.closing.unchecked.beyond_reach_flags);
  let proposing: Vec<Regex> = policy
    .closing
    .words
    .proposing
    .iter()
    .map(|one| compiled(&one.pattern, &one.flags))
    .collect();
  let unchecked: Vec<Regex> = policy
    .closing
    .words
    .unchecked
    .iter()
    .map(|one| compiled(&one.pattern, &one.flags))
    .collect();

  assert!(beyond.is_match("a permission only Owner holds"));
  assert!(beyond.is_match("the NIC is not yet in the lab"));
  assert!(!beyond.is_match("it was hard"));
  assert!(proposing.iter().any(|one| one.is_match("That is up to Owner to decide.")));
  assert!(proposing.iter().any(|one| one.is_match("This could be moved later.")));
  assert!(!proposing.iter().any(|one| one.is_match("The tests pass.")));
  assert!(unchecked.iter().any(|one| one.is_match("This was not verified.")));
  assert!(unchecked.iter().any(|one| one.is_match("It needs admin rights.")));
  assert!(!unchecked.iter().any(|one| one.is_match("Checked and measured: 12 ms.")));
}
