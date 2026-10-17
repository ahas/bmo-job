//! The queue's own readings that need no database (docs/queue.md): a worker's path and what a worker may
//! be, an ending counted against its item, how far a work order has come, a selection read against the
//! providers, the runners and their depths, a job's unit and its checkout's lock, a voiced notice, a
//! report's outward summary, a broken run's signature and journal window, a wait mark and a word's files.

mod support;

use std::path::PathBuf;

use bmo_job::queue::job_start::{Host, bus_refusal, bus_unreachable, lock_file_name, runner_command};
use bmo_job::queue::outcomes::{
  JOB_DIED, JOB_DONE, JOB_ERROR, JOB_FAILED, JOB_UNCLOSED, ending, execution_failure_streak, tried_the_work,
};
use bmo_job::queue::overtime::spoken;
use bmo_job::queue::paths::resolve;
use bmo_job::queue::progress::{BuildCount, WorkProgress, work_progress};
use bmo_job::queue::routes::{Choice, Roads};
use bmo_job::queue::runners::{Runner, command, runners};
use bmo_job::queue::telling::failure::{job_signature, journal_window};
use bmo_job::queue::telling::{JobWait, outward_summary};
use bmo_job::queue::voiced::VoicedNotice;
use bmo_job::queue::word_files::{WordFile, directory, refusal, save, with_files};
use bmo_job::queue::worker_rules::{Adding, Beyond, Judge};
use bmo_job::queue::workers::{Changes, Worker};
use job_codes::job_ended::JobEnding;
use job_codes::job_wait::WaitTarget;
use job_codes::queue_facts::WaitStanding;
use job_codes::work_lines::WorkStep;
use job_codes::{Fact, JobFact};
use serde_json::json;
use support::queue::{Fake, policy};

#[test]
fn a_path_is_worked_out_on_its_own_words() {
  assert_eq!(resolve("/srv/infra/"), PathBuf::from("/srv/infra"));
  assert_eq!(resolve("/srv/./infra/../repo"), PathBuf::from("/srv/repo"));
  assert_eq!(resolve("/.."), PathBuf::from("/"));
}

#[test]
fn a_failed_answer_counts_against_the_item_until_the_item_was_closed_before_it_ended() {
  assert!(tried_the_work(Some(JOB_FAILED), None, Some(10)));
  assert!(tried_the_work(Some(JOB_UNCLOSED), Some(20), Some(10)));
  assert!(!tried_the_work(Some(JOB_UNCLOSED), Some(10), Some(20)));
  assert!(!tried_the_work(Some(JOB_ERROR), None, Some(10)));
  assert_eq!(ending("live-tasks"), Some(JobEnding::LiveTasks));
  assert_eq!(ending("something else"), None);
}

#[test]
fn the_streak_counts_back_from_the_last_ended_run_and_a_run_going_on_breaks_nothing() {
  let runs = vec![
    (Some(1), Some(JOB_DONE.to_owned())),
    (Some(2), Some(JOB_ERROR.to_owned())),
    (Some(3), Some(JOB_DIED.to_owned())),
    (None, None),
  ];

  assert_eq!(execution_failure_streak(&runs), 2);
  assert_eq!(execution_failure_streak(&runs[..1]), 0);
}

#[test]
fn a_step_stands_as_its_name_and_only_a_build_step_is_counted() {
  let research = json!({
    "status": "active", "phase": "research", "stage": 1, "round": 2,
    "stages": [{ "kind": "investigate" }, { "kind": "review" }, {}],
  });
  let unnamed = json!({ "status": "active", "phase": "research", "stage": 2, "stages": [{}, {}, {}] });
  let building = json!({ "status": "active", "phase": "implementation", "tasks": [{ "state": "done" }, { "state": "running" }, { "state": "pending" }] });
  let step = |step| WorkProgress {
    round: None,
    step,
    build: None,
  };

  assert_eq!(
    work_progress(&research),
    Some(WorkProgress {
      round: Some(2),
      ..step(WorkStep::Review)
    })
  );
  assert_eq!(work_progress(&unnamed), Some(step(WorkStep::Investigate)));
  assert_eq!(
    work_progress(&building),
    Some(WorkProgress {
      build: Some(BuildCount { at: 2, of: 3 }),
      ..step(WorkStep::Build)
    })
  );
  assert_eq!(
    work_progress(&json!({ "status": "active", "phase": "planning" })),
    Some(step(WorkStep::Plan))
  );
  assert_eq!(
    work_progress(&json!({ "status": "paused", "phase": "synthesis" })),
    Some(step(WorkStep::Synthesis))
  );
  assert_eq!(
    work_progress(&json!({ "status": "active", "phase": "finalization" })),
    Some(step(WorkStep::Final))
  );
  assert_eq!(work_progress(&json!({ "status": "draft", "phase": "planning" })), None);
}

fn worker(name: &str) -> Worker {
  Worker {
    name: name.into(),
    builtin: false,
    ..Worker::builtin(&PathBuf::from("/tmp"))
  }
}

#[test]
fn a_name_is_the_small_alphabet_and_nobody_elses() {
  let policy = policy();
  let judge = Judge::new(&policy.workers).unwrap();

  assert!(judge.name_fault("infra", &[]).is_ok());
  assert_eq!(judge.name_fault("INFRA", &[]).unwrap_err().code, "worker_name_form");
  assert_eq!(judge.name_fault("search", &["search".into()]).unwrap_err().code, "agent_name_taken");
}

#[test]
fn a_worker_is_judged_on_its_label_its_path_and_its_bench_before_it_is_written() {
  let policy = policy();
  let judge = Judge::new(&policy.workers).unwrap();
  let scratch = tempfile::tempdir().unwrap();
  let path = scratch.path().to_string_lossy().into_owned();
  let adding = |label, cwd, made| Adding {
    name: "infra",
    label,
    cwd,
    made,
    preamble: None,
    dev_command: Some("bun dev"),
  };

  assert!(judge.new_worker_fault(&adding("Infra", &path, false), &[]).is_ok());
  assert_eq!(judge.new_worker_fault(&adding(" ", &path, false), &[]).unwrap_err().code, "label_empty");
  assert_eq!(
    judge.new_worker_fault(&adding("Infra", "relative", false), &[]).unwrap_err().code,
    "path_not_absolute"
  );
  assert_eq!(
    judge.new_worker_fault(&adding("Infra", "/no/such/dir", false), &[]).unwrap_err().code,
    "path_absent"
  );
  assert!(judge.new_worker_fault(&adding("Infra", "/no/such/dir", true), &[]).is_ok());

  let two_lines = Adding {
    dev_command: Some("one\ntwo"),
    ..adding("Infra", &path, false)
  };

  assert_eq!(judge.new_worker_fault(&two_lines, &[]).unwrap_err().code, "dev_command_lines");
}

#[test]
fn a_support_worker_changes_with_its_base_and_the_built_in_stays_where_it_is() {
  let policy = policy();
  let judge = Judge::new(&policy.workers).unwrap();
  let support = Worker {
    support_of: Some("infra".into()),
    support_slot: Some(2),
    ..worker("infra-w2")
  };
  let label = Changes {
    label: Some("New".into()),
    ..Changes::default()
  };
  let moved = Changes {
    cwd: Some("/elsewhere".into()),
    ..Changes::default()
  };

  assert_eq!(
    judge.change_fault(&support, &label, Beyond::default()).unwrap_err().code,
    "support_changed_by_base"
  );
  assert_eq!(
    judge
      .change_fault(&support, &Changes::default(), Beyond { runtime: true })
      .unwrap_err()
      .code,
    "support_changed_by_base"
  );
  assert!(judge.change_fault(&support, &Changes::default(), Beyond::default()).is_ok());
  assert_eq!(
    judge
      .change_fault(&Worker::builtin(&PathBuf::from("/tmp")), &moved, Beyond::default())
      .unwrap_err()
      .code,
    "builtin_path_fixed"
  );
  assert_eq!(judge.support_fault(&worker("infra"), 1.5, 0.0).unwrap_err().code, "support_count_range");
  assert_eq!(
    judge.support_fault(&worker("infra"), 1.0, 2.0).unwrap_err().code,
    "keepalive_over_count"
  );
  assert!(judge.support_fault(&worker("infra"), 2.0, 1.0).is_ok());
}

#[tokio::test]
async fn a_selection_names_a_provider_bmo_agency_lists_and_a_model() {
  let fake = Fake::default();
  let db = sea_orm::DatabaseConnection::Disconnected;
  let roads = Roads::read(&db, &fake, &policy()).await.unwrap();

  assert_eq!(
    roads.choice("codex::gpt-5.5"),
    Some(Choice {
      provider: "codex",
      model: "gpt-5.5"
    })
  );
  assert!(roads.choice("nobody::model").is_none());
  assert!(roads.choice("codex::").is_none());
  assert!(roads.is_selection("claude-opus-4-1"));
  assert!(roads.is_selection("auto"));
  assert!(!roads.is_selection("opus"));
  assert_eq!(
    roads.resolve_job_model(Some("claude-opus-4-1"), None).await.unwrap(),
    "claude-code::claude-opus-4-1"
  );
  assert_eq!(roads.resolve_job_model(Some("opus"), None).await.unwrap_err().code, "job_model_wrong");
}

#[tokio::test]
async fn a_worker_walks_every_provider_that_carries_its_line_unless_it_is_pinned() {
  let fake = Fake::default();
  let db = sea_orm::DatabaseConnection::Disconnected;

  fake.pin("pinned", "codex", None);

  let roads = Roads::read(&db, &fake, &policy()).await.unwrap();

  assert_eq!(
    roads.worker_selections(None, Some("worker")).await.unwrap(),
    ["claude-code::claude-code-frontier", "codex::codex-frontier"]
  );
  assert_eq!(
    roads.worker_selections(Some("Reliable"), Some("pinned")).await.unwrap(),
    ["codex::codex-reliable"]
  );
}

#[test]
fn the_runners_and_a_searchs_depths_are_the_research_keys() {
  let policy = policy();
  let found = runners(&policy.research);
  let search = found.iter().find(|runner| runner.name == "search").expect("a search runner");

  assert!(!search.depths.is_empty());
  assert!(found.iter().any(|runner| *runner
    == Runner {
      name: "browse".into(),
      depths: vec![]
    }));

  let default = search.depths[0].clone();

  assert_eq!(
    command(&policy.research, "search", None, 42).unwrap(),
    ["--chat", "42", "--depth", &default, "--demand", "explicit", "--by", "console"]
  );
  assert_eq!(
    command(&policy.research, "browse", Some("deep"), 42).unwrap(),
    ["--chat", "42", "--by", "console"]
  );
  assert_eq!(
    command(&policy.research, "search", Some("very-deep"), 42).unwrap_err().code,
    "runner_depth_wrong"
  );
  assert_eq!(command(&policy.research, "digest", None, 42).unwrap_err().code, "runner_unknown");
}

#[test]
fn a_runtime_directory_with_no_manager_in_it_is_refused() {
  let dir = tempfile::tempdir().unwrap();
  let runtime = dir.path().to_string_lossy().into_owned();
  let env = std::collections::BTreeMap::from([("XDG_RUNTIME_DIR".to_owned(), runtime)]);

  assert!(bus_refusal(&env).is_some());
  std::fs::write(dir.path().join("bus"), "").unwrap();
  assert_eq!(bus_refusal(&env), None);
  assert!(bus_refusal(&std::collections::BTreeMap::new()).is_some());
}

#[test]
fn only_a_bus_systemd_could_not_reach_reads_as_unreachable() {
  assert!(bus_unreachable("Failed to connect to bus: No medium found"));
  assert!(bus_unreachable("Failed to connect to user scope bus via local transport"));
  assert!(!bus_unreachable("Unit bmo-task-1.service already exists."));
}

#[test]
fn a_jobs_unit_runs_the_runner_verb_with_the_hosts_variables_and_its_users() {
  let host = Host {
    program: PathBuf::from("/opt/bin/bmo-job"),
    cwd: PathBuf::from("/opt"),
    setenv: vec![("PATH".into(), "/usr/bin".into())],
    environment_file: Some(PathBuf::from("/etc/job.env")),
    locks: PathBuf::from("/tmp/locks"),
    prompts: PathBuf::from("/tmp/prompts"),
    env: std::collections::BTreeMap::new(),
    systemd_run: PathBuf::from("systemd-run"),
    lock_wait: std::time::Duration::from_secs(1),
  };
  let args = runner_command(&host, "bmo-task-1", Some("user-7"), vec!["1".into()]);

  assert_eq!(&args[..4], ["--user", "--quiet", "--collect", "--unit=bmo-task-1"]);
  assert!(args.contains(&"--setenv=PATH=/usr/bin".to_owned()));
  assert!(args.contains(&"--setenv=BMO_TURN_USER=user-7".to_owned()));
  assert!(args.contains(&"--property=EnvironmentFile=-/etc/job.env".to_owned()));
  assert_eq!(&args[args.len() - 4..], ["/opt/bin/bmo-job", "job-run", "--", "1"]);
}

#[test]
fn a_checkouts_lock_is_named_by_its_path_and_its_digest() {
  let name = lock_file_name("/srv/repos/infra");

  assert!(name.starts_with("srv-repos-infra-"), "{name}");
  assert!(name.ends_with(".lock"));
  assert_ne!(name, lock_file_name("/srv/repos/infra2"));
  assert!(lock_file_name("/").starts_with("root-"));
  assert!(lock_file_name(&format!("/{}", "a".repeat(200))).len() < 100);
}

#[test]
fn a_duration_reads_in_whole_minutes_where_it_can() {
  assert_eq!(spoken(60_000), "1 minute");
  assert_eq!(spoken(600_000), "10 minutes");
  assert_eq!(spoken(45_000), "45 seconds");
  assert_eq!(spoken(90_000), "1 minutes 30 seconds");
}

#[test]
fn a_notice_counts_its_times_from_the_moment_it_is_voiced_and_says_what_follows_only_where_something_does()
{
  let fact = Fact::Job(JobFact::WorkReported {
    item: Some(3),
    done: true,
    summary: None,
  });
  let notice = VoicedNotice {
    follow_up: Some("  ".into()),
    ttl_ms: Some(1_000),
    ..VoicedNotice::of(fact.clone(), " tell the chat. ")
  }
  .notice(10);

  assert_eq!(notice.fact, fact);
  assert_eq!(notice.intent, "tell the chat.");
  assert_eq!(notice.follow_up, None);
  assert_eq!(notice.expires_at, Some(1_010));
  assert_eq!(notice.rides_until, None);
}

#[test]
fn the_summary_keeps_only_sentences_about_the_result() {
  let report =
    "Login works again. Fixed in `auth.ts` with 3 tests. The Basic worker did it.\n\nFixed\n1. x";

  assert_eq!(outward_summary(report, &["Basic".into()]), "Login works again.");
  assert_eq!(outward_summary("Changed 4 files. The page is faster.", &[]), "The page is faster.");
}

#[test]
fn the_journal_reads_utc_two_minutes_either_side() {
  assert_eq!(
    journal_window("bmo-job", 10 * 60_000),
    "journalctl --user -u bmo-job --since \"1970-01-01 00:08:00 UTC\" --until \"1970-01-01 00:12:00 UTC\" --no-pager"
  );
}

#[test]
fn a_signature_takes_out_what_differs_between_two_runs_of_one_fault() {
  assert_eq!(
    job_signature("error", Some("\n  run 1234 failed at deadbeef99  \nmore")),
    "job:error:run # failed at #"
  );
  assert_eq!(job_signature("died", None), "job:died:");
}

#[test]
fn a_mark_reads_only_with_a_target_it_can_ask() {
  let read = JobWait::read(
    r#"{"what":"build","target":{"kind":"unit","name":"bmo-build"},"since":10,"until":20,"ended_at":null}"#,
  )
  .unwrap();

  assert_eq!(
    read.target,
    WaitTarget::Unit {
      name: "bmo-build".into()
    }
  );
  assert_eq!(read.standing(15), WaitStanding::Waiting);
  assert_eq!(read.standing(20), WaitStanding::Overdue);
  assert!(
    JobWait::read(r#"{"what":"x","target":{"kind":"pid","pid":0},"since":1,"until":2,"ended_at":null}"#)
      .is_none()
  );
  assert!(JobWait::read("not json").is_none());
}

#[tokio::test]
async fn a_words_files_are_written_under_its_own_directory_each_by_a_name_that_stays_inside_it() {
  let policy = policy();
  let bench = tempfile::tempdir().unwrap();
  let files = [
    WordFile {
      name: "../../etc/screen.png",
      bytes: b"one",
    },
    WordFile {
      name: "C:\\Users\\tester\\screen.png",
      bytes: b"two",
    },
    WordFile {
      name: ".env",
      bytes: b"three",
    },
    WordFile {
      name: "a\u{7}b.log",
      bytes: b"four",
    },
  ];
  let place = directory(bench.path(), 0);

  assert!(place.starts_with(bench.path().join("uploads")));
  assert!(refusal(&files, &policy.queue).is_ok());

  let saved = save(&place, &files).await.unwrap();
  let names: Vec<String> = saved
    .iter()
    .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
    .collect();

  assert_eq!(names, ["screen.png", "screen-2.png", "file-3.env", "ab.log"]);
  assert!(saved.iter().all(|path| path.starts_with(&place)));
  assert_eq!(std::fs::read(&saved[1]).unwrap(), b"two");
  assert!(
    with_files("Look.", &saved[..1]).starts_with("Look.\n\nFiles the owner attached to this request:\n- ")
  );
  assert_eq!(
    refusal(
      &[WordFile {
        name: "empty",
        bytes: b""
      }],
      &policy.queue
    )
    .unwrap_err()
    .code,
    "file_empty"
  );
}
