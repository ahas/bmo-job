//! The hook in front of a job's shell (docs/live-claude-guard.md): the readings of a command — the
//! live-Claude guard, the structure document, the brain, other workers' checkouts, a write into the job's own
//! checkout, the stalling habits, the removals the CLI stops on and the heavy commands that belong in the
//! testbed, Hangul going into git — and the hook binary as the worker's settings run it, over a scratch archive. The commands the
//! #318 job ran to fork a session and measure `/compact` on it are held here word for word, beside the
//! spellings the same thing takes and the everyday commands a job must keep running.

use std::path::Path;

use bmo_job::verbs::job_guard::git_language::{EnglishTest, git_language_refusal};
use bmo_job::verbs::job_guard::habits::{
  CLOSED_STDIN, LONG_COMMAND_WAIT_MS, ShellCall, closed_stdin_answer, habit_refusal, unquoted_glob,
};
use bmo_job::verbs::job_guard::records::{CliRecords, command_refusal, print_mode_refusal};
use bmo_job::verbs::job_guard::removal::{Unsafe, removal_refusal, unsafe_target};
use bmo_job::verbs::job_guard::writes::{
  CheckoutWrite, ForeignCheckout, Means, WriteShape, brain_command_write, brain_file_write, checkout_write,
  checkout_write_refusal, foreign_command_write, foreign_file_write, structure_command_refusal,
  structure_file_refusal, verb_for, what_of,
};
use bmo_job::verbs::job_guard::{Hooked, hook, hook_input, job_checkouts, structure_refusal_message};
use bmo_job::verbs::stand::JobProcess;
use serde_json::{Map, Value, json};

const HOME: &str = "/srv/user";
const ROOT: &str = "/srv/user/repos/bench";

fn machine() -> CliRecords {
  CliRecords::of(&[format!("{HOME}/.claude")], HOME)
}

fn refusal(command: &str) -> Option<String> {
  command_refusal(command, &machine())
}

// What that job ran, as its record has it.
const FORK: &str = r#"SP=/tmp/claude-1000/-srv-user-repos-bench/11111111-1111-1111-1111-111111111111/scratchpad && P=~/.claude/projects/-srv-user-repos-bench && NEW=$(uuidgen) && echo $NEW > $SP/fork-id && sed "s/11111111-1111-1111-1111-111111111111/$NEW/g" $P/11111111-1111-1111-1111-111111111111.jsonl > $P/$NEW.jsonl && timeout 5 bun $SP/segments.ts $P/$NEW.jsonl </dev/null | tail -2"#;
const CLEANUP: &str = r#"SP=/tmp/claude-1000/-srv-user-repos-bench/11111111-1111-1111-1111-111111111111/scratchpad; P=/srv/user/.claude/projects/-srv-user-repos-bench; for f in fork-id fork3-id; do id=$(cat $SP/$f); command ls $P/$id.jsonl </dev/null && rm -f $P/$id.jsonl; rm -rf $P/$id; done"#;
const PANE_SCRIPT: &str = "cat > $SP/pane-fold3.ts <<EOF\nimport { herdrCall } from \"/srv/user/repos/bench/src/herdr\";\nconst sent: any = await herdrCall(\"agent.prompt\", { target: pane, text: \"/compact\" }, {});\nEOF";

const REFUSED: [&str; 25] = [
  FORK,
  CLEANUP,
  PANE_SCRIPT,
  "claude -p hi",
  "env FOO=1 claude --resume 00000000-0000-0000-0000-000000000000",
  "command claude",
  "echo x | xargs -n1 claude",
  "bash -c 'claude --resume x'",
  r#"zsh -c "timeout 30 claude -p hi""#,
  "~/.local/bin/claude -p x",
  "nohup claude -p hi &",
  r#"bun -e 'Bun.spawn(["claude","-p","hi"])'"#,
  r#"cp ~/.claude/projects/a.jsonl "$HOME/.claude/projects/b.jsonl""#,
  r#"sed -i s/a/b/ "$HOME/.claude/projects/x.jsonl""#,
  "echo hi > $CLAUDE_CONFIG_DIR/projects/x.jsonl",
  "ln -s /tmp/a.jsonl /srv/user/.claude/projects/-x/b.jsonl",
  "claude --fork-session",
  // The spellings that hide the program word or the records directory behind the shell's own reading.
  "A=claude; $A -p hi",
  "A=cla; B=ude; $A$B -p hi",
  "$(echo claude) -p hi",
  "eval 'claude -p hi'",
  "source <(echo 'claude -p hi')",
  "echo 'claude -p hi' | bash",
  r#"CONFDIR=~/.claude; cp x "$CONFDIR/projects/y""#,
  "`claude` -p hi",
];

const ALLOWED: [&str; 16] = [
  "bun run test",
  "bun test --parallel scripts/job-guard.test.ts > /tmp/out.txt 2>&1 </dev/null",
  "~/.local/share/bmo/deploy/bin/bmo fold-worker",
  "bun scripts/deploy.ts --then 'ship'",
  r#"git commit -m "claude guard""#,
  "git push",
  r#"grep -n "claude" agents/agent/claude-code.ts"#,
  "ls /srv/user/.claude/projects </dev/null | wc -l",
  "tail -n 5 ~/.claude/projects/x/y.jsonl 2>/dev/null",
  "jq . ~/.claude/projects/x.jsonl > /tmp/out.json",
  r#"grep -rn "agent.prompt" src"#,
  "cd ~/.claude/projects/-home-x && grep -c 'Work order' a.jsonl </dev/null",
  "OUT=/tmp/count.txt; ls ~/.claude/projects > $OUT",
  "bash scripts/setup.sh",
  "grep -rn claude $(git ls-files)",
  "source ~/.zshrc && bun run test",
];

// "refuses %p"
// "lets %p through"
#[test]
fn what_a_jobs_shell_is_refused() {
  for command in REFUSED {
    assert!(refusal(command).is_some(), "let through: {command}");
  }

  for command in ALLOWED {
    assert_eq!(refusal(command), None, "{command}");
  }
}

// "refuses print mode, and leaves every other start of the CLI to the opening"
#[test]
fn print_mode_is_refused_and_every_other_start_of_the_cli_is_left_to_the_opening() {
  for command in [
    "claude -p hi",
    "claude --print hi",
    "env FOO=1 claude --model x -p hi",
    r#"zsh -c "timeout 30 claude -p hi""#,
    "eval 'claude --print hi'",
    "A=claude; $A -p hi",
  ] {
    let refused = print_mode_refusal(command).unwrap_or_else(|| panic!("let through: {command}"));

    assert!(
      refused.contains("Claude Code's print mode, which no agent of this system runs"),
      "{refused}"
    );
  }

  for command in [
    "claude --resume x",
    "claude",
    "claude --fork-session",
    "systemctl show -p MainPID bmo",
    "echo claude -p",
    r#"bun -e 'Bun.spawn(["claude","--resume","x"])'"#,
  ] {
    assert_eq!(print_mode_refusal(command), None, "{command}");
  }

  assert!(
    print_mode_refusal("claude -p hi")
      .unwrap()
      .starts_with("refused: it starts the Claude Code CLI, read in \"claude\" with \"-p\"")
  );
}

// "a script run from a file is out of this hook's sight, which is why the launch guard stands behind it"
#[test]
fn a_script_run_from_a_file_is_out_of_this_hooks_sight() {
  assert_eq!(refusal("timeout 420 bun $SP/pane-fold3.ts"), None);
}

// Every login keeps its records under its own configuration directory, and the main login's lies under the
// data root, where neither `.claude/projects` nor `CLAUDE_CONFIG_DIR` is spelled. The #725 job saw an
// in-place edit refused on the one and let through on the other.
fn logins() -> [String; 2] {
  [format!("{HOME}/.claude"), format!("{HOME}/.local/share/bmo/data/env/claude-main")]
}

// "refuses the same writes into the records under %p"
// "lets reads of the records under %p through"
// "a redirect into a login's configuration directory counts as a write where the records are named"
// "handed no login, it refuses and lets through exactly what the machine's spellings decide"
// "a directory whose name only begins like a login's is not its records"
#[test]
fn every_logins_session_records() {
  let records = CliRecords::of(&logins(), HOME);

  for dir in logins() {
    let under = &dir[HOME.len()..];
    let edit = format!("sed -i s/a/b/ {dir}/projects/-x/y.jsonl");

    assert_eq!(
      command_refusal(&edit, &records).as_deref(),
      Some("it edits the Claude Code CLI's session records in place, read in \"sed\"")
    );

    for command in [
      format!("cp /tmp/y.jsonl ~{under}/projects/-x/y.jsonl"),
      format!("rm \"$HOME{under}/projects/-x/y.jsonl\""),
      format!("echo x > ${{HOME}}{under}/projects/-x/y.jsonl"),
      format!("P=~{under}/projects; cp /tmp/y \"$P/-x/y.jsonl\""),
      format!("bash -c 'sed -i s/a/b/ {dir}/projects/-x/y.jsonl'"),
    ] {
      assert!(command_refusal(&command, &records).is_some(), "{command}");
    }

    assert_eq!(
      command_refusal(&format!("grep -c x {dir}/projects/-x/y.jsonl </dev/null"), &records),
      None
    );
    assert_eq!(
      command_refusal(&format!("tail -n 5 {dir}/projects/-x/y.jsonl > /tmp/out.txt"), &records),
      None
    );
  }

  let main = &logins()[1];

  assert!(command_refusal(&format!("ls {main}/projects > {main}/list.txt"), &records).is_some());

  // No account, or an archive the hook could not read, hands the guard nothing, and it judges with the
  // machine's spellings alone.
  let none = CliRecords::of(&[], HOME);

  for command in REFUSED {
    assert!(command_refusal(command, &none).is_some(), "{command}");
  }

  for command in ALLOWED {
    assert_eq!(command_refusal(command, &none), None, "{command}");
  }

  assert!(command_refusal(&format!("sed -i s/a/b/ {HOME}/.claude/projects/-x/y.jsonl"), &none).is_some());
  assert_eq!(command_refusal(&format!("sed -i s/a/b/ {main}/projects/-x/y.jsonl"), &none), None);
  assert_eq!(command_refusal(&format!("sed -i s/a/b/ {main}-old/projects/y.jsonl"), &records), None);
}

// A backquote opens a command substitution only where the shell would open one, and not inside quotes. The
// #596 job wrote a report naming a graph node inside a markdown code span, the guard read the backquote
// after the bracket as a substitution, read the node's name as a program, and refused the command.
// "lets %p through"
// "still refuses %p"
#[test]
fn a_backquote_that_stands_as_a_character() {
  for command in [
    "bun scripts/todo.ts --done 596 --report 'added the node (`bot/claude`) to the graph'",
    "bun scripts/todo.ts --done 596 --report 'Fixed\n1. added the `bot/claude` node'",
    "git commit -m \"node (`bot/claude`) stands in the graph\"",
    "bun scripts/graph.ts alias bot/live-claude-guard --add 'job guard (`claude`)'",
  ] {
    assert_eq!(refusal(command), None, "{command}");
  }

  for command in [
    "`echo claude` -p hi",
    "(`echo claude`) -p hi",
    "if true; then `echo claude` -p hi; fi",
    "`claude` -p hi",
  ] {
    assert!(refusal(command).is_some(), "{command}");
  }
}

// "names the piece of the command it was read in"
#[test]
fn names_the_piece_of_the_command_it_was_read_in() {
  assert_eq!(
    refusal("~/.local/bin/claude -p x").as_deref(),
    Some("it starts the Claude Code CLI, read in \"~/.local/bin/claude\" with \"-p\"")
  );
  assert!(
    refusal("bash -c 'claude --resume x'")
      .unwrap()
      .contains("inside the text \"claude --resume x\" given to bash")
  );
  assert!(
    refusal("`echo claude` -p hi")
      .unwrap()
      .contains("a command substitution stands where the program does")
  );
  assert!(
    refusal("echo 'claude -p hi' | bash")
      .unwrap()
      .contains("runs the text it is handed as commands")
  );
}

// "is a Bash call's command with the wait it asked for, or the file a file tool names"
#[test]
fn is_a_bash_calls_command_with_the_wait_it_asked_for_or_the_file_a_file_tool_names() {
  let read = |value: Value| hook_input(&value.to_string()).unwrap();
  let bash = read(json!({ "tool_name": "Bash", "tool_input": { "command": "claude -p" } }));

  assert_eq!(
    (bash.tool.as_str(), bash.command.as_deref(), bash.path.as_deref()),
    ("Bash", Some("claude -p"), None)
  );
  assert_eq!(bash.call, ShellCall::default());

  let long = read(
    json!({ "tool_name": "Bash", "tool_input": { "command": "git push", "timeout": 600000, "run_in_background": true } }),
  );

  assert_eq!(
    long.call,
    ShellCall {
      timeout_ms: Some(600_000),
      background: true
    }
  );

  let edit = read(json!({ "tool_name": "Edit", "tool_input": { "file_path": "/x" } }));

  assert_eq!(
    (edit.tool.as_str(), edit.command.as_deref(), edit.path.as_deref()),
    ("Edit", None, Some("/x"))
  );
  assert_eq!(
    read(json!({ "tool_name": "NotebookEdit", "tool_input": { "notebook_path": "/n" } }))
      .path
      .as_deref(),
    Some("/n")
  );
  assert!(hook_input("not json").is_err());
}

// STRUCTURE.md is the owner's statement of the system, and a job changes it only on his own request.
// "refuses ${JSON.stringify(command)}"
// "lets ${JSON.stringify(command)} through"
// "refuses a file tool on it wherever the checkout is, and nothing else"
// "says in its refusal what the document is and what to do instead"
#[test]
fn the_structure_document() {
  for command in [
    "echo x > STRUCTURE.md",
    "printf x >> /srv/user/repos/bench/STRUCTURE.md",
    "sed -i 's/a/b/' STRUCTURE.md",
    "cp /tmp/x STRUCTURE.md",
    "rm -f ./STRUCTURE.md",
    "git checkout -- STRUCTURE.md",
    "cat notes | tee STRUCTURE.md",
    "F=STRUCTURE.md; echo x > $F",
    "bash -c 'mv a STRUCTURE.md'",
    r#"python3 -c "open('STRUCTURE.md','w').write('x')""#,
    "python3 - <<'EOF'\nopen('STRUCTURE.md','w').write('x')\nEOF",
    r#"bun -e 'await Bun.write("STRUCTURE.md", "x")'"#,
  ] {
    assert!(structure_command_refusal(command).is_some(), "{command}");
  }

  for command in [
    "cat STRUCTURE.md",
    "grep -n Worker STRUCTURE.md",
    "git diff STRUCTURE.md",
    "git add STRUCTURE.md README.md",
    "bun test --parallel server/lore.test.ts > out.txt; grep STRUCTURE.md out.txt",
    "sed -n 1,20p STRUCTURE.md",
    "echo x > README.md",
  ] {
    assert_eq!(structure_command_refusal(command), None, "{command}");
  }

  assert!(structure_file_refusal("/srv/user/repos/bench/STRUCTURE.md").is_some());
  assert!(structure_file_refusal("STRUCTURE.md").is_some());
  assert_eq!(structure_file_refusal("/srv/user/repos/bench/docs/STRUCTURE.md.bak"), None);
  assert_eq!(structure_file_refusal("/srv/user/repos/bench/NOT_STRUCTURE.md"), None);

  let message = structure_refusal_message("it edits STRUCTURE.md in place");

  assert!(message.contains("it edits STRUCTURE.md in place"));
  assert!(message.contains("report the difference"));
}

// An interpreter takes its flags combined — `perl -ne`, `python3 -Bc`, a script written against the flag
// with no space between them — and a reading that matched one flag whole saw no script in any of those.
// "refuses a script naming it whose write call goes somewhere the reading cannot resolve"
// "reads an -i backup suffix as a suffix rather than as a flag carrying a script"
#[test]
fn an_interpreters_flags_standing_combined() {
  let document = "STRUCTURE.md";

  for command in [
    format!(r#"perl -ne 'open(F, ">{document}")' notes.txt"#),
    format!(r#"perl -pe 'BEGIN {{ open(STDOUT, ">", "{document}") }}' notes.txt"#),
    format!(r#"perl -e'open(F, ">{document}")'"#),
    format!(r#"ruby -ne 'File.write("{document}", $_)' notes.txt"#),
    format!(r#"ruby -nle 'File.write("{document}", $_)' notes.txt"#),
    format!(r#"python3 -Bc "open('{document}','w').write('x')""#),
    format!(r#"python3 -uc "open('{document}','w').write('x')""#),
    format!(r#"python3 -c"open('{document}','w').write('x')""#),
    format!(r#"node --eval='require("fs").writeFileSync("{document}","x")'"#),
    format!(r#"bun --eval 'await Bun.write("{document}", "x")'"#),
    format!("python3 -u - <<'EOF'\nopen('{document}','w').write('x')\nEOF"),
    format!("perl -i -pe 's/a/b/' {document}"),
    format!("perl -i.bak -pe 's/a/b/' {document}"),
    format!("perl -pi -e 's/a/b/' {document}"),
    format!(r#"ruby -i.bak -pe '$_.sub!("a", "b")' {document}"#),
    format!(r#"python3 -ic "open('{document}','w').write('x')""#),
  ] {
    assert!(structure_command_refusal(&command).is_some(), "{command}");
  }

  for command in [
    format!("perl -ne 'print if /Worker/' {document}"),
    format!("perl -lane 'print $F[0]' {document}"),
    format!("ruby -ne 'puts $_ if $_ =~ /Worker/' {document}"),
    format!(r#"ruby -e 'puts File.read("{document}").lines.count'"#),
    format!("perl -pe 's/a/b/' {document} > /tmp/out.md"),
    format!("perl -ne 'print' {document} | wc -l"),
    format!("python3 scripts/count.py {document}"),
    // A script that opens the document only to read it makes no write call.
    format!(r#"python3 -c "print(open('{document}').read().count('Worker'))""#),
    format!("python3 - <<'PY'\ns=open('{document}').read()\nprint(len(s))\nPY"),
  ] {
    assert_eq!(structure_command_refusal(&command), None, "{command}");
  }

  // A write whose file the reading cannot resolve, in a script naming the document, is refused rather than
  // guessed at.
  assert!(
    structure_command_refusal(&format!(
      r#"python3 -c "import os; p=os.getcwd()+'/{document}'; open(p,'w').write('x')""#
    ))
    .is_some()
  );
  assert!(
    structure_command_refusal(&format!(
      r#"python3 -c "p=os.getcwd()+'/notes.md'; open(p,'w').write(open('{document}').read())""#
    ))
    .is_some()
  );

  // In an in-place editor's cluster `-i` takes the rest of it as the backup suffix.
  let in_place = format!("it edits {document} in place");
  let script = format!("it runs a script that names {document} and writes");

  assert_eq!(
    structure_command_refusal(&format!("perl -ie 'print' {document}")).as_deref(),
    Some(in_place.as_str())
  );
  assert_eq!(
    structure_command_refusal(&format!("perl -i.bak -pe 's/a/b/' {document}")).as_deref(),
    Some(in_place.as_str())
  );
  assert_eq!(
    structure_command_refusal(&format!(r#"ruby -i.bak -pe '$_.sub!("a", "b")' {document}"#)).as_deref(),
    Some(in_place.as_str())
  );
  assert_eq!(
    structure_command_refusal(&format!(r#"perl -ne 'open(F, ">{document}")' notes.txt"#)).as_deref(),
    Some(script.as_str())
  );
  assert_eq!(
    structure_command_refusal(&format!(r#"python3 -ic "open('{document}','w').write('x')""#)).as_deref(),
    Some(script.as_str())
  );
}

// "counts ${JSON.stringify(command)}"
// "does not count ${JSON.stringify(command)}"
// "counts a file tool inside the brain and nothing beside it"
#[test]
fn a_write_into_the_brain() {
  let brain = "/srv/someone/.local/share/bmo/data/brain";

  for command in [
    format!("echo x > {brain}/knowledge/chunk.md"),
    "echo x >> ~/.local/share/bmo/data/brain/people/1/sheet.md".to_owned(),
    r#"cp /tmp/a.md "$HOME/.local/share/bmo/data/brain/knowledge/a.md""#.to_owned(),
    format!("sed -i s/a/b/ {brain}/rooms/1/rules.md"),
    format!("git -C {brain} commit -am translate"),
    format!("cd {brain} && git add knowledge && git commit -m translate"),
    format!("cd {brain} && git commit -am 'translate'"),
    format!("cd {brain}/knowledge && echo x > chunk.md"),
    format!("B={brain}; mv /tmp/a.md $B/knowledge/a.md"),
    format!("bash -c 'rm {brain}/knowledge/old.md'"),
    format!(r#"python3 -c "open('{brain}/knowledge/a.md','w').write('x')""#),
    format!(r#"ruby -ne 'File.write("{brain}/knowledge/a.md", $_)' notes.txt"#),
  ] {
    assert!(brain_command_write(&command, brain, "/srv/someone").is_some(), "{command}");
  }

  for command in [
    format!("cat {brain}/knowledge/index.md"),
    format!("grep -rn translate {brain}/knowledge > /tmp/out.txt"),
    format!("git -C {brain} log --oneline -5"),
    format!("cd {brain} && git diff --stat"),
    format!("ls {brain}/people </dev/null | wc -l"),
    "echo x > /tmp/brain.md".to_owned(),
    format!(r#"python3 -c "print(open('{brain}/knowledge/index.md').read())""#),
    format!("cd {brain} && cd /tmp && echo x > out.md"),
    "git commit -m 'bmo/data/brainstorm'".to_owned(),
  ] {
    assert_eq!(brain_command_write(&command, brain, "/srv/someone"), None, "{command}");
  }

  assert!(brain_file_write(&format!("{brain}/knowledge/a.md"), brain));
  assert!(!brain_file_write(&format!("{brain}-old/a.md"), brain));
  assert!(!brain_file_write("/srv/user/repos/bench/docs/people-book.md", brain));
}

// The block on writing a file of this checkout with an inline script, an in-place editor or a redirect
// (docs/job-tools.md 「What a job's shell may no longer write with」). The rows are real commands out of the
// record #632 was measured against, their text made harmless, several per branch of it, and each one says
// whether it is refused or allowed.
// "the fixture says one of the two verdicts for every row it holds"
// "${row.verdict} · ${row.branch} · fixture line ${at + 2}"
#[test]
fn what_a_jobs_shell_may_no_longer_write_with() {
  let fixture = std::fs::read_to_string(
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/job-guard-writes.tsv"),
  )
  .unwrap();
  let tilde = format!("~{}", &ROOT[HOME.len()..]);
  let rows: Vec<(String, String, String)> = fixture
    .lines()
    .skip(1)
    .filter(|line| !line.trim().is_empty())
    .map(|line| {
      let cells: Vec<&str> = line.split('\t').collect();
      let command = serde_json::from_str::<String>(cells[2])
        .unwrap()
        .replace("{root}", ROOT)
        .replace("{tilde}", &tilde)
        .replace("{home}", HOME)
        .replace("{tmp}", "/tmp/claude-1000/probe");

      (cells[0].to_owned(), cells[1].to_owned(), command)
    })
    .collect();

  assert!(rows.iter().filter(|(verdict, _, _)| verdict == "refused").count() > 20);
  assert!(rows.iter().filter(|(verdict, _, _)| verdict == "allowed").count() > 20);
  assert!(rows.iter().all(|(verdict, _, _)| verdict == "refused" || verdict == "allowed"));

  for (at, (verdict, branch, command)) in rows.iter().enumerate() {
    let refused = checkout_write_refusal(command, ROOT, HOME);
    let line = at + 2;

    if verdict == "refused" {
      let path = checkout_write(command, ROOT, HOME).map(|write| write.path).unwrap_or_default();

      assert!(
        refused.as_deref().is_some_and(|refused| refused.contains(&path)),
        "{branch} · fixture line {line}: {command}"
      );
    } else {
      assert_eq!(refused, None, "{branch} · fixture line {line}: {command}");
    }
  }
}

fn write(shape: WriteShape, path: &str) -> CheckoutWrite {
  CheckoutWrite {
    shape,
    means: Means::Script,
    path: path.into(),
    word: "python3".into(),
  }
}

const UNREADABLE: &str =
  r#"python3 -c "p='server/sample.ts'; s=open(p).read(); open(p,'w').write(s.upper())""#;

// The verb is read off the shape and the path by one function, so a refusal cannot hand back a call that does
// something else, and the one shape no call answers is the one that is never refused.
// "names one call for every shape the reading comes to"
#[test]
fn names_one_call_for_every_shape_the_reading_comes_to() {
  let verb = |shape| verb_for(&write(shape, "server/sample.ts")).unwrap();

  assert_eq!(verb(WriteShape::Phrase), "bmo edit server/sample.ts --replace '<before>' '<after>'");
  assert!(verb(WriteShape::Block).contains("--stdin") && verb(WriteShape::Block).contains("@@from"));
  assert!(verb(WriteShape::Pattern).contains("--replace-regex '<pattern>' '<after>'"));
  assert!(verb(WriteShape::Anchors).contains("--between '<start>' '<end>' '<after>'"));
  assert!(verb(WriteShape::Lines).contains("--replace-lines <a>-<b>"));
  assert!(verb(WriteShape::Cut).contains("--delete-lines <a>-<b>"));
  assert!(verb(WriteShape::Insert).contains("--insert-after"));
  assert!(verb(WriteShape::Append).contains("--insert-at <line>"));
  assert!(verb(WriteShape::Whole).contains("Write"));
}

// "hands back the graph's own verb for a file of the graph, whatever was being done to it"
#[test]
fn hands_back_the_graphs_own_verb_for_a_file_of_the_graph_whatever_was_being_done_to_it() {
  for shape in [WriteShape::Phrase, WriteShape::Block, WriteShape::Pattern, WriteShape::Whole] {
    assert_eq!(
      verb_for(&write(shape, "graph/sample.json")).as_deref(),
      Some("bmo graph update <node> --replace '<before>' '<after>'")
    );
  }
}

// "names nothing for a write it could not read, which is why that one is let through"
#[test]
fn names_nothing_for_a_write_it_could_not_read_which_is_why_that_one_is_let_through() {
  assert_eq!(verb_for(&write(WriteShape::Unread, "server/sample.ts")), None);
  assert_eq!(
    checkout_write(UNREADABLE, ROOT, HOME).map(|write| write.shape),
    Some(WriteShape::Unread)
  );
  assert_eq!(checkout_write_refusal(UNREADABLE, ROOT, HOME), None);
}

// "keeps reading past a write it cannot name, so a nameable edit standing beside one is still refused"
#[test]
fn keeps_reading_past_a_write_it_cannot_name_so_a_nameable_edit_standing_beside_one_is_still_refused() {
  let command = format!("{UNREADABLE} && sed -i 's/alpha/beta/' server/two.ts");

  assert_eq!(
    checkout_write(&command, ROOT, HOME).map(|write| write.path).as_deref(),
    Some("server/two.ts")
  );
}

// "says what was read, in which word, and the whole call that does it"
#[test]
fn says_what_was_read_in_which_word_and_the_whole_call_that_does_it() {
  let refused = checkout_write_refusal("sed -i 's/alpha/beta/' server/sample.ts", ROOT, HOME).unwrap();

  assert!(refused.contains("it runs a pattern substitution over server/sample.ts with an in-place editor"));
  assert!(refused.contains("read in \"sed\""));
  assert!(refused.contains("bmo edit server/sample.ts --replace-regex"));
  assert_eq!(
    what_of(&CheckoutWrite {
      shape: WriteShape::Whole,
      means: Means::Redirect,
      path: "server/one.ts".into(),
      word: "cat".into()
    }),
    "it writes the whole of server/one.ts with a redirect"
  );
}

// Other workers' checkouts (docs/worker-slots.md, 「A job writes only in its own checkout」). Out of
// server/worker-slots.test.ts:
// "is named however its path is spelled, and never by a worktree whose path merely begins with it"
// "is not a write where the command only reads it"
// "a job of another repository is refused a write into the order's worktree, borrows one of its own, and the order's parking takes none of its files"
#[test]
fn another_workers_checkout_as_the_job_guard_reads_a_write_into_it() {
  let other = ForeignCheckout {
    worker: "worker-w2".into(),
    base: "worker".into(),
    cwd: format!("{HOME}/repos/bmo-w2"),
  };
  let checkouts = [other.clone()];

  for command in [
    format!("echo x > {HOME}/repos/bmo-w2/a.md"),
    "cp a.md ~/repos/bmo-w2/b.md".to_owned(),
    "cd $HOME/repos/bmo-w2 && git commit -am x".to_owned(),
  ] {
    assert_eq!(
      foreign_command_write(&command, &checkouts, HOME).map(|(found, _)| found),
      Some(&other),
      "{command}"
    );
  }

  assert_eq!(foreign_command_write(&format!("cat {HOME}/repos/bmo-w2/a.md"), &checkouts, HOME), None);
  assert_eq!(
    foreign_command_write(&format!("git -C {HOME}/repos/bmo-w2 log --oneline -3"), &checkouts, HOME),
    None
  );
  assert_eq!(
    foreign_command_write(&format!("grep -rn x {HOME}/repos/bmo-w2/server"), &checkouts, HOME),
    None
  );
  assert_eq!(
    foreign_command_write(&format!("echo x > {HOME}/repos/bmo-w23/a.md"), &checkouts, HOME),
    None
  );
  assert_eq!(foreign_file_write(&format!("{HOME}/repos/bmo-w2/a.md"), &checkouts), Some(&other));
  assert_eq!(foreign_file_write(&format!("{HOME}/repos/bmo-w23/a.md"), &checkouts), None);
}

fn plain() -> ShellCall {
  ShellCall::default()
}

fn habit(command: &str) -> Option<String> {
  habit_refusal(command, plain())
}

// The shell habits that stall a job, each beside the way the refusal asks for it to be written, which must
// pass, and the everyday commands that only look like one. Out of repo/repo/habit-guard.test.ts:
// "refuses ${JSON.stringify(command)}"
// "lets through ${JSON.stringify(command)}"
#[test]
fn a_reader_named_nothing_to_read() {
  for command in [
    "ls",
    "cd /tmp && ls -la",
    "eza --icons=always",
    "ls -L 2",
    "cat",
    "cat -",
    "jq '.a'",
    "jq -r '.a[]'",
    "grep -n foo",
    "sed -n 1p",
    "python3",
    "python3 -",
    "head -n 5",
    "tr a b",
    "(cat)",
  ] {
    assert!(habit(command).is_some_and(|refused| refused.contains("standard input")), "{command}");
  }

  for command in [
    "ls -la /tmp",
    "ls .",
    "ls </dev/null",
    "ls -L 2 docs",
    "cat README.md",
    "echo a | cat",
    "git log | head -5",
    "jq '.a' file.json",
    "jq -n '1+1'",
    "jq -f prog.jq data.json",
    "grep -rn foo",
    "grep -e foo docs/a.md",
    "grep -n foo docs/a.md",
    "sed -n 1p a.md",
    "sed -e s/a/b/ a.md",
    "python3 script.py",
    "python3 -c 'print(1)'",
    "python3 -m http.server",
    "python3 - <<'EOF'\nprint(1)\nEOF",
    "cat <<'EOF' > a.txt\nhello\nEOF",
    "jq '.a' <<< '{}'",
    "while read -r line; do grep x; done < list.txt",
    "diff <(sort a) b",
    "cat <(ls /tmp)",
    "rg foo",
    "wc -l a b",
    "a || cat file",
  ] {
    assert_eq!(habit(command), None, "{command}");
  }
}

// "refuses ${JSON.stringify(command)} under the tool's own wait"
// "lets ${JSON.stringify(command)} through with a wait that covers it, or in the background"
// "leaves ${JSON.stringify(command)} alone"
#[test]
fn a_known_long_command_with_no_wait_asked_for() {
  for command in [
    "bun scripts/job-close.ts --then 'x'",
    "bun scripts/check.ts --since main",
    "~/.local/share/bmo/deploy/bin/bmo check a.ts",
    "~/.local/share/bmo/deploy/bin/bmo job close --then 'x'",
    "bmo test suite --since main",
    "bmo test-suite",
    "cd /srv/user/repos/bench && bun /srv/user/.local/share/bmo/deploy/scripts/job-close.ts --skip-suite 'docs'",
    "bun test",
    "bun test --parallel",
    "~/.local/share/bmo/deploy/bin/bmo fold-worker",
    "bmo ask --question 'x'",
    "~/.local/share/bmo/deploy/bin/bmo worker fold",
    "bmo job ask --question 'x'",
    "bmo job land",
    "~/.local/share/bmo/deploy/bin/bmo testbed --narrow -- cargo test",
    "~/.local/share/bmo/deploy/bin/bmo testbed run --narrow -- cargo test",
    "bmo-testbed -- bun test a.test.ts b.test.ts",
    "git push",
    "git -C /tmp/x push origin main",
    "cd client && bunx nuxi typecheck",
    "cd client && bun run build",
    "bunx vue-tsc --noEmit",
  ] {
    assert!(habit(command).is_some_and(|refused| refused.contains("one-minute wait")), "{command}");
    assert!(
      habit_refusal(
        command,
        ShellCall {
          timeout_ms: Some(120_000),
          background: false
        }
      )
      .is_some_and(|refused| refused.contains("one-minute wait"))
    );
    assert_eq!(
      habit_refusal(
        command,
        ShellCall {
          timeout_ms: Some(LONG_COMMAND_WAIT_MS),
          background: false
        }
      ),
      None
    );
    assert_eq!(
      habit_refusal(
        command,
        ShellCall {
          timeout_ms: None,
          background: true
        }
      ),
      None
    );
  }

  for command in [
    "bun test --parallel repo/repo/habit-guard.test.ts",
    "git status",
    "git log --oneline -5",
    "bun scripts/edit.ts a.ts --replace a b",
    "bunx tsc --noEmit",
    "bmo testbed status",
    "bmo-testbed status --json",
  ] {
    assert_eq!(habit(command), None, "{command}");
  }
}

// "refuses ${JSON.stringify(command)}"
// "lets through ${JSON.stringify(command)}"
#[test]
fn a_full_match_that_finds_its_own_shell() {
  for command in [
    "pkill -f voice-cer.py",
    "pkill -9 -f bench",
    "pgrep -f vite && echo up",
    "kill $(pgrep -f vite)",
    "until ! pgrep -f 'voice-cer.py' > /dev/null; do sleep 10; done",
  ] {
    assert!(habit(command).is_some_and(|refused| refused.contains("-f")), "{command}");
  }

  for command in [
    "pgrep -af vite",
    "pgrep -fl vite",
    "pkill vite",
    "pgrep vite",
    "systemctl --user stop bmo-web",
  ] {
    assert_eq!(habit(command), None, "{command}");
  }
}

// "refuses ${JSON.stringify(command)}"
// "lets through ${JSON.stringify(command)}"
// "reads a glob only outside quotes and expansions"
#[test]
fn a_pattern_the_command_expands_for_itself_left_unquoted() {
  for command in [
    "grep -rn foo --include=*.ts .",
    "find . -name *.ts",
    "rg -g *.vue foo client",
    "find docs -path */lore/* -prune",
    "grep -r x --exclude-dir=node_* .",
  ] {
    assert!(habit(command).is_some_and(|refused| refused.contains("Quote it")), "{command}");
  }

  for command in [
    "grep -rn foo --include='*.ts' .",
    "find . -name '*.ts'",
    r#"rg -g "*.vue" foo client"#,
    "find . -name a.ts",
    "bun test --timeout=5000 a.test.ts",
    "echo --file=${S:?}",
  ] {
    assert_eq!(habit(command), None, "{command}");
  }

  assert!(unquoted_glob("*.ts"));
  assert!(!unquoted_glob("'*.ts'"));
  assert!(unquoted_glob(r#""a"*"#));
  assert!(!unquoted_glob("${S:?}"));
  assert!(!unquoted_glob("\\*.ts"));
}

// "refuses ${JSON.stringify(command)}"
// "lets through ${JSON.stringify(command)}"
#[test]
fn a_sleeping_wait_with_no_bound() {
  for command in [
    "until [ -f done ]; do sleep 5; done",
    "while true; do curl -s localhost:3000 && break; sleep 2; done",
    "bash -c 'while ! test -f x; do sleep 1; done'",
  ] {
    assert!(habit(command).is_some_and(|refused| refused.contains("bounded")), "{command}");
  }

  for command in [
    "timeout 300 zsh -c 'until [ -f done ]; do sleep 5; done'",
    "for i in {1..30}; do test -f done && break; sleep 2; done",
    "while read -r f; do echo $f; done < list.txt",
    "sleep 2 && ls /tmp",
    "wait $pid",
  ] {
    assert_eq!(habit(command), None, "{command}");
  }
}

// "a heredoc body is text, not commands", in "the text a command carries"
#[test]
fn a_heredoc_body_is_text_not_commands() {
  assert_eq!(habit("cat > run.sh <<'EOF'\nls\npkill -f x\nEOF"), None);
}

fn sent(given: Value, command: &str) -> Map<String, Value> {
  let answer: Value =
    serde_json::from_str(&closed_stdin_answer(given.as_object().unwrap(), command)).unwrap();

  answer["hookSpecificOutput"]["updatedInput"].as_object().unwrap().clone()
}

// "puts the closing line ahead of the command once, and gives back the rest of the call"
#[test]
fn puts_the_closing_line_ahead_of_the_command_once_and_gives_back_the_rest_of_the_call() {
  let given = json!({ "command": "rg -n foo", "timeout": 600000, "description": "search" });
  let mut expected = given.as_object().unwrap().clone();

  expected.insert("command".into(), json!(format!("{CLOSED_STDIN}\nrg -n foo")));

  assert_eq!(sent(given.clone(), "rg -n foo"), expected);
  assert_eq!(
    sent(given, &format!("{CLOSED_STDIN}\nrg -n foo"))["command"],
    json!(format!("{CLOSED_STDIN}\nrg -n foo"))
  );
}

fn shell_output(command: &str) -> String {
  let mut shell = std::process::Command::new("/bin/sh")
    .args(["-c", command])
    .stdin(std::process::Stdio::piped())
    .stdout(std::process::Stdio::piped())
    .spawn()
    .unwrap();
  // The tool hands the shell a pipe it never writes to or closes; the rewritten command still ends.
  let held = shell.stdin.take();
  let done = shell.wait_with_output().unwrap();

  drop(held);
  String::from_utf8_lossy(&done.stdout).into_owned()
}

// "ends a reader named nothing on a shell whose standard input never closes"
// "still feeds a heredoc to the command it stands on"
#[test]
fn ends_a_reader_named_nothing_on_a_shell_whose_standard_input_never_closes() {
  let ended = sent(json!({}), "echo x </dev/null; cat; echo ended")["command"]
    .as_str()
    .unwrap()
    .to_owned();
  let fed = sent(json!({}), "cat <<'EOF'\nfed\nEOF")["command"].as_str().unwrap().to_owned();

  assert_eq!(shell_output(&ended), "x\nended\n");
  assert_eq!(shell_output(&fed), "fed\n");
}

// The removal shapes a job's pane stops on, and the ones written the way the refusal asks for, which must
// pass. Out of repo/repo/removal-guard.test.ts:
// "the refusal names the target and says how to write it"
#[test]
fn removal_refusal_reads_the_targets_the_cli_stops_on() {
  for command in [
    "rm -f $S/*.md",
    "S=/tmp/x; rm -rf $S/",
    r#"rm -f "$S"/a.md"#,
    "rm -f ${S}/a.md",
    "rm -f ${S:-/tmp}/a.md",
    "rm -f $P/$(cat f).jsonl",
    r#"S=work; echo "$(cat $S/a.md)"; rm -f $S/*.md"#,
    "cd /tmp && rm -rf `pwd`/x",
    "sudo rm -rf $HOME/x",
    "timeout 5 rm -f $1/x",
    "x=$(ls); rm $x",
    "bash -c 'rm -rf $D/x'",
    r#"zsh -c "rm -rf \$D/x""#,
    "eval 'rm -rf $D/x'",
    "echo $(rm -rf $D/x)",
    "if true; then rm -r $D; fi",
    "cat > run.sh <<'EOF'\nrm -f $S/x\nEOF",
    "rm -f -- $S",
  ] {
    assert!(
      removal_refusal(command).is_some_and(|refused| refused.contains("refused:")),
      "{command}"
    );
  }

  for command in [
    r#"rm -f "${S:?}"/*.md"#,
    "rm -f ${S:?}/x",
    r#"rm -rf "${S?scratchpad unset}/build""#,
    r#"S=work; echo "$(cat "$S"/a.md)"; rm -f "${S:?}"/*.md"#,
    "rm -f /tmp/a.md",
    "rm -rf node_modules dist",
    "echo '$S'; rm x",
    "rm -f 'a$b'",
    "git rm $f",
    "find . -name '*.tmp' -delete",
    "ls $S | xargs rm",
    "grep -rn 'rm -rf $S' docs",
    "echo rmdir $S",
    "bun test --parallel repo/repo/removal-guard.test.ts",
    "# rm -rf $S/\nls /tmp",
    "rm -f a.$$.tmp",
  ] {
    assert_eq!(removal_refusal(command), None, "{command}");
  }

  let reason = removal_refusal("rm -f $S/*.md").unwrap();

  assert!(reason.contains("$S/*.md"));
  assert!(reason.contains(r#""${S:?}"/*.md"#));
  assert!(reason.contains("~/.claude"));
}

// "reads a substitution and an unguarded expansion apart", in "unsafeTarget"
#[test]
fn reads_a_substitution_and_an_unguarded_expansion_apart() {
  assert_eq!(unsafe_target("$(pwd)/x"), Some(Unsafe::Substitution));
  assert_eq!(unsafe_target(r#""$D"/x"#), Some(Unsafe::Expansion));
  assert_eq!(unsafe_target("'$D'/x"), None);
  assert_eq!(unsafe_target(r#""${D:?}"/x"#), None);
  assert_eq!(unsafe_target("${D:?}/${E}"), Some(Unsafe::Expansion));
}

// The first Hangul syllable: test files carry no Hangul of their own.
const SYLLABLE: char = '\u{AC00}';

// A commit message, a tag or a branch name holding Hangul is refused with the rule, wherever the command
// line shows it; an English one, the names of address the close sets aside and a message read from a file
// pass.
#[test]
fn git_language_refusal_reads_what_goes_into_git() {
  let policy = crate::support::queue::policy();
  let english = EnglishTest::of(&policy);
  let korean = SYLLABLE.to_string().repeat(3);

  for command in [
    format!(r#"git commit -m "{korean}""#),
    format!("git commit -am '{korean} fix'"),
    format!("git commit -m 'Fix the guard' -m '{korean}'"),
    format!("git commit --message={korean}"),
    format!("git -C /tmp/x commit -q -m {korean}"),
    format!("git add -A && git commit -q -F - <<'EOF'\nFix the guard\n\n{korean}\nEOF"),
    format!("git commit -m \"$(cat <<'EOF'\n{korean}\nEOF\n)\""),
    format!("git tag -a v1 -m '{korean}'"),
    format!("git tag {korean}"),
    format!("git branch work/{korean}"),
    format!("git branch -m old {korean}"),
    format!("git switch -c {korean}"),
    format!("git checkout -b {korean}"),
    format!("bash -c 'git commit -m {korean}'"),
  ] {
    let reason = git_language_refusal(&command, &english);

    assert!(
      reason
        .as_deref()
        .is_some_and(|refused| refused.contains("refused:") && refused.contains("English only")),
      "{command}"
    );
  }

  let names = policy.close.english.set_aside.split('|').collect::<Vec<_>>();

  for command in [
    r#"git commit -m "Fix the guard""#.to_owned(),
    "git commit -q -F - <<'EOF'\nFix the guard\n\nBody in English.\nEOF".to_owned(),
    format!("git commit -m 'Answer {} as {} asked'", names[0], names[names.len() - 1]),
    "git switch -c work/1682".to_owned(),
    format!("git commit -F {korean}.txt"),
    format!("git commit -F msg.txt && echo {korean}"),
    format!("git branch -D {korean}"),
    format!("git log --grep {korean}"),
    format!("git checkout -- {korean}.md"),
    format!("grep -rn '{korean}' docs"),
  ] {
    assert_eq!(git_language_refusal(&command, &english), None, "{command}");
  }
}

// The hook as a job's program runs it (docs/verbs.md, "The shell hook"): its input as the program hands it,
// the job's markers, the home and the directory it runs in, on a stand of the test's own whose units are
// stood in.
struct Hook {
  desk: crate::support::desk::Desk,
  brain: String,
  here: tempfile::TempDir,
}

// The unit of a work order's step, which names its item.
const WORK_UNIT: &str = "bmo-work-331-stage-4-0-1";

fn in_job(run: &str) -> JobProcess {
  JobProcess {
    run_name: Some(run.to_owned()),
    in_job: true,
    work_order_step: false,
  }
}

impl Hook {
  async fn new() -> Self {
    let here = tempfile::tempdir().unwrap();
    // The brain stands beside the checkout the hook runs in, never inside it.
    let brain = format!("{}-brain", here.path().display());
    let desk = crate::support::desk::Desk::with(&format!("brain = \"{brain}\"\n")).await;

    Self { desk, brain, here }
  }

  async fn call(&self, input: &Value, process: &JobProcess) -> String {
    let here = self.here.path().display().to_string();
    let hooked = Hooked {
      settings: &self.desk.settings,
      process,
      home: HOME,
      here: &here,
    };

    hook(&hooked, &input.to_string()).await.lines.join("\n")
  }

  async fn bash(&self, command: &str, process: &JobProcess) -> String {
    self
      .call(&json!({ "tool_name": "Bash", "tool_input": { "command": command } }), process)
      .await
  }

  // Whether the hook let the command through, which it answers with the call sent on as it was.
  async fn let_through(&self, command: &str, process: &JobProcess) -> bool {
    self.bash(command, process).await
      == closed_stdin_answer(json!({ "command": command }).as_object().unwrap(), command)
  }

  async fn open_job(&self, unit: &str, todo: Option<i64>, worker: &str) -> i64 {
    self
      .desk
      .run(
        r#"INSERT INTO "jobs" ("chat_id", "unit", "launched_at", "prompt", "todo_id", "worker") VALUES (1, $1, 1, 'work', $2, $3)"#,
        vec![unit.into(), todo.into(), worker.into()],
      )
      .await;
    self.desk.count(r#"SELECT MAX("id") FROM "jobs""#, vec![]).await
  }

  async fn reset(&self) {
    for table in [
      "live_claude_runs",
      "live_claude_openings",
      "jobs",
      "todos",
      "checkout_leases",
      "workers",
    ] {
      self.desk.run(&format!(r#"DELETE FROM "{table}""#), vec![]).await;
    }
  }
}

fn denial(out: &str) -> (String, String) {
  let decision: Value = serde_json::from_str(out).unwrap_or_else(|_| panic!("not an answer: {out}"));

  (
    decision["hookSpecificOutput"]["permissionDecision"]
      .as_str()
      .unwrap_or_default()
      .to_owned(),
    decision["hookSpecificOutput"]["permissionDecisionReason"]
      .as_str()
      .unwrap_or_default()
      .to_owned(),
  )
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_command_is_denied_with_the_way_to_open_it_and_one_let_through_runs_with_its_input_closed()
 {
  let hook = Hook::new().await;
  let outside = JobProcess::default();
  let (decision, reason) = denial(&hook.bash(FORK, &outside).await);

  assert_eq!(decision, "deny");
  assert!(reason.contains("job live-claude --purpose"), "{reason}");
  assert_eq!(
    hook.bash("git status", &outside).await,
    json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "updatedInput": { "command": "exec </dev/null\ngit status" } } })
      .to_string()
  );

  // A job that has said what for is let through, and the run kept against its opening; another job is not.
  let job = hook.open_job(WORK_UNIT, Some(331), "worker").await;

  hook
    .desk
    .run(
      r#"INSERT INTO "live_claude_openings" ("job_id", "todo_id", "purpose", "opened_at") VALUES ($1, 331, 'measure /compact on a copied session', 1)"#,
      vec![job.into()],
    )
    .await;

  assert!(hook.let_through(FORK, &in_job(WORK_UNIT)).await);
  assert!(hook.let_through("git status", &in_job(WORK_UNIT)).await);
  assert!(hook.bash(FORK, &in_job("bmo-task-other")).await.contains("deny"));
  assert_eq!(
    hook
      .desk
      .count(
        r#"SELECT COUNT(*) FROM "live_claude_runs" WHERE "what" = $1"#,
        vec![format!("shell: {}", FORK.chars().take(200).collect::<String>()).into()]
      )
      .await,
    1
  );
  assert_eq!(hook.desk.count(r#"SELECT COUNT(*) FROM "live_claude_runs""#, vec![]).await, 1);

  // Print mode, whatever the job has opened; the interactive program stays the opening's to let through.
  let (decision, reason) = denial(&hook.bash("claude -p x", &in_job(WORK_UNIT)).await);

  assert_eq!(decision, "deny");
  assert!(reason.contains("print mode"), "{reason}");
  assert!(hook.let_through("claude --resume x", &in_job(WORK_UNIT)).await);

  // A removal the program would stop on, whatever the job has opened.
  let (decision, reason) = denial(
    &hook
      .bash(r#"S=/tmp/x; echo "$(cat $S/what.md)"; rm -f $S/*.md"#, &in_job(WORK_UNIT))
      .await,
  );

  assert_eq!(decision, "deny");
  assert!(reason.contains(r#""${S:?}"/*.md"#), "{reason}");
  assert!(
    hook
      .let_through(r#"S=/tmp/x; echo "$(cat "$S"/what.md)"; rm -f "${S:?}"/*.md"#, &outside)
      .await
  );

  // Input it cannot read stops nothing.
  let here = hook.here.path().display().to_string();
  let hooked = Hooked {
    settings: &hook.desk.settings,
    process: &outside,
    home: HOME,
    here: &here,
  };

  assert!(bmo_job::verbs::job_guard::hook(&hooked, "not json").await.lines.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn every_login_s_records_are_refused_as_the_agency_names_them_and_the_home_s_beside_them() {
  let hook = Hook::new().await;
  let logins = tempfile::tempdir().unwrap();
  let login = logins.path().join("claude-main");
  let edit = format!("sed -i s/a/b/ {}/projects/-x/y.jsonl", login.display());
  let outside = JobProcess::default();

  // The agency names no login: only the home's own records are the program's.
  hook.desk.agency.answer("GET /accounts/claude/homes", 200, json!({ "homes": [] }));
  assert!(hook.let_through(&edit, &outside).await);
  assert!(
    hook
      .bash("sed -i s/a/b/ ~/.claude/projects/-x/y.jsonl", &outside)
      .await
      .contains("session records")
  );

  hook.desk.agency.answer(
    "GET /accounts/claude/homes",
    200,
    json!({ "homes": [login.display().to_string()] }),
  );
  assert!(hook.bash(&edit, &outside).await.contains("session records"));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_structure_document_is_changed_only_on_the_owner_s_own_request_naming_it() {
  let hook = Hook::new().await;
  let structure = json!({ "tool_name": "Edit", "tool_input": { "file_path": format!("{ROOT}/STRUCTURE.md"), "old_string": "a", "new_string": "b" } });
  let owner = crate::support::desk::OWNER_SENDER;

  for (requester, title, allowed) in [
    (owner, "add a term to STRUCTURE.md", true),
    (owner, "tidy the documents", false),
    (7_000_000_007, "fix STRUCTURE.md", false),
  ] {
    hook.reset().await;
    hook
      .desk
      .run(
        r#"INSERT INTO "todos" ("id", "title", "instruction", "requester", "requester_id", "chat_id", "approved", "priority", "status", "created_at")
           VALUES (40, $1, 'change the document', 'Owner', $2, 1, 1, 1, 'approved', 1)"#,
        vec![title.into(), requester.into()],
      )
      .await;
    hook.open_job(WORK_UNIT, Some(40), "worker").await;

    let out = hook.call(&structure, &in_job(WORK_UNIT)).await;

    assert_eq!(out.is_empty(), allowed, "{title}: {out}");

    if !allowed {
      assert!(out.contains("deny"));
      assert!(hook.bash("echo x > STRUCTURE.md", &in_job(WORK_UNIT)).await.contains("STRUCTURE.md"));
    }
  }

  // Nothing the approval needs is guessed at: a run with no row, or none at all, is refused.
  assert!(hook.call(&structure, &in_job("bmo-work-1-stage-0-0-1")).await.contains("deny"));
  assert!(hook.call(&structure, &JobProcess::default()).await.contains("deny"));
  assert_eq!(
    hook
      .call(
        &json!({ "tool_name": "Write", "tool_input": { "file_path": format!("{ROOT}/README.md") } }),
        &JobProcess::default()
      )
      .await,
    ""
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_write_into_the_checkout_is_refused_with_the_verb_and_the_verb_and_a_read_go_through() {
  let hook = Hook::new().await;
  let outside = JobProcess::default();
  let (decision, reason) = denial(&hook.bash("sed -i 's/alpha/beta/' server/sample.ts", &outside).await);

  assert_eq!(decision, "deny");
  assert!(reason.contains("server/sample.ts"), "{reason}");
  assert!(reason.contains("This repository has a verb for that"), "{reason}");
  assert!(hook.let_through("bmo edit server/sample.ts --replace alpha beta", &outside).await);
  assert!(hook.let_through("sed -n '1,20p' server/sample.ts", &outside).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_s_first_write_into_the_brain_sets_its_hold_and_a_write_under_an_editor_s_lock_is_refused() {
  let hook = Hook::new().await;
  let brain = hook.brain.clone();
  let holds = || {
    hook
      .desk
      .brain
      .calls()
      .into_iter()
      .filter(|call| call.route == "POST /brain/hold")
      .count()
  };

  hook.open_job(WORK_UNIT, Some(456), "worker").await;
  hook
    .desk
    .brain
    .answer("POST /brain/hold", 200, json!({ "answered": true, "held": true, "taken": [] }));

  // A read holds nothing, and a write outside a job holds nothing either.
  assert_eq!(
    hook
      .call(&json!({ "tool_name": "Read", "tool_input": { "file_path": format!("{brain}/knowledge/chunk.md") } }), &in_job(WORK_UNIT))
      .await,
    ""
  );
  assert!(hook.let_through(&format!("cat {brain}/knowledge/index.md"), &in_job(WORK_UNIT)).await);
  assert!(
    hook
      .let_through(&format!("echo x > {brain}/knowledge/chunk.md"), &JobProcess::default())
      .await
  );
  assert_eq!(holds(), 0);

  assert_eq!(
    hook
      .call(&json!({ "tool_name": "Write", "tool_input": { "file_path": format!("{brain}/knowledge/chunk.md") } }), &in_job(WORK_UNIT))
      .await,
    ""
  );
  assert_eq!(holds(), 1);
  assert_eq!(hook.desk.brain.calls()[0].body, Some(json!({ "run": WORK_UNIT })));

  // A brain editor holding its lock refuses the write, saying how to wait.
  hook.desk.brain.answer(
    "POST /brain/hold",
    200,
    json!({ "answered": true, "held": true, "taken": ["people-digest"] }),
  );

  let commit = format!("cd {brain} && git commit -am 'translate'");
  let (decision, reason) = denial(&hook.bash(&commit, &in_job(WORK_UNIT)).await);

  assert_eq!(decision, "deny");
  assert!(reason.contains("people-digest") && reason.contains("wait"), "{reason}");
  assert!(hook.let_through("git -C /tmp/x commit -m y", &in_job(WORK_UNIT)).await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_is_refused_a_write_into_another_worker_s_checkout_and_not_into_one_it_borrowed() {
  let hook = Hook::new().await;

  for (name, cwd, base) in [
    ("worker", ROOT.to_owned(), None),
    ("worker-w2", format!("{HOME}/repos/bench-w2"), Some("worker")),
    ("worker-w3", format!("{HOME}/repos/bench-w3"), Some("worker")),
    ("gate", format!("{HOME}/repos/gate"), None),
  ] {
    hook
      .desk
      .run(
        r#"INSERT INTO "workers" ("name", "label", "cwd", "builtin", "support_count", "support_of", "created_at") VALUES ($1, $1, $2, 0, 0, $3, 1)"#,
        vec![name.into(), cwd.into(), base.into()],
      )
      .await;
  }

  let other = hook.open_job("bmo-task-739", Some(739), "gate").await;

  hook
    .desk
    .run(
      r#"INSERT INTO "checkout_leases" ("worker", "job_id", "taken_at") VALUES ('worker-w3', $1, 1)"#,
      vec![other.into()],
    )
    .await;

  let seen = job_checkouts(&hook.desk.stand.db, "bmo-task-739").await;
  let gate = in_job("bmo-task-739");
  let edit = |path: String| json!({ "tool_name": "Write", "tool_input": { "file_path": path } });

  assert_eq!(seen.own.as_deref(), Some(format!("{HOME}/repos/gate").as_str()));
  assert_eq!(
    seen
      .foreign
      .iter()
      .map(|checkout| (checkout.worker.as_str(), checkout.base.as_str()))
      .collect::<Vec<_>>(),
    [("worker", "worker"), ("worker-w2", "worker")]
  );

  let (decision, reason) =
    denial(&hook.call(&edit(format!("{HOME}/repos/bench-w2/server/new.ts")), &gate).await);

  assert_eq!(decision, "deny");
  assert!(reason.contains("job borrow worker"), "{reason}");
  assert!(
    hook
      .bash(&format!("cd {HOME}/repos/bench-w2 && git add -A && git commit -m x"), &gate)
      .await
      .contains("worker-w2")
  );
  assert_eq!(hook.call(&edit(format!("{HOME}/repos/bench-w3/server/new.ts")), &gate).await, "");
  assert_eq!(hook.call(&edit(format!("{HOME}/repos/gate/gate.ts")), &gate).await, "");
}
