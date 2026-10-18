//! A job's run, in the pieces that stand without a database (docs/runs.md): what a failed turn was, a pane's
//! question and a command the runner allows itself, a turn's earlier results and its item switch, the session
//! a later turn goes on, the ending's phases, the notes handed a running job, what the cache siren is told,
//! the judgement of a last answer, and the parts a prompt is made of.

mod support;

use std::collections::{HashMap, HashSet};
use std::path::Path;

use bmo_job::job_run::claude::confirm::{
  ANSWERED_IN_PANE, confirmation_answer, is_destructive, recommended,
};
use bmo_job::job_run::claude::failure::{
  FAILURE_TEXT_LIMIT, Failure, failure_text, of_errored, of_unanswered,
};
use bmo_job::job_run::claude::own_work::{Places, command_of, own_work};
use bmo_job::job_run::claude::turn::{item_switch_notice, open_work_results, shape_turn_prompt};
use bmo_job::job_run::drive::{own_note_of, resume_of, selected, selection_of};
use bmo_job::job_run::ending::{ends_as_job, job_phases, usage_spans};
use bmo_job::job_run::notes::{Note, follow_up_task};
use bmo_job::job_run::prompt::{
  JobItem, assembled, dev_verb, impact_block, instruction_line, job_lines, verbs_word,
};
use bmo_job::job_run::siren::{codex_events, spend_event};
use bmo_job::job_run::verdict::{
  ImpactFaults, abandoned_workflows, answered_with, change_names, graph_mapped, impact_calls,
  impact_faults, impact_note, left_impact, left_open_item, nothing_reported, open_item_note,
  report_shape_fault, shape_note, sweep_note, unchecked_note, uncommitted_note, unfinished_lines,
  unrecorded_change, unswept_screens,
};
use bmo_job::job_run::{WORK_FINAL_PHASE, WORK_STEP_PHASE};
use job_agency::{
  AccountFailure, ConfirmAnswer, Confirmation, LiveTask, ModelSpend, Reply, RequestUsage, Resume,
  RunEnding, RunWorkflow, TurnFailure, TurnSpan,
};
use job_codes::job_ended::Selection;
use job_policy::Names;
use queue_read::impact::ImpactGrounds;
use queue_rows::closing::Reader;
use support::queue::policy;

// ---- what a failed turn was

#[test]
fn a_full_window_is_a_limit_with_the_reset_it_names() {
  assert_eq!(
    of_unanswered("Claude AI usage limit reached|1791430000", "job-worker"),
    Failure::Limit {
      resets_at: Some(1_791_430_000),
      account: None,
      silenced: false,
    }
  );
  assert_eq!(
    of_unanswered("You've hit your session limit · resets 3pm", "job-worker"),
    Failure::Limit {
      resets_at: None,
      account: None,
      silenced: false,
    }
  );
}

#[test]
fn a_login_gone_is_read_off_the_last_line_and_named_by_the_key() {
  let failed = of_unanswered("banner\n\nInvalid API key · Please run /login\n", "job-worker");

  assert_eq!(
    failed,
    Failure::Account {
      failure: AccountFailure::Login,
      detail: "job-worker: banner\nInvalid API key · Please run /login".to_owned(),
    }
  );
  assert_eq!(failed.account_failure(), Some(AccountFailure::Login));

  // A long-lived token expired or revoked, in the words the CLI says it.
  for said in [
    "OAuth token has expired",
    "Failed to authenticate: OAuth token revoked.",
    "Invalid bearer token",
  ] {
    assert_eq!(
      of_unanswered(said, "job-worker").account_failure(),
      Some(AccountFailure::Login),
      "{said}"
    );
  }
}

#[test]
fn anything_else_died_with_the_tail_of_what_it_said() {
  let failed =
    of_unanswered("\x1b[31m────\x1b[0m\nNo conversation found with session ID x\n", "job-worker");

  assert_eq!(
    failed,
    Failure::Died {
      ending: RunEnding::Exit { code: 1 },
      said: Some("No conversation found with session ID x".to_owned()),
      message: "claude exited with code 1: No conversation found with session ID x".to_owned(),
    }
  );
  assert_eq!(failed.account_failure(), None);
}

#[test]
fn a_long_text_keeps_its_tail_from_a_whole_line() {
  let long = format!("{}\n{}", "a".repeat(10), "b".repeat(FAILURE_TEXT_LIMIT));
  let kept = failure_text(&format!("{long}\nlast"));

  assert!(kept.starts_with("…\n"));
  assert!(kept.ends_with("last"));
}

#[test]
fn a_program_s_server_error_is_the_service_being_out() {
  assert_eq!(
    of_errored("API Error: 529 overloaded", "job-worker").account_failure(),
    Some(AccountFailure::Outage)
  );
  assert!(matches!(of_errored("API Error: 400 bad request", "job-worker"), Failure::Died { .. }));
}

#[test]
fn a_failure_bmo_agency_read_comes_by_its_kind_and_is_named_by_its_error() {
  let unavailable: Failure = TurnFailure::Unavailable {
    scope: "job-worker:worker".into(),
    mode: "codex".into(),
    reason: String::new(),
  }
  .into();

  assert_eq!(
    unavailable.cause(),
    "SelectedRouteUnavailableError: job-worker:worker: selected codex route did not answer"
  );
  assert_eq!(Failure::from(TurnFailure::Person).name(), "JobWaitingOnPerson");
  assert_eq!(Failure::Exhausted { attempted: vec![] }.name(), "Error");
}

// ---- a pane's question and a command the runner allows itself

#[test]
fn only_a_command_nothing_restores_after_is_recommended_refused() {
  assert!(is_destructive("rm -rf /"));
  assert!(is_destructive("rm -rf ~/"));
  assert!(is_destructive("git push --force origin main"));
  assert!(!is_destructive("rm -rf build"));
  assert!(!is_destructive("git push origin main"));
  assert_eq!(
    recommended(&Confirmation::Permission {
      text: "mkfs /dev/sda".into(),
      amendable: false
    }),
    1
  );
  assert_eq!(
    recommended(&Confirmation::Unknown {
      screen: String::new()
    }),
    0
  );
}

fn asked(chosen: Option<i64>, answer: Option<&str>, by: &str) -> bmo_job::queue::questions::Question {
  bmo_job::queue::questions::Question {
    id: 1,
    job_id: Some(41),
    todo_id: 7,
    chat_id: 1,
    question: "rm x".to_owned(),
    options: vec!["allow".to_owned(), "refuse".to_owned()],
    recommended: 0,
    why: "w".to_owned(),
    asked_at: 0,
    due_at: 60_000,
    chosen,
    answer: answer.map(str::to_owned),
    paused_left: None,
    settled_at: Some(1),
    settled_by: Some(by.to_owned()),
    talks: false,
  }
}

#[test]
fn a_settled_question_answers_the_pane() {
  let permission = Confirmation::Permission {
    text: "rm x".to_owned(),
    amendable: false,
  };

  assert_eq!(confirmation_answer(&permission, &asked(None, None, "deadline")), ConfirmAnswer::Allow);
  assert_eq!(
    confirmation_answer(&permission, &asked(Some(1), None, "owner")),
    ConfirmAnswer::Refuse { words: None }
  );
  assert_eq!(
    confirmation_answer(&permission, &asked(None, Some("not that one"), "owner")),
    ConfirmAnswer::Refuse {
      words: Some("not that one".into())
    }
  );
  assert_eq!(
    confirmation_answer(&permission, &asked(None, Some(ANSWERED_IN_PANE), "owner")),
    ConfirmAnswer::Leave
  );
  assert_eq!(
    confirmation_answer(
      &Confirmation::Unknown {
        screen: String::new()
      },
      &asked(Some(1), None, "owner")
    ),
    ConfirmAnswer::Leave
  );
}

const HOME: &str = "/srv/user";
const CHECKOUT: &str = "/srv/user/repos/project-w7";
const SCRATCHPAD: &str =
  "S=/tmp/agent-1000/-srv-user-repos-project-w7/22222222-2222-2222-2222-222222222222/scratchpad;";

fn places() -> Places {
  Places {
    dirs: vec![CHECKOUT.to_owned(), "/tmp".to_owned(), format!("{HOME}/data/testbed")],
    files: vec![format!("{HOME}/platform/bin/bmo"), "/dev/null".to_owned()],
    cwd: CHECKOUT.to_owned(),
    home: HOME.to_owned(),
  }
}

// A `Bash command` question as the program draws it: the description, then the command in its box with the rows
// wrapped where the screen wrapped them, then why it asks.
fn question(rows: &[&str]) -> String {
  let rule = "╌".repeat(104);

  ["Bash command", " A command", rule.as_str()]
    .into_iter()
    .map(str::to_owned)
    .chain(rows.iter().map(|row| format!(" │ {row}")))
    .chain(
      [
        rule.as_str(),
        " This shell -c script runs rm and could not be checked",
        "",
        " Do you want to proceed?",
      ]
      .map(str::to_owned),
    )
    .collect::<Vec<_>>()
    .join("\n")
}

// Three commands a stress job stopped on, each put to the owner though it stayed in its own places.
fn stressed() -> Vec<String> {
  let first = format!("{SCRATCHPAD} cd");
  let probe = format!("{SCRATCHPAD} rg -c");
  let enter =
    r#"/srv/user/repos/project-w7 && R="sh $S/stress.sh"; timeout 590 ~/platform/bin/bmo testbed run --"#;
  let tally = r#"do for c in $S/$d/c*; do printf '%s/%s ' $d $(basename $c); rg -o "^ *[0-9]+ (pass|fail)|\[[0-9.]+s\]$" $c/test.log"#;
  let close = r"| tr '\n' ' '; echo; done; done";

  vec![
    question(&[
      "exec </dev/null",
      first.as_str(),
      enter,
      r#"sh -c "for r in 4 5; do $R $S/v\${r}a 4 8 scripts/task-runner-twin.test.ts & $R $S/v\${r}b 4 8"#,
      r"scripts/task-runner-claude-twin.test.ts & $R $S/v\${r}c 4 8 scripts/task-runner-claude-twin-working.test.ts & wait;",
      r#"done" > $S/v4.log 2>&1; echo $?; tail -1 $S/v4.log | cut -c1-200"#,
    ]),
    question(&[
      "exec </dev/null",
      first.as_str(),
      enter,
      r#"sh -c "$R $S/v6a 4 8 scripts/task-runner-twin.test.ts & $R $S/v6b 4 8 scripts/task-runner-claude-twin.test.ts & $R"#,
      r#"$S/v6c 4 8 scripts/task-runner-claude-twin-working.test.ts & wait" > $S/v6.log 2>&1; echo $?; for d in v6a v6b v6c;"#,
      tally,
      close,
    ]),
    question(&[
      "exec </dev/null",
      probe.as_str(),
      r#""Unhandled|errors?$" /srv/user/data/testbed/2026-10-08T11-50-07-740Z-bun-2535299.log; cd"#,
      enter,
      r#"sh -c "$R $S/v7a 4 8 scripts/task-runner-twin.test.ts & $R $S/v7b 4 8 scripts/task-runner-claude-twin.test.ts & $R"#,
      r#"$S/v7c 4 8 scripts/task-runner-claude-twin-working.test.ts & wait" > $S/v7.log 2>&1; echo $?; for d in v7a v7b v7c;"#,
      tally,
      close,
    ]),
  ]
}

#[test]
fn the_three_commands_the_stress_job_stopped_on_are_its_own_work() {
  for text in stressed() {
    assert!(own_work(&text, &places()), "{text}");
  }
}

// A screen a job stopped on: the program named the target from the checkout, though the `cd` before it had moved
// into the item's own scratch folder.
fn scratch_removal(item: &str) -> String {
  let rule = "╌".repeat(119);

  [
    "Bash command",
    " Recapture the chat in both modes",
    rule.as_str(),
    &format!(
      " │ cd {HOME}/workbench/worker-five/items/{item}; rm -f shots/*; timeout 240 bun capture.ts before >"
    ),
    " │ before.out 2>&1; echo exit $?; rg -n 'Hash|bubbles' before.out",
    rule.as_str(),
    " │ Dangerous rm operation on statically-unresolvable target: /srv/user/repos/project-w5/shots/*",
    "",
    " Do you want to proceed?",
  ]
  .join("\n")
}

#[test]
fn a_removal_in_the_item_s_own_scratch_after_a_cd_is_its_own_work() {
  let workbench = format!("{HOME}/workbench/worker-five");
  let places = Places::of_job(
    Path::new(&format!("{HOME}/repos/project-w5")),
    Some(Path::new(&format!("{workbench}/items/1569"))),
    Some(Path::new(&format!("{HOME}/data/testbed"))),
    Path::new(&format!("{HOME}/platform")),
    Path::new(HOME),
  );

  assert!(own_work(&scratch_removal("1569"), &places));
  // Another item's scratch is no place of this job's, and the rest of the workbench is shared by every job.
  assert!(!own_work(&scratch_removal("1568"), &places));
  assert!(!own_work(&question(&[&format!("cd {workbench}; rm -f shots/*")]), &places));
  assert!(!own_work(&question(&[&format!("rm -rf {workbench}/items/1569")]), &places));
}

#[test]
fn the_command_is_read_out_of_its_box_and_no_other_shape_is_judged() {
  assert_eq!(command_of(&question(&["cd", "/tmp && ls"])).as_deref(), Some("cd /tmp && ls"));
  assert!(!own_work("Bash command\n   rm -f x\n   Remove x\n Do you want to proceed?", &places()));
}

#[test]
fn a_removal_inside_the_places_is_allowed() {
  for command in [
    format!(r#"{SCRATCHPAD} sh -c "rm -rf $S/v4a""#),
    "rm -rf build node_modules/.cache".to_owned(),
    format!("rm -f {HOME}/data/testbed/old.log"),
  ] {
    assert!(own_work(&question(&[command.as_str()]), &places()), "{command}");
  }
}

#[test]
fn a_removal_past_the_places_or_one_the_reading_cannot_resolve_goes_to_the_owner() {
  for command in [
    format!(r#"sh -c "rm -rf {HOME}/workbench/worker""#),
    format!("rm -rf {HOME}/repos/project-w3/scripts"),
    "rm -rf ~/platform/scripts".to_owned(),
    r#"rm -rf "$HOME/data""#.to_owned(),
    "rm -rf ../project-w3".to_owned(),
    format!("rm -rf {CHECKOUT}"),
    "rm -rf /tmp".to_owned(),
    format!("cd {HOME} && rm -rf repos"),
    // A shell started anew is handed only what was exported, so `$S` is empty there.
    "S=/tmp/x; sh -c 'rm -rf $S/'".to_owned(),
    "rm -rf $(cat list)".to_owned(),
    "ls | xargs rm -rf".to_owned(),
    "cd".to_owned(),
    "sudo ls /tmp".to_owned(),
    r#"R="rm -rf"; $R /tmp/x"#.to_owned(),
    format!("git -C {HOME}/repos/project-w3 status"),
    "ls --dir=/etc".to_owned(),
    // A value this reading cannot resolve is handed only to a program that only reads.
    "echo pwned > $(cat /tmp/x2.target)".to_owned(),
    r#"echo pwned | tee "$(cat /tmp/x4.target)""#.to_owned(),
    r#"cp /tmp/source.txt "$(cat /tmp/x3.target)""#.to_owned(),
    r#"dd if=/tmp/source.txt of="$(cat /tmp/x3.target)""#.to_owned(),
    r#"export D=$(cat /tmp/x5.target); echo pwned > "$D""#.to_owned(),
    "for f in $(cat /tmp/list); do cp /tmp/a $f; done".to_owned(),
  ] {
    assert!(!own_work(&question(&[command.as_str()]), &places()), "{command}");
  }

  // Rows of a loop are joined into one line, and a removal in it is still seen.
  assert!(!own_work(&question(&["for f in a b", "do rm $f", "done"]), &places()));
}

// ---- a turn's earlier results, its item switch and its shaping

#[test]
fn a_result_the_session_holds_goes_as_its_marker_and_the_rest_whole() {
  let prompt = "before\nWORK-RESULT {\"key\":\"a\",\"version\":\"1\",\"document\":null}\nalpha\nEND WORK-RESULT\nWORK-RESULT {\"key\":\"b\",\"version\":\"2\",\"document\":\"/d.md\"}\nbeta\nEND WORK-RESULT\nafter";
  let held = HashMap::from([("a".to_owned(), "1".to_owned())]);
  let (opened, sent) = open_work_results(prompt, &held);

  assert_eq!(opened, "before\n(a: already delivered earlier in this session, unchanged)\nbeta\nafter");
  assert_eq!(sent.iter().map(|head| head.key.as_str()).collect::<Vec<_>>(), ["b"]);
}

#[test]
fn a_session_that_last_answered_for_another_item_is_told_so() {
  assert!(item_switch_notice(Some(3), 4).is_some_and(|notice| {
    notice.starts_with("[Item switch]\nThe previous turn in this session was for TODO #3.")
  }));
  assert_eq!(item_switch_notice(Some(4), 4), None);
  assert_eq!(item_switch_notice(None, 4), None);
}

#[test]
fn a_format_turn_carries_the_job_s_prompt_only_to_a_session_that_did_not_write_the_design() {
  assert_eq!(shape_turn_prompt("Shape it.", "The job.", true), "Shape it.");
  assert_eq!(shape_turn_prompt("Shape it.", "The job.", false), "The job.\n\nShape it.");
}

// ---- the session a later turn goes on

fn reply(mode: &str, session: Option<&str>) -> Reply {
  Reply {
    mode: mode.to_owned(),
    model: Some("model-o".to_owned()),
    effort: None,
    text: String::new(),
    session_id: session.map(str::to_owned),
    account: None,
    context_ratio: None,
    context_tokens: None,
    session_usage: None,
    spend: vec![],
    live_tasks: vec![],
    workflows: vec![],
    upstream: None,
    request_usages: None,
  }
}

#[test]
fn a_later_turn_goes_on_the_session_and_the_road_the_last_answer_came_back_on() {
  assert_eq!(resume_of(&reply("claude-code", Some("s1"))), Resume::Session { id: "s1".into() });
  assert_eq!(resume_of(&reply("claude-code", None)), Resume::Fresh);
  assert_eq!(resume_of(&reply("codex", Some("t1"))), Resume::Auto);
  assert_eq!(selection_of(&reply("codex", None)), "codex::model-o");
  assert_eq!(selected("model-o"), None);
  assert_eq!(
    selected("codex::model-o"),
    Some(Selection {
      provider: "codex".into(),
      model: "model-o".into()
    })
  );
}

#[test]
fn a_worker_s_own_note_is_carried_only_from_the_session_it_was_asked_of() {
  assert_eq!(
    own_note_of(Some("held"), "held", "  I was finishing #1099.  "),
    Some("I was finishing #1099.".to_owned())
  );
  assert_eq!(own_note_of(Some("fresh"), "held", "another session's words"), None);
  assert_eq!(own_note_of(None, "held", "an answer with no session"), None);
  assert_eq!(own_note_of(Some("held"), "held", "None"), None);
}

// ---- the ending

#[test]
fn a_work_order_s_run_ends_as_a_job_only_as_its_final_turn_ending_done() {
  assert!(ends_as_job("", "failed"));
  assert!(!ends_as_job(WORK_STEP_PHASE, "done"));
  assert!(ends_as_job(WORK_FINAL_PHASE, "done"));
  assert!(!ends_as_job(WORK_FINAL_PHASE, "failed"));
}

#[test]
fn the_phases_cover_the_row_s_stretch_and_spending_takes_no_seconds() {
  let phases = job_phases(1_000, 3_000, 4_000, 10_000, 9_000);

  assert_eq!(
    phases.iter().map(|span| (span.name.as_str(), span.seconds)).collect::<Vec<_>>(),
    vec![("wait", 2.0), ("ready", 1.0), ("agent run", 6.0), ("finish", 0.0)]
  );

  let spend = |model: &str, tokens| ModelSpend {
    mode: None,
    model: model.into(),
    tokens,
    input_tokens: 4,
    output_tokens: 3,
    cache_read_tokens: 2,
    cache_write_tokens: 1,
    cost: None,
  };
  let spent = usage_spans(&[spend("a", 0), spend("b", 10)]);

  assert_eq!(spent.len(), 1);
  assert_eq!((spent[0].kind.as_str(), spent[0].seconds, spent[0].tokens), ("usage", 0.0, Some(10)));
}

// ---- the notes handed a running job

#[test]
fn a_change_after_a_turn_names_the_item_the_agent_and_the_way_to_reopen_it() {
  let note = |id, change| Note {
    id,
    change,
    text: "Cover the tests as well.".to_owned(),
    added_at: 1_791_430_000_000,
  };
  let asked = follow_up_task(9, &[note(1, true), note(2, false)], "/srv/platform/bin/bmo", "Agent");

  assert!(asked.starts_with("While you were working on TODO item #9, Agent changed what it asks of you."));
  assert!(asked.contains("Change to the brief (note 1, Agent, 2026-10-08T03:26:40.000Z):"));
  assert!(asked.contains("Wind-down (note 2, Agent, 2026-10-08T03:26:40.000Z):"));
  assert!(asked.contains("reopen it with /srv/platform/bin/bmo todo --reopen 9"));
  assert!(asked.ends_with("close it again with an account that includes it."));
}

// ---- what the cache siren is told

fn codex_reply(requests: Option<Vec<RequestUsage>>) -> Reply {
  Reply {
    session_id: Some("t1".into()),
    text: "done".into(),
    request_usages: requests,
    ..reply("codex", Some("t1"))
  }
}

#[test]
fn each_request_a_turn_counted_is_an_event_of_its_own() {
  let usage = |input| RequestUsage {
    input_tokens: input,
    cache_read_tokens: 0,
    cache_write_tokens: 0,
    output_tokens: 1,
  };
  let told = codex_events(
    "job-worker",
    "job-worker:worker",
    "/w/project",
    &codex_reply(Some(vec![usage(1), usage(2)])),
    "model-c",
  );

  assert_eq!(told.iter().map(|event| event.input_tokens).collect::<Vec<_>>(), vec![1, 2]);
  assert_eq!((told[0].agent.as_str(), told[0].project.as_str()), ("worker", "project"));
}

#[test]
fn a_turn_that_counted_nothing_is_one_event_of_unknown_usage() {
  let told = codex_events("job-worker", "job-worker:worker", "/w/project", &codex_reply(None), "model-c");

  assert_eq!(told.len(), 1);
  assert!(!told[0].usage_known);

  let spent = Reply {
    spend: vec![ModelSpend {
      mode: Some("openrouter".into()),
      model: "model-r".into(),
      tokens: 9,
      input_tokens: 5,
      output_tokens: 2,
      cache_read_tokens: 1,
      cache_write_tokens: 1,
      cost: None,
    }],
    ..reply("openrouter", None)
  };
  let event = spend_event("job-worker", "job-worker:worker", "/w/project", &spent, "model-r");

  assert!(event.usage_known);
  assert_eq!(event.input_tokens, 5);
}

// ---- the judgement of a last answer

#[test]
fn a_reply_of_silence_markers_alone_reports_nothing() {
  assert!(nothing_reported("  <<SKIP>>\n<<WAIT>> "));
  assert!(!nothing_reported("<<SKIP>> done it"));
}

#[test]
fn a_report_is_held_to_three_paragraphs_and_a_short_closing_with_figures() {
  assert_eq!(
    report_shape_fault("one\n\ntwo"),
    Some("it has 2 paragraph(s) rather than the three of summary, detail and conclusion".to_owned())
  );
  assert_eq!(report_shape_fault("Summary\n\nDetail\n\n1 done"), None);
  assert_eq!(
    report_shape_fault("Summary\n \t\nDetail\n\nend"),
    Some("its closing line carries no figures".to_owned())
  );
  assert_eq!(
    report_shape_fault(&format!("Summary\n\nDetail\n\n{}1", "a".repeat(200))),
    Some("its closing line is too long and would be folded away with the detail".to_owned())
  );
}

#[test]
fn a_screen_changed_is_named_unless_the_report_claims_its_sweep() {
  let policy = policy();
  let swept = &policy.closing.words.swept;
  let paths = vec!["client/app/pages/a.vue".to_owned(), "server/a.ts".to_owned()];

  assert_eq!(unswept_screens(&paths, "fixed it", swept), vec!["client/app/pages/a.vue".to_owned()]);
  assert!(unswept_screens(&paths, "Swept it against the design rules.", swept).is_empty());
  assert!(unswept_screens(&paths, "Checked with ui-design-rules.", swept).is_empty());
}

#[test]
fn what_a_run_put_down_is_shown_short_and_quoted_past_its_own_backticks() {
  assert_eq!(answered_with("  "), "(nothing)");
  assert_eq!(answered_with("<<SKIP>>"), "`<<SKIP>>`");
  assert_eq!(answered_with("`a` b"), "`` `a` b ``");
  assert_eq!(answered_with(&"a".repeat(130)), format!("`{}…`", "a".repeat(120)));
}

#[test]
fn what_a_run_walked_away_from_is_named_by_task_and_workflow() {
  let tasks = vec![
    LiveTask {
      id: "b1".into(),
      description: "dev server".into(),
    },
    LiveTask {
      id: "b2".into(),
      description: " ".into(),
    },
  ];
  let flows = vec![
    RunWorkflow {
      call: "c1".into(),
      name: "review".into(),
      status: Some("completed".into()),
    },
    RunWorkflow {
      call: "c2".into(),
      name: "".into(),
      status: None,
    },
  ];

  assert_eq!(abandoned_workflows(&flows).len(), 1);
  assert_eq!(
    unfinished_lines(&tasks, &flows),
    vec!["Left running: dev server, b2".to_owned(), "Workflows not finished: c2".to_owned()]
  );
  assert_eq!(
    change_names(&(0..10).map(|n| n.to_string()).collect::<Vec<_>>(), 8),
    "0, 1, 2, 3, 4, 5, 6, 7 and 2 more"
  );
}

#[test]
fn a_graph_left_behind_is_named_unless_the_commits_touched_the_graph() {
  let named: HashSet<String> = ["server/a.ts".to_owned()].into();
  let touched = vec!["server/a.ts".to_owned(), "server/b.ts".to_owned()];

  assert_eq!(unrecorded_change(&touched, &named, ".graph"), vec!["server/a.ts".to_owned()]);
  assert!(
    unrecorded_change(&[touched, vec![".graph/server.json".into()]].concat(), &named, ".graph").is_empty()
  );
  // Commits touching no file any node names, and a job that committed nothing, leave nothing behind.
  assert!(unrecorded_change(&["README.md".into(), "docs/todo.md".into()], &named, ".graph").is_empty());
  assert!(unrecorded_change(&[], &named, ".graph").is_empty());
}

#[test]
fn notes_under_a_report_name_its_shape_its_unswept_screens_and_what_it_left_open() {
  let policy = policy();

  assert_eq!(shape_note("Summary\n\nDetail\n\n1 done"), "");
  assert_eq!(
    uncommitted_note(&["a.ts".into()], &["b.ts".into()]),
    "\n\nUncommitted changes are left: a.ts\n\nUncommitted changes of earlier work were kept as they stood: b.ts"
  );
  assert_eq!(
    sweep_note(&["x.vue".into()], "end", &policy.closing.words.swept),
    "\n\nThe job changed screens and its report says nothing of sweeping them against the design rules: x.vue."
  );
}

#[test]
fn a_report_line_leaving_a_check_undone_is_named_under_it_and_a_settled_one_is_not() {
  let policy = policy();
  let names = Names {
    owner: "Owner".into(),
    agent: "Agent".into(),
    sender: "Agent".into(),
  };
  let reader = Reader::new(&policy.closing, &names);
  let report =
    "Summary.\n\nChecked\n1. The screen was not checked.\n2. Read `not measured` back.\n\n1 done";

  assert_eq!(
    unchecked_note(&policy, &reader, report),
    format!("\n\n{}\n1. The screen was not checked.", policy.closing.unchecked.note)
  );
  assert_eq!(
    unchecked_note(&policy, &reader, "Summary.\n\nChecked\n1. 600 tests passed.\n\n1 done"),
    ""
  );
}

#[test]
fn an_item_left_open_is_asked_about_with_both_verbs_and_named_under_the_report_only_where_it_stayed_open() {
  let asked = left_open_item(1607, "/tree/bin/bmo", None);

  assert!(asked.contains("Your item #1607 is still open"));
  assert!(asked.contains("`/tree/bin/bmo todo --done 1607 --report '"));
  assert!(asked.contains("`/tree/bin/bmo todo --stop-here 1607 --why '"));
  assert!(!asked.contains("final turn"));

  let final_turn = left_open_item(1607, "/tree/bin/bmo", Some("`add-step`"));

  assert!(final_turn.contains("add each piece as a step with `add-step`"));
  assert!(final_turn.ends_with("in place of the answer above."));
  assert_eq!(open_item_note(None), "");
  assert!(open_item_note(Some(1607)).contains("left its item #1607 open"));
}

// A report that answers for the graph: summary, an Impact part, a check, and figures.
const ANSWERED: &str = "Summary.\n\nImpact\n1. bot/work-orders: the plan JSON gains a field, checked by job-run-plans.test.ts.\n\nChecked\n1. 12 tests passed.\n\n1 done";

fn mapped() -> Vec<String> {
  vec!["src/job_run/verdict.rs".to_owned()]
}

#[test]
fn a_report_of_a_change_that_reaches_the_graph_with_no_impact_part_is_asked_for_it() {
  let policy = policy();
  let faults = impact_faults(&policy, &mapped(), "Summary.\n\nChecked\n1. 12 tests passed.\n\n1 done", 3);
  let asked = left_impact(&policy, faults, &mapped());

  assert_eq!(
    faults,
    ImpactFaults {
      missing_part: true,
      no_call: false
    }
  );
  assert!(asked.starts_with("Your answer was not sent."), "{asked}");
  assert!(asked.contains("the report has no Impact part"), "{asked}");
  assert!(!asked.contains("no `graph_impact` call"), "{asked}");
  assert!(impact_note(&policy, faults, &mapped()).contains("the report has no Impact part"));
}

#[test]
fn a_report_with_its_impact_part_after_a_recorded_call_is_accepted_whatever_the_heading_is_dressed_in() {
  let policy = policy();

  assert!(!impact_faults(&policy, &mapped(), ANSWERED, 1).any());
  assert!(!impact_faults(&policy, &mapped(), &ANSWERED.replace("Impact", "**Impact:**"), 1).any());
  assert!(!impact_faults(&policy, &mapped(), &ANSWERED.replace("Impact", "## Impact"), 1).any());
  // A word in a sentence is not the part.
  assert!(
    impact_faults(&policy, &mapped(), &ANSWERED.replace("Impact\n", "The impact was small.\n"), 1)
      .missing_part
  );
  assert_eq!(impact_note(&policy, ImpactFaults::default(), &mapped()), "");
}

#[test]
fn a_change_that_reaches_the_graph_with_no_impact_call_recorded_is_asked_for_the_call() {
  let policy = policy();
  let faults = impact_faults(&policy, &mapped(), ANSWERED, 0);
  let asked = left_impact(&policy, faults, &mapped());

  assert_eq!(
    faults,
    ImpactFaults {
      missing_part: false,
      no_call: true
    }
  );
  assert!(
    asked.contains("no `graph_impact` call is recorded for this item, while your commits changed files the graph names: src/job_run/verdict.rs"),
    "{asked}"
  );
  assert!(
    asked.contains("Call `graph_impact` on the node each of those files belongs to"),
    "{asked}"
  );
}

#[test]
fn a_change_that_reaches_no_file_the_graph_names_is_asked_for_nothing() {
  let policy = policy();
  let named: HashSet<String> = ["src/job_run/verdict.rs".to_owned()].into();
  let touched = vec!["notes/plain.txt".to_owned()];

  assert!(graph_mapped(&touched, &named).is_empty());
  assert!(!impact_faults(&policy, &graph_mapped(&touched, &named), "Done.\n\nIt went.\n\n1 done", 0).any());
  assert_eq!(graph_mapped(&mapped(), &named), mapped());
}

#[test]
fn the_impact_tool_is_counted_by_whichever_road_reached_it_and_never_by_another_tool() {
  let policy = policy();
  let span = |kind: &str, name: &str, times: i64| TurnSpan {
    kind: kind.into(),
    name: name.into(),
    seconds: 1.0,
    times,
    tokens: None,
    input_tokens: None,
    output_tokens: None,
    cache_read_tokens: None,
    cache_write_tokens: None,
  };

  assert_eq!(
    impact_calls(
      &policy,
      &[
        span("tool", "mcp__worker__graph_impact", 2),
        span("tool", "worker/graph_impact", 1),
        span("tool", "mcp__worker__graph_get", 4),
        span("agent", "graph_impact", 9),
      ]
    ),
    3
  );
}

// ---- the parts a prompt is made of

#[test]
fn a_job_s_text_names_the_platform_s_command_line_by_its_tree() {
  assert_eq!(verbs_word(Path::new("/srv/platform")), "/srv/platform/bin/bmo");
  assert_eq!(dev_verb("bmo", "start", "w1"), "`bmo dev-server start w1`");
}

#[test]
fn an_item_is_read_as_its_requesters_line_and_a_bare_job_as_the_service_s() {
  let item = JobItem {
    title: "Fix the gate".into(),
    instruction: "Make it answer.".into(),
    requester: "owner".into(),
    requester_id: Some(5),
    impact: None,
  };

  assert_eq!(
    job_lines(Some(&item), "the task file"),
    vec!["owner (id 5): Fix the gate\nMake it answer.".to_owned()]
  );
  assert_eq!(job_lines(None, " Restart the bench. \n"), vec![instruction_line("Restart the bench.")]);
}

#[test]
fn the_knowledge_stands_after_the_text_and_before_the_trailer() {
  assert_eq!(
    assembled(Some("preamble"), None, "text", Some("reference"), "trailer"),
    "preamble\n\ntext\n\nreference\n\ntrailer"
  );
  assert_eq!(assembled(None, None, "text", None, "trailer"), "text\n\ntrailer");
}

#[test]
fn an_items_impact_grounds_stand_in_a_block_of_their_own_ahead_of_its_text() {
  let policy = policy();
  let grounds = ImpactGrounds::read(
    r#"{"nodes":[{"id":"bot/work-orders","contract":"the plan's JSON gains a field"},{"id":"lang/policy"}]}"#,
  )
  .unwrap();
  let block = impact_block(&policy, &grounds);

  assert_eq!(
    block,
    [
      "[Impact grounds — what Agent weighed this item's co-work setting on]",
      "- bot/work-orders: its outward contract changes: the plan's JSON gains a field",
      "- lang/policy: only its inside changes",
      "By these grounds the item is a single job: 1 node(s) change an outward contract.",
    ]
    .join("\n")
  );
  assert_eq!(
    assembled(Some("preamble"), Some(&block), "text", None, "trailer"),
    format!("preamble\n\n{block}\n\ntext\n\ntrailer")
  );

  let homeless = ImpactGrounds::read(r#"{"nodes":[],"new_home":true}"#).unwrap();

  assert!(impact_block(&policy, &homeless).ends_with(
    "- A new home must be decided before the work is written.\nBy these grounds the item is a co-work: 0 node(s) change an outward contract, or a new home must be decided."
  ));
}
