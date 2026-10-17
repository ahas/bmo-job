//! The shell hook's readings of a command line (docs/verbs.md, "The shell hook"): the shell's own split of
//! it, what a command writes and starts, what it hands to the agent program, and the refusals the hook makes
//! of a heavy command and of Hangul going into git.

mod support;

use bmo_job::verbs::job_guard::git_language::{EnglishTest, git_language_refusal};
use bmo_job::verbs::job_guard::reading::{
  expand_assigned, program_of as reading_program, read_command, shortly,
};
use bmo_job::verbs::job_guard::records::{CliRecords, command_refusal};
use bmo_job::verbs::job_guard::shell::{
  Heredoc, Split, bare, program_of, split, unquoted, value, without_heredoc_bodies,
};
use bmo_job::verbs::job_guard::testbed::testbed_refusal;
use bmo_job::verbs::job_guard::{hook_input, hook_refusal};
use job_testbed::Classified;
use support::queue::policy;

fn words(text: &str) -> Vec<Vec<String>> {
  split(text).commands.into_iter().map(|command| command.words).collect()
}

#[test]
fn words_keep_their_quoting_and_operators_end_a_command() {
  assert_eq!(
    words(r#"git commit -m "x" && rm -rf "${S:?}"/*.md; A=1 cmd"#),
    [
      vec!["git", "commit", "-m", "\"x\""],
      vec!["rm", "-rf", "\"${S:?}\"/*.md"],
      vec!["A=1", "cmd"]
    ]
  );
}

#[test]
fn a_descriptor_belongs_to_its_redirect_and_a_duplicate_names_no_file() {
  let read = split("echo hi > out.txt 2>&1 </dev/null >> log");
  let command = &read.commands[0];

  assert_eq!(command.words, ["echo", "hi"]);
  assert_eq!(command.redirects, ["out.txt", "log"]);
  assert_eq!(command.appended, ["log"]);
  assert_eq!(command.inputs, ["/dev/null"]);
  assert!(command.stdin_given);
}

#[test]
fn a_heredoc_body_is_text_beside_the_line_that_opened_it() {
  let read = split("cat > x <<'EOF' | wc -l\nls\npkill -f x\nEOF\necho after");

  assert_eq!(
    read.commands.iter().map(|command| command.words.clone()).collect::<Vec<_>>(),
    [vec!["cat"], vec!["wc", "-l"], vec!["echo", "after"]]
  );
  assert_eq!(
    read.heredocs,
    [Heredoc {
      opener: "cat > x <<'EOF' | wc -l".into(),
      text: "ls\npkill -f x".into()
    }]
  );
  assert!(read.commands[1].stdin_given);
}

#[test]
fn substitutions_come_out_with_whether_they_stood_quoted() {
  let read = split("echo $(echo one) \"a $(two) `three`\" <(ls four)");

  assert_eq!(
    read.inner.iter().map(|inner| (inner.text.as_str(), inner.quoted)).collect::<Vec<_>>(),
    [("echo one", false), ("two", true), ("three", true), ("ls four", false)]
  );
  assert_eq!(read.commands[0].words.last().unwrap(), "<(ls four)");
}

#[test]
fn heredoc_bodies_come_out_and_quoted_text_is_blanked() {
  let text = "edit docs/x.md --stdin <<'EOF'\n@@from\nfor each file\nEOF\necho done";

  assert_eq!(without_heredoc_bodies(text), "edit docs/x.md --stdin <<'EOF'\necho done");
  assert_eq!(bare(text), "edit docs/x.md --stdin << '' \n echo done");
  assert_eq!(bare("x --replace 'waits for the run' \"a $b\"c"), "x --replace '' ''c");
}

#[test]
fn a_line_the_tokenizer_cannot_read_reads_as_nothing() {
  assert_eq!(split("echo 'open"), Split::default());
}

#[test]
fn a_value_takes_the_quoting_off_and_leaves_expansions_as_written() {
  assert_eq!(value(r#""$HOME/.config"/x"#), "$HOME/.config/x");
  assert_eq!(value("'a b'c\\ d"), "a bc d");
  assert_eq!(value("$(echo name)"), "$(echo name)");
  assert_eq!(unquoted(r#""rm -rf \$D/x""#), "rm -rf $D/x");
}

#[test]
fn the_program_is_found_past_assignments_runners_and_grammar() {
  let given: Vec<String> = ["FOO=1", "timeout", "30", "tool", "-p"].map(str::to_owned).to_vec();
  let run = program_of(&given).unwrap();

  assert_eq!(
    (run.program.as_str(), run.args.as_slice(), run.runners.as_slice()),
    ("tool", &["-p".to_owned()][..], &["timeout".to_owned()][..])
  );
  assert_eq!(program_of(&["if".into(), "!".into()]), None);
}

#[test]
fn substitutions_are_read_as_commands_before_the_one_they_stand_in() {
  let read = read_command("echo $(cat x) > \"out file\"");

  assert_eq!(read.commands[0].words, ["cat", "x"]);
  assert_eq!(read.commands[1].words, ["echo", "$(cat x)"]);
  assert_eq!(read.commands[1].redirects, ["out file"]);
  assert!(!read.substituted_program);
  assert!(read_command("(`echo prog`) -p hi").substituted_program);
  assert!(!read_command("git commit -m \"node (`bot/prog`)\"").substituted_program);
  assert_eq!(read_command("git commit -m \"node (`bot/prog`)\"").commands.len(), 1);
}

#[test]
fn assigned_variables_are_put_in_where_they_are_used() {
  assert_eq!(expand_assigned("A=pro; B=g; $A$B -p hi"), "A=pro; B=g; prog -p hi");
  assert_eq!(expand_assigned("P=\"~/x\"; ls ${P}/y"), "P=\"~/x\"; ls ~/x/y");
}

#[test]
fn a_runner_and_its_value_are_looked_past() {
  let given: Vec<String> = ["timeout", "5", "bun", "x"].map(str::to_owned).to_vec();

  assert_eq!(reading_program(&given).unwrap().program, "bun");
  assert_eq!(shortly("a   b\nc"), "\"a b c\"");
}

#[test]
fn a_start_of_the_agent_program_is_named_by_the_piece_it_was_read_in() {
  let records = CliRecords::of(&[], "/srv/user");
  let program = ["cl", "aude"].concat();

  assert_eq!(
    command_refusal(&format!("~/.local/bin/{program} -p x"), &records).as_deref(),
    Some(format!("it starts the Claude Code CLI, read in \"~/.local/bin/{program}\" with \"-p\"").as_str())
  );
  assert!(
    command_refusal(&format!("bash -c '{program} --resume x'"), &records)
      .unwrap()
      .contains(&format!("inside the text \"{program} --resume x\" given to bash"))
  );
}

#[test]
fn a_heavy_command_is_refused_with_the_line_that_runs_it_in_the_testbed() {
  let heavy = Classified {
    heavy: true,
    language: Some("rust".into()),
    command: Some(vec!["cargo".into(), "test".into()]),
    instead: Some("bmo-testbed run -- cargo test".into()),
  };
  let light = Classified {
    heavy: false,
    language: None,
    command: None,
    instead: None,
  };
  let refused = testbed_refusal(&heavy).unwrap();

  assert!(refused.starts_with("refused: cargo test (rust) is a heavy command"), "{refused}");
  assert!(refused.contains("bmo-testbed run -- cargo test"), "{refused}");
  assert_eq!(testbed_refusal(&light), None);
}

#[test]
fn a_commit_message_holding_hangul_is_refused_and_an_english_one_passes() {
  let english = EnglishTest::of(&policy());
  let hangul: String = ['\u{d55c}', '\u{AE00}'].iter().collect();

  assert!(git_language_refusal(&format!("git commit -m 'Fix {hangul}'"), &english).is_some());
  assert_eq!(git_language_refusal("git commit -m 'Fix the reader'", &english), None);
  assert_eq!(git_language_refusal(&format!("echo {hangul}"), &english), None);
}

#[test]
fn the_hook_reads_its_input_and_refuses_with_the_reason() {
  let input = hook_input(
    r#"{"tool_name":"Bash","tool_input":{"command":"ls","timeout":1000,"run_in_background":true}}"#,
  )
  .unwrap();

  assert_eq!((input.tool.as_str(), input.command.as_deref()), ("Bash", Some("ls")));
  assert_eq!((input.call.timeout_ms, input.call.background), (Some(1000), true));
  assert!(hook_input("not json").is_err());

  let refused: serde_json::Value = serde_json::from_str(&hook_refusal("why")).unwrap();

  assert_eq!(refused["hookSpecificOutput"]["permissionDecision"], "deny");
  assert_eq!(refused["hookSpecificOutput"]["permissionDecisionReason"], "why");
}
