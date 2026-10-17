//! The queue on its rows (docs/queue.md): a question settled once and its clock held by talk, a stop that
//! closes its job as stopped, the workers and a base's support workers, whose turn it is past a stopped
//! work order, what a launch is refused, a clock's ask, the standing told to the agent and a dead runner's
//! row closed by the sweep; and the same statements on PostgreSQL.

mod support;

use std::path::{Path, PathBuf};
use std::time::Duration;

use bmo_job::queue::jobs::{self, JobEnding};
use bmo_job::queue::launches::{
  Launch, Stand, checkout_holders, launch_refusal, leases, lend, run_endings,
};
use bmo_job::queue::overtime::standing_until;
use bmo_job::queue::questions::{self, Answer, NewQuestion, Waited};
use bmo_job::queue::telling::Teller;
use bmo_job::queue::turn::{Passed, Routable, Turn, open_items, worker_queue_turn};
use bmo_job::queue::workers::{self, NewWorker, Worker};
use bmo_job::queue::{holds, settings};
use job_codes::job_questions::Lapse;
use job_codes::work_lines::PassReason;
use job_codes::{Fact, JobFact, QuestionFact, QueueFact, QueueTold};
use job_receiver::Said;
use queue_rows::todos::Item;
use sea_orm::DatabaseConnection;
use serde_json::json;
use support::queue::{NOW, OWNER, Scene, in_postgres};

fn options() -> Vec<String> {
  vec!["Keep it".into(), "Drop it".into()]
}

async fn asked(db: &DatabaseConnection, todo: i64, due_in: i64) -> questions::Question {
  questions::open(
    db,
    &NewQuestion {
      job_id: None,
      todo_id: todo,
      chat_id: OWNER,
      question: "Which way?",
      options: &options(),
      recommended: 1,
      why: "Both work.",
      asked_at: NOW,
      due_at: NOW + due_in,
      talks: true,
    },
  )
  .await
  .unwrap()
}

#[tokio::test]
async fn the_first_settlement_stands_and_the_room_is_told_what_it_came_to() {
  let scene = Scene::new().await;
  let question = asked(&scene.db, 7, 60_000).await;

  assert_eq!(
    question.asked(None),
    QuestionFact::QuestionAsked {
      item: 7,
      question_id: question.id,
      question: "Which way?".into(),
      options: options(),
      recommended: 2,
      why: "Both work.".into(),
      wait_seconds: 60,
      talk: true,
      removed_room: None,
    }
  );
  assert!(
    !questions::settle_unanswered(&scene.db, question.id, 1, NOW + 1).await.unwrap(),
    "not due yet"
  );
  assert!(questions::settle(&scene.db, question.id, &Answer::Chosen(0), NOW + 2).await.unwrap());
  assert!(!questions::settle(&scene.db, question.id, &Answer::Chosen(1), NOW + 3).await.unwrap());

  let settled = questions::question(&scene.db, question.id).await.unwrap().unwrap();

  assert_eq!(settled.answer_text(), "Keep it");
  assert_eq!(
    settled.settled(false, 1_800_000),
    QuestionFact::QuestionChosen {
      item: 7,
      option: 1,
      named: "Keep it".into(),
    }
  );
}

#[tokio::test]
async fn a_line_of_talk_holds_the_clock_and_the_asker_hears_the_owners_lines_once() {
  let scene = Scene::new().await;
  let question = asked(&scene.db, 7, 60_000).await;

  assert!(
    questions::talk(&scene.db, question.id, "owner", "Why?", 1_800_000, NOW + 10_000)
      .await
      .unwrap()
  );

  let held = questions::question(&scene.db, question.id).await.unwrap().unwrap();

  assert_eq!((held.paused_left, held.due_at), (Some(50_000), NOW + 10_000 + 1_800_000));

  let waited = questions::wait_on(&scene.db, question.id, NOW, None, Duration::from_millis(5))
    .await
    .unwrap();
  let Waited::Talk { heard, .. } = waited else { panic!("the owner talked: {waited:?}") };

  assert_eq!(heard.iter().map(|line| line.text.as_str()).collect::<Vec<_>>(), ["Why?"]);
  assert!(questions::hear_talk(&scene.db, question.id, NOW).await.unwrap().is_empty());
  assert!(questions::resume(&scene.db, question.id, NOW + 20_000).await.unwrap());
  assert!(questions::pause(&scene.db, question.id, 1_000, NOW + 20_000).await.unwrap());
}

#[tokio::test]
async fn a_wait_past_its_due_time_settles_on_the_recommendation() {
  let scene = Scene::new().await;
  let question = asked(&scene.db, 7, -1).await;
  let waited = questions::wait_on(&scene.db, question.id, 0, None, Duration::from_millis(5))
    .await
    .unwrap();
  let Waited::Settled {
    question,
    stopped,
    talked,
  } = waited
  else {
    panic!("the wait ran out: {waited:?}")
  };

  assert!(!stopped && !talked);
  assert_eq!(question.settled_by.as_deref(), Some(questions::SETTLED_BY_DEADLINE));
  assert!(matches!(
    question.settled(false, 1_800_000),
    QuestionFact::QuestionLapsed {
      lapsed: Lapse::Waiting,
      option: 2,
      ..
    }
  ));
}

#[tokio::test]
async fn a_job_a_person_stopped_closes_as_stopped_whatever_its_closer_read() {
  let scene = Scene::new().await;
  let job = scene.job(Some(7), "worker", "bmo-task-1", NOW).await;

  jobs::request_stop(&scene.db, job, "Enough.").await.unwrap();
  assert!(jobs::stop_asked(&scene.db, job).await.unwrap());
  jobs::finish(&scene.db, job, "Report.", true, "done", NOW + 5).await.unwrap();

  assert!(!jobs::stop_asked(&scene.db, job).await.unwrap());
  assert_eq!(run_endings(&scene.db, 7).await.unwrap(), [(Some(NOW + 5), Some("stopped".to_owned()))]);
  assert_eq!(
    jobs::ending(&scene.db, Some(job), "done").await.unwrap(),
    JobEnding {
      job_id: Some(job),
      todo_id: Some(7),
      worker: Some("worker".into()),
      outcome: "stopped".into(),
    }
  );
}

#[tokio::test]
async fn the_built_in_worker_stands_first_and_a_base_brings_its_support_workers_to_its_count() {
  let scene = Scene::new().await;
  let source = PathBuf::from("/source");
  let rules = &scene.policy.workers;

  assert_eq!(workers::workers(&scene.db, &source).await.unwrap(), [Worker::builtin(&source)]);

  workers::add_worker(
    &scene.db,
    &NewWorker {
      name: "infra",
      label: "Infra",
      cwd: "/srv/infra",
      preamble: None,
      dev_command: None,
    },
    NOW,
  )
  .await
  .unwrap();
  scene
    .run(r#"UPDATE "workers" SET "support_count" = 2 WHERE "name" = 'infra'"#, vec![])
    .await;

  let settled = workers::settle_support(&scene.db, &scene.units, rules, "infra", &source, |_| false, NOW)
    .await
    .unwrap();
  let first = rules.first_support_slot;

  assert_eq!(
    settled.added,
    [workers::support_name("infra", first), workers::support_name("infra", first + 1)]
  );

  let known = workers::workers(&scene.db, &source).await.unwrap();
  let names: Vec<&str> = known.iter().map(|worker| worker.name.as_str()).collect();

  assert_eq!(names[0], "worker");
  assert_eq!(names[1], "infra");
  assert_eq!(known[2].label, format!("Infra#{first}"));
  assert_eq!(known[2].cwd, format!("/srv/infra-w{first}"));

  scene
    .run(r#"UPDATE "workers" SET "support_count" = 1 WHERE "name" = 'infra'"#, vec![])
    .await;

  let busy = workers::support_name("infra", first + 1);
  let kept =
    workers::settle_support(&scene.db, &scene.units, rules, "infra", &source, |name| name == busy, NOW)
      .await
      .unwrap();

  assert!(kept.removed.is_empty(), "a busy support worker stands");

  let gone = workers::settle_support(&scene.db, &scene.units, rules, "infra", &source, |_| false, NOW)
    .await
    .unwrap();

  assert_eq!(gone.removed, [busy]);
}

struct Open;

impl Routable for Open {
  async fn routable(&self, _: &Item) -> bool {
    true
  }
}

#[tokio::test]
async fn a_stopped_order_resumes_unless_it_has_only_just_stopped_with_work_behind_it() {
  let scene = Scene::new().await;
  let source = PathBuf::from("/source");

  scene.item(1, "Stopped").await;
  scene.item(2, "Behind").await;

  let mut order = queue_read::work_order::draft(1, "purpose", "instruction");

  order["status"] = json!("paused");
  order["stoppedAt"] = json!(NOW - 1_000);
  queue_rows::work_orders::save(&scene.db, &mut order).await.unwrap();

  let items = open_items(&scene.db).await.unwrap();
  let known = workers::workers(&scene.db, &source).await.unwrap();
  let turn = worker_queue_turn(&scene.db, &items, "worker", &known, &Open, NOW).await.unwrap();

  assert_eq!(
    turn,
    Turn::Launch {
      item: 2,
      passed: vec![Passed {
        todo_id: 1,
        why: PassReason::JustStopped,
      }],
    }
  );

  scene.job(Some(2), "worker", "bmo-task-2", NOW - 500).await;

  let turn = worker_queue_turn(&scene.db, &items, "worker", &known, &Open, NOW).await.unwrap();

  assert_eq!(
    turn,
    Turn::Resume {
      item: 1,
      passed: vec![]
    }
  );
}

#[tokio::test]
async fn a_session_launches_an_approved_item_of_the_worker_asked_and_a_held_checkout_is_named() {
  let scene = Scene::new().await;
  let source = PathBuf::from("/source");

  scene.item(3, "Waiting").await;

  let items = open_items(&scene.db).await.unwrap();
  let known = workers::workers(&scene.db, &source).await.unwrap();
  let stand = |item| Stand {
    worker: &known[0],
    queue: &items,
    item,
    item_worker: Some("worker"),
    runs: &[],
    now: NOW,
  };
  let launch = |todo_id, purpose| Launch {
    started_by: Some("chat:100"),
    todo_id,
    purpose,
  };

  assert!(launch_refusal(&launch(Some(3), None), &stand(items.first())).is_ok());
  assert_eq!(
    launch_refusal(&launch(None, None), &stand(None)).unwrap_err().code,
    "session_launches_item"
  );
  assert_eq!(
    launch_refusal(&launch(Some(3), Some("why")), &stand(items.first())).unwrap_err().code,
    "item_launch_takes_no_why"
  );
  assert_eq!(
    launch_refusal(&launch(Some(9), None), &stand(None)).unwrap_err().code,
    "launch_item_absent"
  );

  let died = [(Some(NOW - 1_000), Some("died".to_owned()))];
  let waiting = Stand {
    runs: &died,
    ..stand(items.first())
  };

  assert_eq!(launch_refusal(&launch(Some(3), None), &waiting).unwrap_err().code, "launch_waits");

  let job = scene.job(Some(3), "other", "bmo-task-9", NOW).await;

  lend(&scene.db, "worker", job, NOW).await.unwrap();

  let lent = leases(&scene.db).await.unwrap();
  let open = jobs::unfinished(&scene.db).await.unwrap();
  let holders = checkout_holders(&known[0], &known, Some(&open), &lent).unwrap();

  assert_eq!(holders.len(), 1);
  assert!(holders[0].borrowed);
  assert_eq!(checkout_holders(&known[0], &known, None, &lent), None, "nobody could say");
}

#[tokio::test]
async fn the_agent_hears_of_a_worker_standing_free_with_work_waiting_once_until_it_moves() {
  let scene = Scene::new().await;

  scene.item(4, "Waiting").await;

  let teller = teller(&scene, None);

  assert_eq!(teller.tell_queue_standing("filed", false, None, NOW).await.unwrap(), QueueTold::Told);
  assert_eq!(
    teller.tell_queue_standing("filed", false, None, NOW).await.unwrap(),
    QueueTold::Unchanged
  );
  assert_eq!(teller.tell_queue_standing("filed", true, None, NOW).await.unwrap(), QueueTold::Told);

  let told = scene.units.instructions();
  let Said::Fact(Fact::Queue(QueueFact::QueueWaiting { workers, .. })) = &told[0].said else {
    panic!("the standing is told as a fact: {:?}", told[0].said)
  };

  assert_eq!((workers[0].worker.as_str(), workers[0].waiting), ("worker", 1));

  holds::set_queue_paused(&scene.db, true).await.unwrap();
  assert_eq!(teller.tell_queue_standing("filed", true, None, NOW).await.unwrap(), QueueTold::Paused);
}

fn teller<'a>(scene: &'a Scene, systemctl: Option<PathBuf>) -> Teller<'a> {
  Teller {
    db: &scene.db,
    units: &scene.units,
    policy: &scene.policy,
    owner_chat: OWNER,
    source: PathBuf::from("/source"),
    service_unit: "bmo-job".into(),
    systemctl,
    events: None,
  }
}

// A systemctl that reads every unit asked of it as inactive.
fn inactive_systemctl(dir: &Path) -> PathBuf {
  use std::os::unix::fs::PermissionsExt;

  let path = dir.join("systemctl");

  std::fs::write(
    &path,
    "#!/bin/sh\nfor unit in \"$@\"; do case \"$unit\" in --user|is-active) ;; *) echo inactive ;; esac; done\n",
  )
  .unwrap();
  std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();

  path
}

#[tokio::test]
async fn the_sweep_closes_a_dead_runners_row_tells_its_room_and_hands_the_run_to_the_agent() {
  let scene = Scene::new().await;
  let scratch = tempfile::tempdir().unwrap();

  scene.item(5, "Gone").await;

  let job = scene.job(Some(5), "worker", "bmo-task-5", NOW - 10 * 60_000).await;
  let fresh = scene.job(None, "worker", "bmo-task-6", NOW - 1_000).await;
  let teller = teller(&scene, Some(inactive_systemctl(scratch.path())));
  let closed = teller.sweep(NOW).await.unwrap();

  assert_eq!(closed, [job], "a job inside its grace is not read as dead");
  assert_eq!(run_endings(&scene.db, 5).await.unwrap(), [(Some(NOW), Some("died".to_owned()))]);
  assert!(jobs::unfinished(&scene.db).await.unwrap().iter().any(|open| open.id == fresh));

  let notices = scene.units.notices();

  assert!(matches!(
    &notices[0].1.fact,
    Fact::Job(JobFact::JobGone { job_id, .. }) if *job_id == job
  ));

  let told = scene.units.instructions();

  assert!(
    told
      .iter()
      .any(|one| matches!(&one.said, Said::Order(text) if text.contains("A run broke")))
  );
  assert!(told.iter().any(|one| matches!(&one.said, Said::Fact(Fact::Queue(QueueFact::JobFinished(finish))) if finish.job_id == Some(job))));
}

// The statements every backend must take alike: a question's talk and its clock, a clock's ask, the
// workers' order, a job's stop and a checkout's loan.
async fn every_backend_takes(scene: &Scene) {
  let question = asked(&scene.db, 7, 60_000).await;

  assert!(questions::talk(&scene.db, question.id, "job", "Because.", 1_000, NOW).await.unwrap());
  assert!(
    questions::hear_talk(&scene.db, question.id, NOW).await.unwrap().is_empty(),
    "the job's line is heard"
  );
  assert!(questions::talk(&scene.db, question.id, "owner", "Why?", 1_000, NOW).await.unwrap());
  assert_eq!(questions::hear_talk(&scene.db, question.id, NOW).await.unwrap().len(), 1);
  assert!(questions::talked(&scene.db, question.id).await.unwrap());

  scene
    .run(
      r#"INSERT INTO "clock_holds" ("run", "agent", "what", "clock", "clock_ms", "started_at", "asked_at", "answer_by", "verdict")
         VALUES ('run-1', 'agent', 'a build', 'turn', 60000, $1, $1, $2, NULL)"#,
      vec![NOW.into(), (NOW + 5).into()],
    )
    .await;

  assert_eq!(standing_until(&scene.db, "run-1").await.unwrap(), Some(NOW + 5));
  assert_eq!(standing_until(&scene.db, "run-2").await.unwrap(), None);
  assert_eq!(workers::workers(&scene.db, Path::new("/source")).await.unwrap().len(), 1);

  let job = scene.job(Some(7), "worker", "bmo-task-1", NOW).await;

  jobs::request_stop(&scene.db, job, "Enough.").await.unwrap();
  jobs::finish(&scene.db, job, "Report.", true, "done", NOW).await.unwrap();
  assert_eq!(jobs::of_unit(&scene.db, "bmo-task-1").await.unwrap(), Some((job, true)));
  assert!(leases(&scene.db).await.unwrap().is_empty());
  assert_eq!(settings::numbered(&scene.db, "job_stop:").await.unwrap(), Vec::<i64>::new());
}

#[tokio::test]
async fn a_sqlite_database_takes_the_queues_statements() {
  every_backend_takes(&Scene::new().await).await;
}

#[tokio::test]
async fn a_postgres_database_takes_the_queues_statements() {
  in_postgres("queue", |db: DatabaseConnection| async move {
    every_backend_takes(&Scene::on(db, None)).await
  })
  .await;
}
