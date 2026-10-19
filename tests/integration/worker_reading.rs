//! What a worker is handed of an item (docs/runs.md and docs/queue.md, "The work-order runner"): a running
//! job is pushed the agent's changes to the brief and the wind-down line and never a person's words; a step of
//! a work order reads the order's purpose and instruction and none of the item's conversation, appended words
//! or approval note; and the header over a job's turns names the item by the English bmo-gate kept for its
//! title, the runner asking the gate for the item's texts in English before the first turn.

use bmo_job::job_run::Ground;
use bmo_job::job_run::notes;
use bmo_job::job_run::plan_check::Roster;
use bmo_job::job_run::prompt::{item_heading, keep_item_english};
use bmo_job::order_run::context::work_context;
use queue_rows::todos::WIND_DOWN_NOTE_MARK;
use serde_json::{Value, json};

use crate::support::desk::Desk;
use crate::support::queue::Scene;
use crate::support::runner::Bench;

const AT: i64 = 1_700_000_000_000;

async fn item(scene: &Scene) {
  scene
    .run(
      r#"INSERT INTO "todos" ("id", "title", "instruction", "requester", "approved", "priority", "status", "created_at", "note")
         VALUES (5, 'Fix the panel', 'Conclusion

RAW_CONVERSATION_STAYS_WITH_THE_AGENT', 'owner', 1, 1, 'pending', $1, 'RAW_APPROVAL_NOTE')"#,
      vec![AT.into()],
    )
    .await;
}

async fn note(scene: &Scene, author: &str, text: &str, kind: Option<&str>) {
  scene
    .run(
      r#"INSERT INTO "todo_notes" ("todo_id", "author", "text", "added_at", "kind") VALUES (5, $1, $2, $3, $4)"#,
      vec![author.into(), text.into(), (AT + 100_000).into(), kind.map(str::to_owned).into()],
    )
    .await;
}

#[tokio::test]
async fn a_running_job_is_handed_the_agent_s_change_and_the_wind_down_line_and_never_a_person_s_words() {
  let scene = Scene::new().await;
  let wind_down = format!("{WIND_DOWN_NOTE_MARK} Stop after this step.");

  item(&scene).await;
  note(&scene, "owner", "RAW_PERSON_NOTE", None).await;
  note(&scene, "Companion", "RAW_COMPANION_NOTE", None).await;
  note(&scene, "agent", "Keep the touch input as it is.", Some("change")).await;
  note(&scene, "owner", &wind_down, None).await;

  let handed = notes::undelivered(&scene.db, 5, AT).await.unwrap();

  assert_eq!(
    handed.iter().map(|note| (note.text.as_str(), note.change)).collect::<Vec<_>>(),
    [("Keep the touch input as it is.", true), (wind_down.as_str(), false)]
  );
}

fn roster() -> Roster {
  Roster {
    tiers: vec![],
    roles: vec![],
  }
}

async fn context(bench: &Bench, order: &Value, task: &Value) -> String {
  work_context(&bench.ground(), &roster(), order, task).await.unwrap()
}

#[tokio::test]
async fn a_builder_step_reads_the_order_s_instruction_and_none_of_the_item_s_raw_words() {
  let bench = Bench::new().await;

  item(&bench.scene).await;
  note(&bench.scene, "owner", "RAW_PERSON_NOTE", None).await;

  let order = json!({
    "todoId": 5,
    "intent": "Make the panel taller",
    "instruction": "Raise the panel height and keep the touch input.",
    "phase": "synthesis",
    "stage": 0,
    "tasks": [{ "id": "synthesis", "role": "builder", "instruction": "Join the findings.", "state": "running" }],
  });
  let context = context(&bench, &order, &order["tasks"][0]).await;

  assert!(
    context.contains("Instruction: Raise the panel height and keep the touch input."),
    "{context}"
  );

  for raw in ["RAW_CONVERSATION_STAYS_WITH_THE_AGENT", "RAW_APPROVAL_NOTE", "RAW_PERSON_NOTE"] {
    assert!(!context.contains(raw), "{raw} reached the step");
  }
}

// The current task stands whole under [Current task], so [Remaining tasks] names only what runs after it.
#[tokio::test]
async fn a_builder_step_names_its_own_instruction_once() {
  let bench = Bench::new().await;

  item(&bench.scene).await;

  let order = json!({
    "todoId": 5,
    "intent": "Make the panel taller",
    "instruction": "Raise the panel height and keep the touch input.",
    "phase": "implementation",
    "stage": 0,
    "tasks": [
      { "id": "build-1", "role": "builder", "instruction": "RAISE_THE_PANEL", "state": "running" },
      { "id": "build-2", "role": "builder", "instruction": "KEEP_THE_TOUCH", "state": "pending" },
    ],
  });
  let first = context(&bench, &order, &order["tasks"][0]).await;
  let last = context(&bench, &order, &order["tasks"][1]).await;

  assert_eq!(first.matches("RAISE_THE_PANEL").count(), 1, "{first}");
  assert!(first.contains("[Remaining tasks]\nbuild-2: KEEP_THE_TOUCH\n"), "{first}");
  assert!(last.contains("[Remaining tasks]\nbuild-1: RAISE_THE_PANEL\n"), "{last}");
  assert_eq!(last.matches("KEEP_THE_TOUCH").count(), 1, "{last}");

  let alone =
    json!({ "todoId": 5, "phase": "implementation", "stage": 0, "tasks": [order["tasks"][0].clone()] });
  let only = context(&bench, &alone, &alone["tasks"][0]).await;

  assert!(only.contains("[Remaining tasks]\nnone\n"), "{only}");
}

// A word in a script a worker does not read, of so many syllables, made from its code points.
fn foreign(syllables: u32) -> String {
  (0..syllables).map(|at| char::from_u32(0xac00 + at * 29).unwrap()).collect()
}

async fn titled(desk: &Desk, id: i64, title: &str, instruction: &str) {
  desk
    .run(
      r#"INSERT INTO "todos" ("id", "title", "instruction", "requester", "approved", "priority", "status", "created_at")
         VALUES ($1, $2, $3, 'owner', 1, 1, 'pending', $4)"#,
      vec![id.into(), title.into(), instruction.into(), AT.into()],
    )
    .await;
}

// The runner's ground over the desk's stand, built as the job-run verb builds it.
macro_rules! with_ground {
  ($desk:expr, $ground:ident, $body:block) => {{
    let queue = &$desk.stand.ground;
    let units = queue.units();
    let calls = queue.run_calls();
    let $ground = Ground {
      db: &$desk.stand.db,
      units: &units,
      calls: &calls,
      policy: &$desk.stand.policy,
      reader: &$desk.stand.reader,
      glossary: &queue.banned,
      teller: queue.teller(&$desk.stand.db, &units, &$desk.stand.policy, None),
      host: &queue.host,
      source: queue.source.clone(),
      tree: queue.tree.clone(),
      documents: queue.documents.clone(),
      testbed_logs: queue.testbed_logs.clone(),
      siren: queue.siren.clone(),
      owner_chat: queue.owner_chat,
      agent_key: queue.agent_key.clone(),
      agent_name: $desk.stand.names.agent.clone(),
      owner_name: $desk.stand.names.owner.clone(),
    };

    $body
  }};
}

#[tokio::test(flavor = "multi_thread")]
async fn the_header_over_a_job_s_turns_names_the_item_by_its_kept_english_or_marks_its_title() {
  let desk = Desk::new().await;
  let (kept, bare) = (foreign(4), foreign(6));

  titled(&desk, 7, &kept, "").await;
  titled(&desk, 8, &bare, "").await;
  desk.gate.answer(
    "POST /translations/kept/read",
    200,
    json!({ "kept": [{ "original": kept, "text": "Fix the panel" }] }),
  );

  with_ground!(desk, ground, {
    assert_eq!(item_heading(&ground, 7).await.as_deref(), Some("#7 Fix the panel"));
    assert_eq!(item_heading(&ground, 8).await, Some(format!("#8 {bare} [not translated]")));
    assert_eq!(item_heading(&ground, 9).await, None);
  });

  let asked: Vec<Value> = desk.gate.calls().into_iter().filter_map(|call| call.body).collect();

  assert_eq!(asked[0], json!({ "language": "en", "originals": [kept] }));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_runner_asks_the_gate_for_the_item_s_title_and_own_sections_in_english_and_starts_whatever_it_answers()
 {
  let desk = Desk::new().await;
  let (title, sections) = (foreign(4), foreign(9));

  titled(&desk, 9, &title, &sections).await;
  desk.gate.answer_empty("POST /translations", 503);

  with_ground!(desk, ground, {
    keep_item_english(&ground, 9).await;
  });

  let asked: Vec<String> = desk
    .gate
    .calls()
    .into_iter()
    .filter(|call| call.route == "POST /translations")
    .flat_map(|call| call.body.unwrap()["originals"].as_array().unwrap().clone())
    .filter_map(|original| original.as_str().map(str::to_owned))
    .collect();

  assert!(asked.contains(&title), "{asked:?}");
  assert!(asked.contains(&sections), "{asked:?}");
}
