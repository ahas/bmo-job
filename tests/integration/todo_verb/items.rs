//! Words added to an item that stands and taken back again, the brief the agent rewrites, the whole queue's
//! auto-progression paused and resumed, and an item written again while it is a draft (docs/rows.md; docs/verbs.md,
//! "The TODO's edits and holds"), each read back through the server's readings.

use serde_json::{Value, json};

use crate::support::todo::{AGENT, As, Bench, OWN_WORDS, OWNER_SENDER, Ran};

const INSTRUCTION: &str = "Conclusion\n\nWhat the item asks for.";
const UNIT: &str = "bmo-task-items";

// An item already standing, approved.
async fn standing_item() -> (Bench, i64) {
  let bench = Bench::new().await;
  let id = bench.item("an item already standing", INSTRUCTION).await;

  bench.served.gate.answer("POST /translations/kept/read", 200, json!({ "kept": [] }));
  (bench, id)
}

async fn append(bench: &Bench, id: i64, text: &str) -> Ran {
  bench.todo(&["--append", &id.to_string(), "--text", text]).await
}

// The words-added facts the agent was told, in the owner's chat.
fn words_added(bench: &Bench) -> Vec<Value> {
  bench
    .told()
    .into_iter()
    .map(|said| said["fact"].clone())
    .filter(|fact| fact["code"] == "words_appended")
    .collect()
}

// The lines a job's worker reads of an item.
async fn worker_lines(bench: &Bench, id: i64) -> String {
  let (status, body) = bench.served.get(&format!("/todos/{id}/worker-lines")).await;

  assert_eq!(status, 200, "{body}");
  body
    .as_array()
    .unwrap()
    .iter()
    .map(|line| line.as_str().unwrap_or_default().to_owned())
    .collect::<Vec<_>>()
    .join("\n")
}

#[tokio::test(flavor = "multi_thread")]
async fn the_words_land_on_the_item_as_the_owner_s_own_and_the_agent_is_told_of_them() {
  let (bench, id) = standing_item().await;
  let ran = append(&bench, id, "look at touch too").await;
  let notes = bench.notes(id).await;

  assert_eq!(ran.answer()["code"], "words_appended_told");
  assert_eq!(notes.len(), 1);
  assert_eq!(notes[0].author_id, Some(OWNER_SENDER));
  assert_eq!(notes[0].text, "look at touch too");
  assert_eq!(notes[0].job, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_running_for_the_item_is_written_on_the_row_and_the_agent_is_told_it_is_at_work() {
  let (bench, id) = standing_item().await;
  let job = bench.job(id, UNIT).await;

  append(&bench, id, "added while the job runs").await.answer();

  let told = words_added(&bench);

  assert_eq!(bench.notes(id).await[0].job, Some(job));
  assert_eq!(told.len(), 1, "{told:?}");
  assert_eq!(told[0]["id"], id);
  assert_eq!(told[0]["job"], job);
}

#[tokio::test(flavor = "multi_thread")]
async fn words_a_person_or_a_companion_adds_tell_the_agent_and_the_agent_s_own_and_a_job_s_tell_nothing() {
  let (bench, id) = standing_item().await;
  let item = id.to_string();

  bench.job(id, "bmo-task-1").await;

  let own = bench.agent_todo(&["--append", &item, "--text", "The owner wants touch too."]).await;
  let jobs = bench
    .job_todo("bmo-task-1", &["--append", &item, "--text", "Stopped after the parser."])
    .await;

  assert_eq!(own.answer()["code"], "words_appended_kept");
  assert_eq!(jobs.answer()["code"], "words_appended_kept");
  assert!(words_added(&bench).is_empty());

  append(&bench, id, "look at touch too").await.answer();
  bench
    .todo_as(
      As {
        agent: Some("companion"),
        ..As::default()
      },
      &["--append", &item, "--text", "they want it louder"],
    )
    .await
    .answer();

  assert_eq!(words_added(&bench).len(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_s_words_on_its_own_item_are_written_under_it_and_read_as_the_item_s_record() {
  let (bench, id) = standing_item().await;
  let job = bench.job(id, "bmo-task-own").await;
  let ran = bench
    .job_todo("bmo-task-own", &["--append", &id.to_string(), "--text", "Stopped after the parser."])
    .await;
  let notes = bench.notes(id).await;

  assert_eq!(ran.answer()["code"], "words_appended_kept");
  assert_eq!(notes[0].job, Some(job));
  assert!(words_added(&bench).is_empty());
  assert!(worker_lines(&bench, id).await.contains("Stopped after the parser."));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_s_words_on_another_item_are_written_under_it_tell_the_agent_and_reach_no_worker_there() {
  let (bench, id) = standing_item().await;
  let other = bench.item("the writing job's own item", INSTRUCTION).await;

  bench.job(id, "bmo-task-running").await;

  let writer = bench.job(other, "bmo-task-writer").await;
  let ran = bench
    .job_todo(
      "bmo-task-writer",
      &["--append", &id.to_string(), "--text", "The parser here needs the same fix."],
    )
    .await;
  let notes = bench.notes(id).await;
  let told = words_added(&bench);

  assert_eq!(ran.answer()["code"], "words_appended_told");
  assert_eq!(notes[0].job, Some(writer));
  assert_eq!(told.len(), 1, "{told:?}");
  assert!(!worker_lines(&bench, id).await.contains("The parser here needs the same fix."));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_session_adding_on_somebody_else_s_behalf_names_them_and_carries_no_id() {
  let (bench, id) = standing_item().await;

  bench
    .todo(&["--append", &id.to_string(), "--text", "one from me too", "--by", "Charlie"])
    .await
    .answer();

  let notes = bench.notes(id).await;

  assert_eq!((notes[0].author.as_str(), notes[0].author_id), ("Charlie", None));
}

#[tokio::test(flavor = "multi_thread")]
async fn the_owner_s_words_through_the_agent_stay_as_said_and_an_approval_note_is_kept() {
  let (bench, id) = standing_item().await;
  let item = id.to_string();

  assert_eq!(
    bench
      .agent_todo(&["--append", &item, "--text", "Keep the touch input as it is."])
      .await
      .code,
    0
  );
  assert_eq!(
    bench
      .agent_todo(&["--approve", &item, "--note", "Go ahead, start from the gate."])
      .await
      .code,
    0
  );
  assert_eq!(bench.todo(&["--approve", &item, "--note", "go ahead"]).await.code, 0);
  assert_eq!(bench.notes(id).await[0].text, "Keep the touch input as it is.");
  assert_eq!(bench.field(id, "note").await.as_deref(), Some("go ahead"));
}

#[tokio::test(flavor = "multi_thread")]
async fn nothing_at_all_is_not_something_to_add_and_a_finished_item_takes_none() {
  let (bench, id) = standing_item().await;

  assert_eq!(append(&bench, id, "   ").await.refused, "append_empty");
  assert!(bench.notes(id).await.is_empty());

  bench.finish(id).await;

  assert_eq!(append(&bench, id, "this one is late").await.refused, "item_closed_already");
  assert!(bench.notes(id).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn the_reading_carries_the_words_oldest_first_each_with_its_number_and_who_added_it() {
  let (bench, id) = standing_item().await;

  append(&bench, id, "words added first").await.answer();
  append(&bench, id, "words added later").await.answer();

  let shown = bench.shown(id).await;
  let notes = shown["notes"].as_array().unwrap();
  let first = bench.notes(id).await[0].id;

  assert_eq!(
    notes.iter().map(|note| note["text"].as_str().unwrap()).collect::<Vec<_>>(),
    ["words added first", "words added later"]
  );
  assert_eq!(notes[0]["id"], first);
  // Reading is reading: nothing read here was handed to a job.
  assert!(bench.notes(id).await.iter().all(|note| !note.delivered));
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_nobody_added_to_carries_no_words() {
  let (bench, id) = standing_item().await;

  assert_eq!(bench.shown(id).await["notes"], json!([]));
}

#[tokio::test(flavor = "multi_thread")]
async fn taking_one_lot_of_words_back_takes_it_off_every_later_reading_and_leaves_the_record() {
  let (bench, id) = standing_item().await;

  append(&bench, id, "words written wrong").await.answer();
  append(&bench, id, "words to keep").await.answer();

  let first = bench.notes(id).await[0].id;
  let taken = bench.todo(&["--unappend", &id.to_string(), "--note-id", &first.to_string()]).await;

  assert_eq!(
    taken.answer(),
    &json!({ "code": "note_removed", "id": id, "title": "an item already standing", "note": first })
  );
  assert_eq!(
    bench.notes(id).await.iter().map(|note| note.text.as_str()).collect::<Vec<_>>(),
    ["words to keep"]
  );
  assert_eq!(
    bench
      .count(r#"SELECT COUNT(*) FROM "todo_notes" WHERE "todo_id" = $1"#, vec![id.into()])
      .await,
    2
  );
  assert!(
    bench
      .text(r#"SELECT "removed_by" FROM "todo_notes" WHERE "id" = $1"#, vec![first.into()])
      .await
      .is_some()
  );
  assert!(!bench.shown(id).await["notes"].to_string().contains("words written wrong"));
}

#[tokio::test(flavor = "multi_thread")]
async fn taking_back_a_word_a_running_job_was_handed_says_it_stays_with_that_job() {
  let (bench, id) = standing_item().await;

  append(&bench, id, "words already gone").await.answer();

  let note = bench.notes(id).await[0].id;

  bench
    .run(r#"UPDATE "todo_notes" SET "delivered_at" = 1 WHERE "id" = $1"#, vec![note.into()])
    .await;

  assert_eq!(
    bench
      .todo(&["--unappend", &id.to_string(), "--note-id", &note.to_string()])
      .await
      .answer()["code"],
    "note_removed_after_delivery"
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn taking_back_a_word_a_change_already_handed_may_carry_says_it_stays_in_that_run() {
  let (bench, id) = standing_item().await;

  append(&bench, id, "words folded into the change").await.answer();
  append(&bench, id, "words after the change").await.answer();

  let notes = bench.notes(id).await;
  let (folded, later) = (notes[0].id, notes[1].id);
  let handed_at = 1_790_000_000_000_i64;

  for (note, added) in [(folded, handed_at - 20_000), (later, handed_at + 20_000)] {
    bench
      .run(
        r#"UPDATE "todo_notes" SET "added_at" = $2 WHERE "id" = $1"#,
        vec![note.into(), added.into()],
      )
      .await;
  }

  bench
    .run(
      r#"INSERT INTO "todo_notes" ("todo_id", "author", "text", "added_at", "kind", "delivered_at")
         VALUES ($1, 'Agent', 'The brief now asks for the words folded in.', $2, 'change', $3)"#,
      vec![id.into(), (handed_at - 10_000).into(), handed_at.into()],
    )
    .await;

  let item = id.to_string();

  assert_eq!(
    bench.todo(&["--unappend", &item, "--note-id", &folded.to_string()]).await.answer()["code"],
    "note_removed_after_delivery"
  );
  assert_eq!(
    bench.todo(&["--unappend", &item, "--note-id", &later.to_string()]).await.answer()["code"],
    "note_removed"
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn taking_back_refuses_words_that_are_not_there_words_taken_back_already_and_a_call_naming_none() {
  let (bench, id) = standing_item().await;

  append(&bench, id, "words taken back once").await.answer();

  let note = bench.notes(id).await[0].id;
  let id = id.to_string();

  assert_eq!(bench.todo(&["--unappend", &id, "--note-id", &note.to_string()]).await.code, 0);
  assert_eq!(
    bench.todo(&["--unappend", &id, "--note-id", &note.to_string()]).await.refused,
    "no_such_note"
  );
  assert_eq!(
    bench.todo(&["--unappend", &id, "--note-id", &(note + 100).to_string()]).await.refused,
    "no_such_note"
  );
  assert_eq!(bench.todo(&["--unappend", &id]).await.refused, "note_needs_number");
}

const BRIEF: &str =
  "Item #1 a brief\n\nWhat the work is\nFix the launcher.\n\nWhat needs doing\n1. Cover touch input too.";
const CHANGE: &str = "Cover touch input as well as the mouse.";

async fn rewrite_brief(bench: &Bench, id: i64) -> Ran {
  bench
    .agent_todo(&["--brief", &id.to_string(), "--text", BRIEF, "--change", CHANGE])
    .await
}

#[tokio::test(flavor = "multi_thread")]
async fn the_agent_s_rewrite_keeps_the_brief_and_the_change_as_its_own_note_with_the_running_job() {
  let (bench, id) = standing_item().await;
  let job = bench.job(id, "bmo-task-1").await;
  let ran = rewrite_brief(&bench, id).await;
  let notes = bench.notes(id).await;

  assert_eq!(
    ran.answer(),
    &json!({ "code": "brief_rewritten", "id": id, "title": "an item already standing", "job": job })
  );
  assert_eq!(bench.field(id, "brief").await.as_deref(), Some(BRIEF));
  assert_eq!(bench.field(id, "instruction").await.as_deref(), Some(INSTRUCTION));
  assert_eq!(notes.len(), 1);
  assert_eq!(notes[0].kind.as_deref(), Some("change"));
  assert_eq!(notes[0].author_id, None);
  assert_eq!(notes[0].text, CHANGE);
  assert_eq!(notes[0].job, Some(job));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rewrite_with_no_job_at_work_keeps_its_change_for_the_next_job() {
  let (bench, id) = standing_item().await;

  assert_eq!(rewrite_brief(&bench, id).await.answer()["job"], Value::Null);
  assert_eq!(bench.notes(id).await[0].job, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_brief_is_the_agent_s_alone_and_a_job_a_person_and_a_companion_are_refused() {
  let (bench, id) = standing_item().await;
  let item = id.to_string();
  let args = ["--brief", item.as_str(), "--text", BRIEF, "--change", CHANGE];
  let in_job = bench
    .todo_as(
      As {
        job: Some("bmo-task-1"),
        agent: Some(AGENT),
        ..As::default()
      },
      &args,
    )
    .await;
  let person = bench.todo(&args).await;
  let companion = bench
    .todo_as(
      As {
        agent: Some("companion"),
        ..As::default()
      },
      &args,
    )
    .await;

  for ran in [&in_job, &person, &companion] {
    assert_eq!(ran.refused, "brief_agent_only");
  }

  assert_eq!(bench.field(id, "brief").await, None);
  assert!(bench.notes(id).await.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn no_change_no_brief_one_not_in_english_and_a_finished_item_are_refused() {
  let (bench, id) = standing_item().await;
  let item = id.to_string();

  assert_eq!(
    bench.agent_todo(&["--brief", &item, "--text", BRIEF]).await.refused,
    "brief_needs_change"
  );
  assert_eq!(
    bench
      .agent_todo(&["--brief", &item, "--text", "  \n", "--change", CHANGE])
      .await
      .refused,
    "brief_empty"
  );
  assert_eq!(
    bench
      .agent_todo(&["--brief", &item, "--text", "Διορθώστε τον εκκινητή.", "--change", CHANGE])
      .await
      .refused,
    "brief_not_english"
  );
  assert_eq!(
    bench
      .agent_todo(&["--brief", &item, "--text", BRIEF, "--change", "Χειριστείτε και την αφή."])
      .await
      .refused,
    "brief_not_english"
  );

  bench.finish(id).await;

  assert_eq!(rewrite_brief(&bench, id).await.code, 1);
  assert_eq!(bench.field(id, "brief").await, None);
  assert!(bench.notes(id).await.is_empty());
}

// An item filed in the owner's chat out of one message, which stands as a draft.
async fn draft(text: &str) -> (Bench, i64) {
  let bench = Bench::new().await;

  bench.served.gate.answer("POST /translations/kept/read", 200, json!({ "kept": [] }));
  bench.message(OWNER_SENDER, "Owner", text);

  let mut args = vec!["--add", "--title", "one request"];

  args.extend_from_slice(&OWN_WORDS);
  args.extend_from_slice(&["--instruction", "7", "--reply-text", "Noted."]);

  let filed = bench.todo(&args).await;

  assert_eq!(filed.answer()["code"], "item_filed");

  let id = bench.last().await;

  (bench, id)
}

async fn order(bench: &Bench, id: i64) -> Value {
  queue_rows::work_orders::read(bench.served.db(), id)
    .await
    .unwrap()
    .expect("the item has an order")
}

#[tokio::test(flavor = "multi_thread")]
async fn a_rewrite_writes_the_work_order_s_instruction_too() {
  let (bench, id) = draft("file it as a draft").await;

  rewrite_brief(&bench, id).await.answer();

  assert_eq!(order(&bench, id).await["instruction"], BRIEF);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_reads_the_brief_and_the_agent_s_changes_in_place_of_the_item_s_own_sections() {
  let (bench, id) = standing_item().await;
  let before = worker_lines(&bench, id).await;

  rewrite_brief(&bench, id).await.answer();

  let after = worker_lines(&bench, id).await;

  assert!(before.contains("What the item asks for."), "{before}");
  assert!(after.contains("Fix the launcher."), "{after}");
  assert!(after.contains(CHANGE), "{after}");
  assert!(!after.contains("What the item asks for."), "{after}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_s_reading_leaves_out_a_person_s_words_and_the_approval_note() {
  let (bench, id) = standing_item().await;

  bench
    .run(r#"UPDATE "todos" SET "note" = 'RAW_APPROVAL_NOTE' WHERE "id" = $1"#, vec![id.into()])
    .await;
  append(&bench, id, "RAW_PERSON_NOTE").await.answer();
  bench.job(id, "bmo-task-1").await;
  bench
    .job_todo("bmo-task-1", &["--append", &id.to_string(), "--text", "Stopped after the parser."])
    .await
    .answer();

  let unbriefed = worker_lines(&bench, id).await;

  rewrite_brief(&bench, id).await.answer();

  let briefed = worker_lines(&bench, id).await;

  assert!(unbriefed.contains("Stopped after the parser."), "{unbriefed}");
  assert!(briefed.contains(CHANGE), "{briefed}");

  for raw in ["RAW_APPROVAL_NOTE", "RAW_PERSON_NOTE"] {
    assert!(!unbriefed.contains(raw), "{unbriefed}");
    assert!(!briefed.contains(raw), "{briefed}");
  }
}

async fn setting(bench: &Bench, key: &str) -> Option<String> {
  bench.text(r#"SELECT "value" FROM "settings" WHERE "key" = $1"#, vec![key.into()]).await
}

// What the server tells of the queue when asked now.
async fn told_now(bench: &Bench) -> String {
  let (_, body) = bench.served.post("/queue/tell", json!({ "moment": "approved", "repeat": true })).await;

  body["told"].as_str().unwrap_or_default().to_owned()
}

fn queue_told(bench: &Bench) -> usize {
  bench.told().iter().filter(|said| said["fact"]["code"] == "queue_waiting").count()
}

fn deploy_hold() -> Value {
  json!({
    "since": 1, "until": 2, "commits": [{ "commit": "a".repeat(40), "todoId": 698 }], "breaks": ["table x is taken away"],
    "waitingOn": [], "waitingFor": null, "passing": [],
  })
}

async fn hold_for_deploy(bench: &Bench) {
  bench
    .run(
      r#"INSERT INTO "settings" ("key", "value") VALUES ('deploy_hold', $1)"#,
      vec![deploy_hold().to_string().into()],
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_pause_writes_the_row_and_holds_the_fact_an_approval_would_otherwise_tell() {
  let (bench, id) = standing_item().await;

  assert_eq!(bench.todo(&["--pause"]).await.answers, [json!({ "code": "queue_paused" })]);
  assert_eq!(setting(&bench, "queue_paused").await.as_deref(), Some("1"));

  let before = queue_told(&bench);

  bench.todo(&["--unapprove", &id.to_string()]).await.answer();
  bench.todo(&["--approve", &id.to_string()]).await.answer();

  assert_eq!(queue_told(&bench), before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resume_clears_the_row_and_tells_the_agent_the_queue_that_was_waiting() {
  let (bench, _) = standing_item().await;

  bench.todo(&["--pause"]).await.answer();
  assert_eq!(told_now(&bench).await, "paused");

  let ran = bench.todo(&["--resume"]).await;

  assert_eq!(ran.answers, [json!({ "code": "queue_resumed" })]);
  assert_eq!(setting(&bench, "queue_paused").await, None);
  // The queue is read again, no longer held: what it tells is the server's own reading of it.
  assert_ne!(told_now(&bench).await, "paused");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_resume_leaves_a_deployment_s_hold_standing_and_says_what_it_waits_for() {
  let (bench, _) = standing_item().await;

  bench.todo(&["--pause"]).await.answer();
  hold_for_deploy(&bench).await;

  let ran = bench.todo(&["--resume"]).await;
  let held: Value = serde_json::from_str(&setting(&bench, "deploy_hold").await.unwrap()).unwrap();

  assert_eq!(ran.answers[0], json!({ "code": "queue_resumed" }));
  assert_eq!(ran.answers[1]["code"], "deploy_hold_stands");
  assert!(
    ran.answers[1]["hold"].as_str().unwrap().contains("aaaaaaaaaaaa (#698)"),
    "{:?}",
    ran.answers
  );
  assert_eq!(setting(&bench, "queue_paused").await, None);
  assert_eq!(held, deploy_hold());
}

#[tokio::test(flavor = "multi_thread")]
async fn releasing_the_deploy_hold_lets_it_go_and_tells_the_queue_and_none_standing_is_refused() {
  let (bench, _) = standing_item().await;

  hold_for_deploy(&bench).await;

  let ran = bench.todo(&["--release-deploy-hold"]).await;

  assert_eq!(ran.answer()["code"], "deploy_hold_released");
  assert_eq!(setting(&bench, "deploy_hold").await, None);
  assert_eq!(bench.todo(&["--release-deploy-hold"]).await.refused, "no_deploy_hold");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_replaces_the_parts_it_names_keeps_the_rest_and_the_draft_order_follows() {
  let (bench, id) = draft("file it as a draft").await;
  let ran = bench
    .todo(&[
      "--edit",
      &id.to_string(),
      "--title",
      "a rewritten title",
      "--do",
      "the work settled anew",
      "--conclusion",
      "the conclusion settled anew",
    ])
    .await;
  let instruction = bench.field(id, "instruction").await.unwrap();
  let order = order(&bench, id).await;

  assert_eq!(ran.answer()["code"], "item_edited");
  assert_eq!(bench.field(id, "title").await.as_deref(), Some("a rewritten title"));
  assert!(
    instruction.contains("the work settled anew") && instruction.contains("the conclusion settled anew"),
    "{instruction}"
  );
  assert!(!instruction.contains("that is what was settled"));
  // The part not named and the conversation both stand as they were.
  assert!(
    instruction.contains("what the work is") && instruction.contains("file it as a draft"),
    "{instruction}"
  );
  assert_eq!(order["status"], "draft");
  assert_eq!(order["intent"], "the conclusion settled anew");
  assert_eq!(order["instruction"], "the work settled anew");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_leaves_a_draft_order_part_somebody_wrote_apart_and_writes_one_named() {
  let (bench, id) = draft("a draft").await;
  let mut written = order(&bench, id).await;

  written["instruction"] = json!("an instruction written apart on the console");
  queue_rows::work_orders::save(bench.served.db(), &mut written).await.unwrap();
  bench
    .todo(&["--edit", &id.to_string(), "--do", "new work", "--intent", "a new purpose"])
    .await
    .answer();

  let after = order(&bench, id).await;

  assert_eq!(after["intent"], "a new purpose");
  assert_eq!(after["instruction"], "an instruction written apart on the console");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_edit_refuses_an_empty_part_a_part_the_item_lacks_a_running_job_and_a_finished_item() {
  let (bench, id) = draft("a draft").await;
  let before = bench.field(id, "instruction").await;

  assert_eq!(bench.todo(&["--edit", &id.to_string(), "--do", "  "]).await.refused, "edit_part_empty");
  assert_eq!(
    bench
      .todo(&["--edit", &id.to_string(), "--judgement", "the agent's judgement"])
      .await
      .refused,
    "edit_no_part"
  );

  bench.job(id, UNIT).await;

  assert_eq!(
    bench
      .todo(&["--edit", &id.to_string(), "--do", "work changed while running"])
      .await
      .refused,
    "edit_job_running"
  );
  assert_eq!(bench.field(id, "instruction").await, before);

  let (other, finished) = draft("a finished one").await;

  other.finish(finished).await;
  assert_eq!(
    other.todo(&["--edit", &finished.to_string(), "--title", "after it ended"]).await.code,
    1
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_title_alone_is_written_while_a_job_runs_and_the_words_stay_as_they_were() {
  let (bench, id) = draft("only the title while it runs").await;
  let before = bench.field(id, "instruction").await;

  bench.job(id, "bmo-task-2").await;

  assert_eq!(
    bench
      .todo(&["--edit", &id.to_string(), "--title", "what the work really was"])
      .await
      .answer()["code"],
    "item_edited"
  );
  assert_eq!(bench.field(id, "title").await.as_deref(), Some("what the work really was"));
  assert_eq!(bench.field(id, "instruction").await, before);
}
