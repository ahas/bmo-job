//! A migrated database of the test's own, on SQLite in a scratch directory or on the test PostgreSQL server
//! in a scratch schema, seeded with one small queue, and the checks every backend must pass on it.

// Each test file is its own crate and uses only part of this module.
#![allow(dead_code)]

use std::collections::HashSet;
use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

use job_agency::Provider;
use job_base::{Settings, run};
use job_codes::work_lines::JobLabel;
use job_policy::Names;
use migration::{Migrator, MigratorTrait};
use queue_read::blocks::{CONVERSATION_HEAD, SAID_INSTRUCTION, render_block};
use queue_read::lines::Facing;
use queue_read::queue::{HoldKind, Queue, RanOn};
use queue_read::{runs, work_order};
use sea_orm::DatabaseConnection;
use tempfile::TempDir;

const DEPLOYMENT: &str = r#"
[deployment]
owner_name = "Owner"
agent_name = "Agent"
agent_key = "agent"
sender_name = "Sender"
receiver_unit = "receiver"
"#;

/// The room the seeded jobs ran in.
pub const ROOM: i64 = 7;

/// The instant the seed is written at, in epoch milliseconds: 2026-09-20 11:09 UTC.
pub const AT: i64 = 1_789_902_540_000;

async fn connect(url: &str) -> DatabaseConnection {
  let settings = Settings::read(&format!("[database]\nurl = \"{url}\"\n{DEPLOYMENT}")).unwrap();

  settings.database.url.connect(2).await.unwrap_or_else(|error| panic!("{url}: {error}"))
}

/// A migrated database in a file of its own, gone with the directory.
pub async fn sqlite() -> (TempDir, DatabaseConnection) {
  let scratch = tempfile::tempdir().unwrap();
  let url = format!("sqlite://{}?mode=rwc", scratch.path().join("job.db").display());
  let db = connect(&url).await;

  Migrator::up(&db, None).await.unwrap();

  (scratch, db)
}

/// A migrated database already seeded with the queue below.
pub async fn seeded() -> (TempDir, DatabaseConnection) {
  let (scratch, db) = sqlite().await;

  seed(&db).await;

  (scratch, db)
}

/// The machine's own PostgreSQL server, as every PostgreSQL test of the service reaches it.
fn test_postgres() -> String {
  std::env::var("BMO_TEST_POSTGRES")
    .unwrap_or_else(|_| "postgres:///bmo_test?host=/var/run/postgresql".into())
}

/// Runs `check` on a migrated database in a scratch schema of its own on the test PostgreSQL server, and
/// drops the schema whatever the check came to.
pub async fn in_postgres<F, Fut>(name: &str, check: F)
where
  F: FnOnce(DatabaseConnection) -> Fut + Send + 'static,
  Fut: Future<Output = ()> + Send + 'static,
{
  let admin = connect(&test_postgres()).await;
  let seconds = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs();
  let schema = format!("t_{seconds}_{}_{name}", std::process::id());

  run(&admin, &format!("CREATE SCHEMA \"{schema}\""), vec![]).await.unwrap();

  let separator = if test_postgres().contains('?') { '&' } else { '?' };
  let db = connect(&format!("{}{separator}options[search_path]={schema}", test_postgres())).await;
  let checked = tokio::spawn(async move {
    Migrator::up(&db, None).await.unwrap();
    check(db).await;
  })
  .await;

  run(&admin, &format!("DROP SCHEMA \"{schema}\" CASCADE"), vec![]).await.unwrap();

  if let Err(failure) = checked {
    std::panic::resume_unwind(failure.into_panic());
  }
}

/// The instruction item 1 was filed with: the conversation it came from and what is to be done.
pub fn first_instruction() -> String {
  [
    CONVERSATION_HEAD.to_owned(),
    render_block(SAID_INSTRUCTION, "Owner", "The launcher is broken."),
    render_block("What to do", "Agent", "Fix the launcher."),
  ]
  .join("\n\n")
}

/// The queue every database test reads:
///
/// - item 1, open, named for a support worker, with an unfinished note, a draft work order of its own
///   words, two runs, notes, questions and a row raised from its unfinished note;
/// - item 2, merging into item 1;
/// - item 3, done;
/// - item 4, a co-work item paused while its run still goes, its road read off its work order;
/// - item 5, awaiting approval and repairing item 1;
/// - item 6, raised from item 1's unfinished note.
pub async fn seed(db: &DatabaseConnection) {
  let instruction = first_instruction().replace('\'', "''");
  let statements = [
    format!(
      "INSERT INTO todos (id, title, instruction, requester, approved, done, priority, status, model, worker, \
       cowork, unfinished, report, created_at) VALUES (1, 'Fix the launcher', '{instruction}', 'Owner', 1, 0, \
       2, 'pending', 'claude-code::claude-opus-5-5', 'web-2', 0, 'The docs are left.', 'Fixed most of it.', {AT})"
    ),
    format!(
      "INSERT INTO todos (id, title, instruction, requester, approved, done, priority, status, merged_into, \
       merged_at, created_at) VALUES (2, 'Tidy the launcher logs', 'Tidy them.', 'Owner', 1, 0, 3, 'pending', 1, \
       {AT}, {AT})"
    ),
    format!(
      "INSERT INTO todos (id, title, instruction, requester, approved, done, priority, status, created_at) \
       VALUES (3, 'Old work', 'Done long ago.', 'Owner', 1, 1, 1, 'done', {AT})"
    ),
    format!(
      "INSERT INTO todos (id, title, instruction, requester, approved, done, priority, status, cowork, held_at, \
       held_reason, held_kind, created_at) VALUES (4, 'Split the reader', 'Split it.', 'Owner', 1, 0, 4, \
       'pending', 1, {AT}, 'waiting for review', 'pause', {AT})"
    ),
    format!(
      "INSERT INTO todos (id, title, instruction, requester, approved, done, priority, status, repairs_todo_id, \
       created_at) VALUES (5, 'Repair the launcher fix', 'Repair it.', 'Guest', 0, 0, 5, 'pending', 1, {AT})"
    ),
    format!(
      "INSERT INTO todos (id, title, instruction, requester, approved, done, priority, status, created_at) \
       VALUES (6, 'Write the launcher docs', 'Write them.', 'Owner', 1, 0, 6, 'pending', {AT})"
    ),
    "INSERT INTO todo_sources (todo_id, source_todo_id) VALUES (6, 1)".to_owned(),
    format!("INSERT INTO workers (name, created_at) VALUES ('web', {AT})"),
    format!("INSERT INTO workers (name, support_of, created_at) VALUES ('web-2', 'web', {AT})"),
    format!("INSERT INTO workers (name, resting_since, created_at) VALUES ('old', {AT}, {AT})"),
    format!(
      "INSERT INTO jobs (id, chat_id, unit, launched_at, prompt, finished_at, report, ok, todo_id, outcome, \
       execution_mode, execution_model) VALUES (1, {ROOM}, 'job-1', {AT}, 'First try', {}, 'It failed.', 0, 1, \
       'failed', 'registered', 'flash')",
      AT + 60_000
    ),
    format!(
      "INSERT INTO jobs (id, chat_id, unit, launched_at, prompt, finished_at, report, ok, todo_id, outcome, \
       execution_mode, execution_model, prompt_english, prompt_korean) VALUES (2, {ROOM}, 'job-2', {}, \
       'Second try', {}, 'It worked.', 1, 1, 'done', 'claude-code', 'claude-opus-5-5', 'Fix the launcher, \
       again.', 'The kept reading')",
      AT + 120_000,
      AT + 300_000
    ),
    format!(
      "INSERT INTO jobs (id, chat_id, unit, launched_at, prompt, purpose) VALUES (3, {ROOM}, 'job-3', {}, \
       'Look around', '  a check ')",
      AT + 400_000
    ),
    format!(
      "INSERT INTO jobs (id, chat_id, unit, launched_at, prompt, todo_id) VALUES (4, 8, 'job-4', {AT}, \
       'Split', 4)"
    ),
    "INSERT INTO job_spans (job_id, todo_id, kind, name, seconds, times) VALUES (1, 1, 'phase', 'agent run', 50.0, 1)"
      .to_owned(),
    "INSERT INTO job_spans (job_id, todo_id, kind, name, seconds, times) VALUES (2, 1, 'phase', 'agent run', 170.0, 1)"
      .to_owned(),
    "INSERT INTO job_spans (job_id, todo_id, kind, name, seconds, times) VALUES (2, 1, 'tool', 'Bash', 20.5, 4)"
      .to_owned(),
    "INSERT INTO job_spans (job_id, todo_id, kind, name, seconds, times, tokens, input_tokens, output_tokens) \
     VALUES (2, 1, 'usage', 'claude-opus-5-5', 0.0, 1, 900, 600, 300)"
      .to_owned(),
    format!(
      "INSERT INTO todo_notes (id, todo_id, author, text, added_at, job_id) VALUES (1, 1, 'Job 2', \
       'The launcher reads its unit now.', {AT}, 2)"
    ),
    format!(
      "INSERT INTO todo_notes (id, todo_id, author, text, added_at) VALUES (2, 1, 'Owner', 'Check the docs too.', \
       {AT})"
    ),
    format!(
      "INSERT INTO todo_notes (id, todo_id, author, text, added_at, delivered_at, kind) VALUES (3, 1, 'Agent', \
       'Narrowed to the launcher.', {AT}, {AT}, 'change')"
    ),
    format!(
      "INSERT INTO todo_notes (id, todo_id, author, text, added_at, removed_at) VALUES (4, 1, 'Owner', 'Taken \
       back.', {AT}, {AT})"
    ),
    format!(
      "INSERT INTO todo_notes (id, todo_id, author, text, added_at) VALUES (5, 2, 'Owner', 'Keep the old \
       logs.', {AT})"
    ),
    format!(
      "INSERT INTO job_questions (job_id, todo_id, chat_id, question, options, recommended, why, asked_at, \
       due_at, chosen, settled_at, settled_by) VALUES (2, 1, {ROOM}, 'Which unit?', 'the old one\nthe new one', \
       0, 'Both work.', {AT}, {AT}, 1, {AT}, 'owner')"
    ),
    format!(
      "INSERT INTO job_questions (job_id, todo_id, chat_id, question, options, recommended, why, asked_at, \
       due_at) VALUES (2, 1, {ROOM}, 'Restart it?', 'yes\nno', 0, 'It is safe.', {AT}, {AT})"
    ),
    "INSERT INTO work_orders (todo_id, body) VALUES (1, '{\"status\":\"draft\",\"intent\":\"A purpose of its \
     own\",\"instruction\":\"A plan of its own\"}')"
      .to_owned(),
    "INSERT INTO work_orders (todo_id, body) VALUES (4, '{\"status\":\"active\",\"intent\":\"Split\",\
     \"instruction\":\"Split it\",\"ranOn\":{\"provider\":\"registered\",\"model\":\"flash\",\"changed\":true}}')"
      .to_owned(),
  ];

  for statement in &statements {
    run(db, statement, vec![]).await.unwrap_or_else(|error| panic!("{statement}: {error}"));
  }
}

/// The providers the seeded items are read against: the head is the first that is on.
pub fn providers() -> Vec<Provider> {
  [("off", false), ("claude-code", true), ("registered", true)]
    .into_iter()
    .map(|(name, enabled)| Provider {
      name: name.to_owned(),
      enabled,
      registered: false,
      lines: vec![],
    })
    .collect()
}

/// What the seeded queue is read against.
pub fn facing(queue: &Queue) -> Facing {
  Facing {
    workers: queue.workers().to_vec(),
    stopping: queue.hold_pending_ids(queue.all_todos()),
    providers: providers(),
  }
}

/// The names the deployment gives.
pub fn names() -> Names {
  Names {
    owner: "Owner".to_owned(),
    agent: "Agent".to_owned(),
    sender: "Sender".to_owned(),
  }
}

/// The seeded queue opened, with item 1's records read.
pub async fn opened(db: &DatabaseConnection) -> Queue {
  let mut queue = Queue::open(db).await.unwrap();

  queue.read_item(db, 1).await.unwrap();
  queue
}

/// What every backend must read off the seeded queue.
pub async fn check(db: DatabaseConnection) {
  seed(&db).await;

  let queue = opened(&db).await;
  let order: Vec<i64> = queue.all_todos().iter().map(|item| item.id).collect();

  // What is open first, each part by priority.
  assert_eq!(order, vec![1, 2, 4, 5, 6, 3]);

  let first = queue.get_todo(1).unwrap();

  assert_eq!(first.title, "Fix the launcher");
  assert!(first.approved && !first.done && !first.cowork);
  assert_eq!(first.priority, 2);
  assert_eq!(first.instruction, first_instruction());
  assert_eq!(
    first.ran_on,
    Some(RanOn {
      provider: "claude-code".to_owned(),
      model: "claude-opus-5-5".to_owned(),
      changed: false
    })
  );

  let held = queue.get_todo(4).unwrap();

  assert_eq!(held.held_kind, Some(HoldKind::Pause));
  assert_eq!(
    held.ran_on.as_ref().map(|road| (road.provider.as_str(), road.changed)),
    Some(("registered", true))
  );
  assert!(queue.get_todo(2).unwrap().merged());
  assert_eq!(queue.find_todo_by_title("Old work"), None);

  let workers: Vec<&str> = queue.workers().iter().map(|worker| worker.name.as_str()).collect();

  assert_eq!(workers.iter().copied().collect::<HashSet<_>>(), HashSet::from(["web", "web-2"]));
  assert_eq!(queue.hold_pending_ids(queue.all_todos()), HashSet::from([4]));
  assert_eq!(queue.todos_merged_into(1).iter().map(|row| row.id).collect::<Vec<_>>(), vec![2]);
  assert_eq!(queue.todos_raised_from(1).iter().map(|row| row.id).collect::<Vec<_>>(), vec![6]);
  assert_eq!(queue.todo_repairs(1).iter().map(|row| row.id).collect::<Vec<_>>(), vec![5]);

  // The item's own records.
  let notes: Vec<(i64, bool)> =
    queue.todo_notes(1).iter().map(|note| (note.id, note.with_items_job)).collect();

  assert_eq!(notes, vec![(1, true), (2, false), (3, false)]);
  assert_eq!(queue.todo_notes(2).len(), 1);
  assert_eq!(queue.work_order(1).map(|order| order.status.as_str()), Some("draft"));
  assert_eq!(queue.todo_questions(1).len(), 2);
  assert_eq!(queue.todo_questions(1)[0].options, vec!["the old one", "the new one"]);
  assert_eq!(queue.todo_questions(1)[0].settled_by.as_deref(), Some("owner"));

  let runs = queue.item_runs(1);

  assert_eq!(runs.iter().map(|(run, _)| run.id).collect::<Vec<_>>(), vec![1, 2]);
  assert_eq!(
    runs[1].1.iter().map(|span| span.name.as_str()).collect::<Vec<_>>(),
    vec!["agent run", "Bash", "claude-opus-5-5"]
  );
  assert_eq!(runs[1].1[1].seconds, 20.5);
  assert_eq!(runs[1].1[2].tokens, Some(900));

  // A room's jobs, newest first.
  let jobs = runs::jobs(&db, ROOM, None).await.unwrap();

  assert_eq!(
    jobs.iter().map(|job| job.label.clone()).collect::<Vec<_>>(),
    vec![
      JobLabel::Itemless,
      JobLabel::Item {
        id: 1,
        attempt: Some(2)
      },
      JobLabel::Item {
        id: 1,
        attempt: None
      }
    ]
  );
  assert_eq!(jobs[0].purpose.as_deref(), Some("a check"));
  assert_eq!(jobs[0].ok, None);
  assert_eq!(jobs[1].prompt, "The kept reading");
  assert_eq!(jobs[1].english.as_deref(), Some("Fix the launcher, again."));
  assert_eq!(jobs[1].ok, Some(true));
  assert_eq!(jobs[2].prompt, "First try");
  assert_eq!(jobs[2].english, None);
  assert_eq!(runs::jobs(&db, ROOM, Some(1)).await.unwrap().len(), 1);

  // An item's work order: the one stored, or the draft its own words would start.
  let stored = work_order::read(&db, 4).await.unwrap().unwrap();
  let drafted = work_order::read(&db, 3).await.unwrap().unwrap();

  assert_eq!(stored["status"], "active");
  assert_eq!(
    (drafted["status"].as_str(), drafted["intent"].as_str()),
    (Some("draft"), Some("Old work"))
  );
  assert_eq!(drafted["instruction"], "Done long ago.");
  assert!(work_order::is_order(&drafted));
  assert_eq!(work_order::read(&db, 99).await.unwrap(), None);
}
