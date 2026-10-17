//! The queue's telling (docs/queue.md, "Telling a room" and "The sweep"): the open job rows swept — a dead
//! runner's row closed and told, a circling job and a quiet one told once, what runners left waiting taken
//! — work whose room's registration is gone told in the owner's chat, the queue's standing told, what runs
//! read from systemd joined with the open executions, and what a run came to handed back to the agent that
//! set it going. systemd's answer is a stand-in's, each test setting which units stand.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bmo_job::queue::holds;
use bmo_job::queue::instructions::queue_for_run;
use bmo_job::queue::jobs;
use bmo_job::queue::running::Running;
use bmo_job::queue::telling::{Teller, gone_report};
use job_agency::{Execution, LogLine, Run};
use job_codes::{Fact, JobFact, QueueFact, QueueTold};
use job_receiver::{Answerer, Answerers, Instruction, Said};

use crate::support::program::{linked, set_shell};
use crate::support::queue::{NOW, OWNER, Scene};

const ROOM: i64 = 9001;
const REMOVED: i64 = -808;
const MINUTE: i64 = 60_000;

// systemctl standing in a folder of its own, answering each unit named in `alive` as active and every
// other as inactive; `silent` answers nothing at all.
fn systemd(dir: &Path, alive: &[&str]) {
  let active: Vec<String> = alive.iter().map(|unit| format!("{unit}.service")).collect();
  let active = if active.is_empty() { "none-stands".to_owned() } else { active.join("|") };

  set_shell(
    dir,
    &format!(
      "for unit in \"$@\"; do case \"$unit\" in --user|is-active) ;; {active}) echo active ;; *) echo inactive ;; esac; done\n"
    ),
  );
}

fn silent(dir: &Path) {
  set_shell(dir, "exit 0\n");
}

fn teller<'a>(scene: &'a Scene, systemctl: &Path) -> Teller<'a> {
  Teller {
    db: &scene.db,
    units: &scene.units,
    policy: &scene.policy,
    owner_chat: OWNER,
    source: PathBuf::from("/source"),
    service_unit: "bmo-job".into(),
    systemctl: Some(systemctl.to_path_buf()),
    events: None,
  }
}

async fn job_in(scene: &Scene, chat: i64, unit: &str, launched_at: i64) -> i64 {
  let job = scene.job(None, "worker", unit, launched_at).await;

  scene
    .run(r#"UPDATE "jobs" SET "chat_id" = $1 WHERE "id" = $2"#, vec![chat.into(), job.into()])
    .await;
  job
}

// A job that finished done with its report while no service took its ending or its report.
async fn left_waiting(scene: &Scene, chat: i64) -> i64 {
  let ended = job_in(scene, chat, "bmo-task-ended", NOW - 20 * MINUTE).await;

  scene
    .run(
      r#"UPDATE "jobs" SET "finished_at" = $1, "outcome" = 'done', "ok" = 1, "report" = 'Done.' WHERE "id" = $2"#,
      vec![(NOW - 2 * MINUTE).into(), ended.into()],
    )
    .await;
  scene
    .run(
      r#"INSERT INTO "settings" ("key", "value") VALUES ($1, 'done'), ($2, 'gated')"#,
      vec![
        format!("job_ending_waiting:{ended}").into(),
        format!("job_report_waiting:{ended}").into(),
      ],
    )
    .await;
  ended
}

async fn setting(scene: &Scene, key: &str) -> Option<String> {
  bmo_job::queue::settings::get(&scene.db, key).await.unwrap()
}

async fn closed_row(scene: &Scene, job: i64) -> Option<(String, String, bool)> {
  bmo_job::one(
    &scene.db,
    r#"SELECT "outcome", "report", "ok" FROM "jobs" WHERE "id" = $1 AND "finished_at" IS NOT NULL"#,
    vec![job.into()],
  )
  .await
  .unwrap()
  .map(|row| {
    (
      row.try_get("", "outcome").unwrap(),
      row.try_get("", "report").unwrap(),
      row.try_get::<i64>("", "ok").unwrap() == 1,
    )
  })
}

fn facts(told: &[Instruction]) -> Vec<(Option<i64>, QueueFact)> {
  told
    .iter()
    .filter_map(|one| match &one.said {
      Said::Fact(Fact::Queue(fact)) => Some((one.chat, fact.clone())),
      _ => None,
    })
    .collect()
}

fn finished(told: &[Instruction], job: i64) -> Vec<(Option<i64>, job_codes::queue_facts::JobFinish)> {
  facts(told)
    .into_iter()
    .filter_map(|(chat, fact)| match fact {
      QueueFact::JobFinished(finish) if finish.job_id == Some(job) => Some((chat, *finish)),
      _ => None,
    })
    .collect()
}

#[tokio::test]
async fn the_sweep_closes_a_dead_runner_tells_a_circling_and_a_quiet_job_once_and_takes_what_was_left_waiting()
 {
  let mut scene = Scene::new().await;
  let dir = tempfile::tempdir().unwrap();
  let link = linked(dir.path(), "systemctl");
  // A runner gone with its row open, one somebody stopped, one circling and one standing still.
  let dead = job_in(&scene, ROOM, "bmo-task-dead", NOW - 30 * MINUTE).await;
  let stopped = job_in(&scene, ROOM, "bmo-task-stopped", NOW - 30 * MINUTE).await;
  let spinning = job_in(&scene, ROOM, "bmo-task-spin", NOW - 30 * MINUTE).await;
  let quiet = job_in(&scene, ROOM, "bmo-task-quiet", NOW - 30 * MINUTE).await;
  // A job too young to be judged dead, whose unit nobody holds yet.
  let young = job_in(&scene, ROOM, "bmo-task-young", NOW - MINUTE).await;
  let ended = left_waiting(&scene, OWNER).await;

  jobs::request_stop(&scene.db, stopped, "Enough.").await.unwrap();
  scene.units.starts = vec![("bmo-task-spin".into(), 13)];

  {
    let line = |id: i64, at: i64| LogLine {
      id,
      at,
      kind: "text".into(),
      name: None,
    };
    let mut lines = scene.units.last_lines.lock().unwrap();

    lines.push((spinning, line(1, NOW - MINUTE)));
    lines.push((quiet, line(2, NOW - 10 * MINUTE)));
  }

  systemd(dir.path(), &["bmo-task-spin", "bmo-task-quiet"]);

  let teller = teller(&scene, &link);

  assert_eq!(teller.sweep(NOW).await.unwrap(), [dead, stopped]);
  assert_eq!(closed_row(&scene, dead).await, Some(("died".into(), gone_report(), false)));
  assert_eq!(
    closed_row(&scene, stopped).await,
    Some(("stopped".into(), "The run was stopped and ended here. Reason: Enough.".into(), false))
  );
  assert_eq!(setting(&scene, &format!("job_stop:{stopped}")).await, None);
  assert_eq!(closed_row(&scene, young).await, None);

  // The room that asked hears the job is gone, and the agent is handed the break as an order, once; the
  // one somebody stopped is neither.
  let gone: Vec<(i64, i64)> = scene
    .units
    .notices()
    .iter()
    .filter_map(|(chat, notice)| match &notice.fact {
      Fact::Job(JobFact::JobGone {
        job_id,
        removed_room: None,
        ..
      }) => Some((*chat, *job_id)),
      _ => None,
    })
    .collect();

  assert_eq!(gone, [(ROOM, dead)]);

  let told = scene.units.instructions();
  let orders: Vec<&String> = told
    .iter()
    .filter_map(|one| match &one.said {
      Said::Order(text) if text.contains("A run broke") => Some(text),
      _ => None,
    })
    .collect();

  assert_eq!(orders.len(), 1, "{orders:?}");
  assert!(orders[0].contains(&format!("job {dead}")), "{}", orders[0]);

  let failure_orders: serde_json::Value =
    serde_json::from_str(&setting(&scene, "failure_orders").await.unwrap()).unwrap();

  assert_eq!(failure_orders.as_array().unwrap().len(), 1);

  // Each closed row told as that job's ending, and the one left waiting with its report.
  assert_eq!(finished(&told, dead)[0].1.outcome, "died");
  assert_eq!(finished(&told, stopped)[0].1.outcome, "stopped");
  assert_eq!(finished(&told, ended).len(), 1);
  assert_eq!(finished(&told, ended)[0].1.report.as_deref(), Some("Done."));
  assert_eq!(setting(&scene, &format!("job_ending_waiting:{ended}")).await, None);
  assert_eq!(setting(&scene, &format!("job_report_waiting:{ended}")).await, None);

  // The circling job and the quiet one told, each as its own fact.
  let circling: Vec<(i64, u64, u64)> = facts(&told)
    .into_iter()
    .filter_map(|(_, fact)| match fact {
      QueueFact::JobCircling {
        job_id,
        starts,
        most,
        ..
      } => Some((job_id, starts, most)),
      _ => None,
    })
    .collect();
  let quieted: Vec<(i64, u32, Option<String>)> = facts(&told)
    .into_iter()
    .filter_map(|(_, fact)| match fact {
      QueueFact::JobQuiet {
        job_id,
        minutes,
        last_line,
        ..
      } => Some((job_id, minutes, last_line.map(|line| line.kind))),
      _ => None,
    })
    .collect();

  assert_eq!(circling, [(spinning, 13, 12)]);
  assert_eq!(quieted, [(quiet, 10, Some("text".to_owned()))]);

  // Told once: a second pass with nothing moved tells none of it again.
  let before = (told.len(), scene.units.notices().len());

  assert!(teller.sweep(NOW + MINUTE).await.unwrap().is_empty());
  assert_eq!((scene.units.instructions().len(), scene.units.notices().len()), before);

  // A systemd that gave no answer closes nothing.
  silent(dir.path());
  assert!(teller.sweep(NOW + 10 * MINUTE).await.unwrap().is_empty());
  assert_eq!(closed_row(&scene, spinning).await, None);
}

#[tokio::test]
async fn a_job_filed_from_a_removed_room_is_told_in_the_owner_s_chat_and_nothing_reaches_that_room() {
  let mut scene = Scene::new().await;
  let dir = tempfile::tempdir().unwrap();
  let link = linked(dir.path(), "systemctl");

  scene.units.removed_rooms = vec![REMOVED];

  let dead = job_in(&scene, REMOVED, "bmo-task-gone", NOW - 30 * MINUTE).await;
  let ended = left_waiting(&scene, REMOVED).await;

  systemd(dir.path(), &[]);
  assert_eq!(teller(&scene, &link).sweep(NOW).await.unwrap(), [dead]);

  let told = scene.units.instructions();

  assert!(told.iter().all(|one| one.chat != Some(REMOVED)), "{told:?}");
  assert!(scene.units.notices().iter().all(|(chat, _)| *chat != REMOVED));

  let gone: Vec<(i64, Option<i64>)> = scene
    .units
    .notices()
    .iter()
    .filter_map(|(chat, notice)| match &notice.fact {
      Fact::Job(JobFact::JobGone {
        job_id,
        removed_room,
        ..
      }) if *job_id == dead => Some((*chat, *removed_room)),
      _ => None,
    })
    .collect();

  assert_eq!(gone, [(OWNER, Some(REMOVED))]);

  let reported = finished(&told, ended);

  assert_eq!(reported.len(), 1);
  assert_eq!(reported[0].0, Some(OWNER));
  assert_eq!(reported[0].1.removed_room, Some(REMOVED));
  assert_eq!(reported[0].1.report.as_deref(), Some("Done."));
}

#[tokio::test]
async fn the_queue_is_told_once_for_a_worker_open_with_work_waiting_and_not_while_it_is_held() {
  let scene = Scene::new().await;
  let dir = tempfile::tempdir().unwrap();
  let link = linked(dir.path(), "systemctl");
  let teller = teller(&scene, &link);

  scene.item(4, "Waiting").await;
  systemd(dir.path(), &[]);

  assert_eq!(teller.tell_queue_standing("approved", false, None, NOW).await.unwrap(), QueueTold::Told);

  let told = scene.units.instructions();
  let Said::Fact(Fact::Queue(QueueFact::QueueWaiting { moment, workers })) = &told[0].said else {
    panic!("the standing is told as a fact: {:?}", told[0].said);
  };

  assert_eq!(moment, "approved");
  assert_eq!(
    (workers[0].worker.as_str(), workers[0].waiting, workers[0].free_checkouts),
    ("worker", 1, 1)
  );
  assert_eq!(told[0].chat, Some(OWNER));
  assert_eq!(told[0].events, ["queue_changed"]);
  assert_eq!(told[0].rides_until, None);

  // The same standing is not told again unless the moment repeats it, and then it rides the turn.
  assert_eq!(
    teller.tell_queue_standing("work-order", false, None, NOW).await.unwrap(),
    QueueTold::Unchanged
  );
  assert_eq!(
    teller.tell_queue_standing("startup", true, Some(90_000), NOW).await.unwrap(),
    QueueTold::Told
  );
  assert_eq!(scene.units.instructions()[1].rides_until, Some(NOW + 90_000));

  // A worker with a job at work in its checkout is no open worker, and what runs that nobody could read
  // leaves the queue unread.
  scene.job(None, "worker", "bmo-task-x", NOW).await;
  systemd(dir.path(), &["bmo-task-x"]);
  assert_eq!(
    teller.tell_queue_standing("approved", true, None, NOW).await.unwrap(),
    QueueTold::AllBusy
  );
  silent(dir.path());
  assert_eq!(
    teller.tell_queue_standing("approved", true, None, NOW).await.unwrap(),
    QueueTold::Unreadable
  );

  // The owner's pause holds it still and writes the told standing off.
  holds::set_queue_paused(&scene.db, true).await.unwrap();
  assert_eq!(
    teller.tell_queue_standing("approved", true, None, NOW).await.unwrap(),
    QueueTold::Paused
  );
  assert_eq!(setting(&scene, "job_queue_told").await, None);
}

#[tokio::test]
async fn what_runs_joins_systemd_with_the_open_executions_save_those_a_cancel_covers() {
  let mut scene = Scene::new().await;
  let dir = tempfile::tempdir().unwrap();
  let link = linked(dir.path(), "systemctl");
  let execution = |id: &str, root: &str, cancelled: bool| Execution {
    id: id.into(),
    parent: None,
    root: Some(root.into()),
    line: Some("worker".into()),
    shared_pane: false,
    started_at: Some(1),
    ended_at: None,
    outcome: None,
    cancelled,
    details: serde_json::Map::new(),
  };

  // A run in a pane has no unit, and its open execution is what holds its name; one a cancel covers
  // holds nothing.
  scene.units.executions = vec![
    execution("exec-1", "bmo-task-pane", false),
    execution("exec-2", "bmo-task-cancelled", true),
  ];

  let running = Running {
    db: &scene.db,
    units: &scene.units,
    systemctl: Some(link),
  };
  let units: Vec<String> = ["bmo-task-unit", "bmo-task-pane", "bmo-task-cancelled", "bmo-task-gone"]
    .map(str::to_owned)
    .to_vec();
  let named = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect::<HashSet<String>>();

  systemd(dir.path(), &["bmo-task-unit"]);
  assert_eq!(
    running.answered(&units).await.unwrap(),
    Some(named(&["bmo-task-unit", "bmo-task-pane"]))
  );

  // systemd silent: nobody could say, though read as an answer of none the executions still hold their
  // names.
  silent(dir.path());
  assert_eq!(running.answered(&units).await.unwrap(), None);
  assert_eq!(running.active(&units).await.unwrap(), named(&["bmo-task-pane"]));
}

#[tokio::test]
async fn a_run_s_result_and_a_job_s_ending_go_back_to_the_companion_that_set_them_going() {
  let mut scene = Scene::new().await;
  let dir = tempfile::tempdir().unwrap();
  let answerer = |agent: &str, companion: bool| Answerer {
    agent: agent.into(),
    companion,
  };
  let run = |id: i64, chat: i64| Run {
    id,
    agent: "companion".into(),
    chat_id: Some(chat),
    unit: "unit".into(),
    depth: 1,
    started_at: 1,
    finished_at: None,
    outcome: None,
    context_tokens: None,
  };

  scene.units.rooms = vec![(
    -300,
    Answerers {
      default: "agent".into(),
      voice: "agent".into(),
      answerers: vec![answerer("agent", false), answerer("companion", true)],
    },
  )];
  scene.units.runs = vec![run(1, -300), run(2, -301)];

  let result = || Instruction {
    agent: None,
    chat: None,
    said: Said::Order("found it".into()),
    kind: None,
    sort: "run".into(),
    summary: None,
    expires_at: None,
    events: vec![],
    rides_until: None,
    via: None,
    run: None,
  };

  for (run, chat) in [(Some(1), -300), (Some(2), -301), (None, -301)] {
    queue_for_run(&scene.units, run, chat, result()).await.unwrap();
  }

  // What the companion set going in its room is its own; elsewhere, or set going by nobody, it is the room's
  // voice's.
  let agents: Vec<(Option<String>, Option<i64>, Option<i64>)> = scene
    .units
    .instructions()
    .into_iter()
    .map(|one| (one.agent, one.chat, one.run))
    .collect();

  assert_eq!(
    agents,
    [
      (Some("companion".to_owned()), Some(-300), Some(1)),
      (Some("agent".to_owned()), Some(-301), Some(2)),
      (Some("agent".to_owned()), Some(-301), None),
    ]
  );

  // An item the companion filed is its to tell of in its room, and nowhere else; the agent's own is told as
  // every item is.
  scene.item(1, "Filed by the companion").await;
  scene.item(2, "Filed by the agent").await;
  scene.run(r#"UPDATE "todos" SET "agent" = 'companion' WHERE "id" = 1"#, vec![]).await;
  scene.run(r#"UPDATE "todos" SET "agent" = 'agent' WHERE "id" = 2"#, vec![]).await;

  let teller = teller(&scene, dir.path());

  assert_eq!(teller.companion_teller(Some(1), -300).await.unwrap().as_deref(), Some("companion"));
  assert_eq!(teller.companion_teller(Some(1), -301).await.unwrap(), None);
  assert_eq!(teller.companion_teller(Some(2), -300).await.unwrap(), None);
}

#[tokio::test]
async fn a_run_s_result_for_a_removed_room_goes_to_the_owner_s_chat_naming_the_room() {
  const KEPT: i64 = -809;
  let mut scene = Scene::new().await;

  scene.units.removed_rooms = vec![REMOVED];

  let result = |text: &str| Instruction {
    agent: None,
    chat: None,
    said: Said::Order(text.into()),
    kind: Some("result".into()),
    sort: "run".into(),
    summary: None,
    expires_at: None,
    events: vec![],
    rides_until: None,
    via: Some("telegram".into()),
    run: None,
  };

  queue_for_run(&scene.units, Some(7), REMOVED, result("found it")).await.unwrap();
  queue_for_run(&scene.units, Some(8), KEPT, result("found that")).await.unwrap();

  let told = scene.units.instructions();

  assert!(told.iter().all(|one| one.chat != Some(REMOVED)));
  assert_eq!(
    (told[0].chat, told[0].run, told[0].via.as_deref(), told[0].kind.as_deref()),
    (Some(OWNER), Some(7), None, None)
  );

  let Said::Order(text) = &told[0].said else { panic!("an order: {:?}", told[0].said) };

  assert!(
    text.contains(&format!("work started in room {REMOVED}, whose registration has been removed"))
      && text.ends_with("found it"),
    "{text}"
  );
  assert_eq!(
    (told[1].chat, told[1].run, &told[1].said),
    (Some(KEPT), Some(8), &Said::Order("found that".into()))
  );
}
