//! Filing a row (docs/rows.md, "Filing an item"): out of a conversation, out of the agent's own judgement on an
//! unfinished note, and the worker, co-work, impact grounds and repair link set on it, with the words the verb
//! does not read — each read back through the server's readings.

use serde_json::json;

use crate::support::todo::{AGENT, As, Bench, MEMBER, OWN_WORDS, OWNER_CHAT, OWNER_SENDER, ROOM, Ran};

const WORDING: &str = "## Banned terms\n\n- en · blastoff → start\n";

// An ordinary filing out of the room's one message, in the owner's chat unless a room is named.
async fn filed(bench: &Bench, title: &str, chat: Option<i64>, extra: &[&str]) -> Ran {
  let chat = chat.map(|chat| chat.to_string());
  let mut args = vec!["--add", "--title", title];

  args.extend_from_slice(&OWN_WORDS);
  args.extend_from_slice(&["--instruction", "7", "--reply-text", "Noted."]);

  if let Some(chat) = &chat {
    args.extend_from_slice(&["--chat", chat]);
  }

  args.extend_from_slice(extra);
  bench.todo(&args).await
}

async fn with_message(sender: i64, name: &str, text: &str) -> Bench {
  let bench = Bench::worded(Some(WORDING)).await;

  bench.served.gate.answer("POST /translations/kept/read", 200, json!({ "kept": [] }));
  bench.message(sender, name, text);
  bench
}

async fn items(bench: &Bench) -> i64 {
  bench.count(r#"SELECT COUNT(*) FROM "todos""#, vec![]).await
}

async fn open_ids(bench: &Bench) -> Vec<i64> {
  bench
    .listed("")
    .await
    .as_array()
    .unwrap()
    .iter()
    .filter(|line| line["state"].is_null())
    .map(|line| line["id"].as_i64().unwrap())
    .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_room_s_request_lands_unapproved_with_the_member_who_asked_as_the_requester() {
  let bench = with_message(MEMBER, "Charlie", "could you do this for me").await;
  let ran = filed(&bench, "one request", Some(ROOM), &[]).await;
  let id = bench.last().await;

  assert_eq!(ran.answer()["code"], "item_filed");
  assert_eq!(ran.answer()["id"], id);
  assert_eq!(bench.field_number(id, "approved").await, Some(0));
  assert_eq!(bench.field(id, "requester").await.as_deref(), Some("Charlie"));
  assert_eq!(bench.field_number(id, "requester_id").await, Some(MEMBER));
  assert_eq!(bench.field_number(id, "chat_id").await, Some(ROOM));

  // The words themselves, both sides, as the item will be read months later.
  let instruction = bench.field(id, "instruction").await.unwrap();

  assert!(
    instruction.contains("could you do this for me") && instruction.contains("Noted."),
    "{instruction}"
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_title_using_a_set_aside_term_is_refused_with_the_term_and_nothing_is_written() {
  let bench = with_message(MEMBER, "Charlie", "this one too").await;
  let refused = filed(&bench, "show the blastoff", Some(ROOM), &[]).await;

  assert_eq!(refused.refused, "banned_terms");
  assert!(refused.said.contains("blastoff"), "{}", refused.said);
  assert_eq!(items(&bench).await, 0);

  filed(&bench, "show the reason", Some(ROOM), &[]).await.answer();

  let id = bench.last().await;

  assert_eq!(
    bench.todo(&["--edit", &id.to_string(), "--title", "a blastoff here"]).await.refused,
    "banned_terms"
  );
  assert_eq!(bench.field(id, "title").await.as_deref(), Some("show the reason"));
}

#[tokio::test(flavor = "multi_thread")]
async fn filing_approves_nothing_not_even_from_the_owner_s_own_chat() {
  let bench = with_message(OWNER_SENDER, "Owner", "do this").await;

  filed(&bench, "one request", None, &[]).await.answer();

  let id = bench.last().await;

  assert_eq!(bench.field_number(id, "requester_id").await, Some(OWNER_SENDER));
  assert_eq!(bench.field_number(id, "approved").await, Some(0));
}

#[tokio::test(flavor = "multi_thread")]
async fn approving_puts_an_item_in_the_queue_and_closing_takes_it_out_and_keeps_it() {
  let bench = with_message(MEMBER, "Charlie", "this one too").await;

  filed(&bench, "request two", Some(ROOM), &[]).await.answer();

  let id = bench.last().await;

  assert_eq!(bench.todo(&["--approve", &id.to_string()]).await.answer()["code"], "item_approved");
  assert_eq!(bench.field_number(id, "approved").await, Some(1));
  assert!(open_ids(&bench).await.contains(&id));

  bench.todo(&["--done", &id.to_string()]).await.answer();

  assert!(!open_ids(&bench).await.contains(&id));
  assert!(bench.done(id).await);
  assert_eq!(items(&bench).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn every_approval_given_or_taken_back_at_a_shell_is_recorded_with_whose_hand_it_was() {
  let bench = with_message(MEMBER, "Charlie", "record who did it").await;

  filed(&bench, "request five", Some(ROOM), &[]).await.answer();

  let id = bench.last().await;
  let item = id.to_string();

  bench.todo(&["--approve", &item]).await.answer();
  bench
    .todo_as(
      As {
        job: Some("worker-w5-9"),
        ..As::default()
      },
      &["--unapprove", &item],
    )
    .await
    .answer();
  bench.agent_todo(&["--approve", &item]).await.answer();

  let rows = job_base::all(
    bench.served.db(),
    r#"SELECT "approved", "who_kind", "who" FROM "todo_approvals" WHERE "todo_id" = $1 ORDER BY "id""#,
    vec![id.into()],
  )
  .await
  .unwrap();
  let read: Vec<(i64, String, String)> = rows
    .iter()
    .map(|row| {
      (
        row.try_get("", "approved").unwrap(),
        row.try_get("", "who_kind").unwrap(),
        row.try_get("", "who").unwrap(),
      )
    })
    .collect();

  assert_eq!(read.len(), 3, "{read:?}");
  assert_eq!((read[0].0, read[0].2.as_str()), (1, "person"));
  assert_eq!((read[1].0, read[1].2.as_str()), (0, "worker-w5-9"));
  assert_eq!((read[2].0, read[2].2.as_str()), (1, AGENT));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_message_that_is_not_kept_files_nothing_at_all() {
  let bench = Bench::new().await;

  bench.served.receiver.answer_empty("GET /rooms/{chat}/messages/{message}", 404);

  let ran = filed(&bench, "no such message", Some(ROOM), &[]).await;

  assert_eq!(ran.refused, "message_not_archived");
  assert_eq!(items(&bench).await, 0);
}

// Every argument of an ordinary filing, so a test can take one away and see what happens.
fn four_parts(chat: &str) -> Vec<String> {
  [
    "--add",
    "--title",
    "an item of four parts",
    "--what",
    "work on the TODO panel",
    "--do",
    "make the opened cell taller and check it on screen",
    "--instruction",
    "7",
    "--reply-text",
    "Noted.",
    "--conclusion",
    "only the height grows and the report cell stays",
    "--chat",
    chat,
  ]
  .map(str::to_owned)
  .to_vec()
}

fn borrowed(args: &[String]) -> Vec<&str> {
  args.iter().map(String::as_str).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn the_parts_are_kept_with_the_conversation_and_every_message_named_is_carried() {
  let bench = with_message(MEMBER, "Charlie", "make the panel narrower").await;
  let mut args = four_parts(&ROOM.to_string());

  // Each message of the conversation is named by a flag of its own.
  args.extend(["--instruction".to_owned(), "8".to_owned()]);
  bench.todo(&borrowed(&args)).await.answer();

  let instruction = bench.field(bench.last().await, "instruction").await.unwrap();

  for part in [
    "work on the TODO panel",
    "make the opened cell taller and check it on screen",
    "only the height grows and the report cell stays",
  ] {
    assert!(instruction.contains(part), "{instruction}");
  }

  // Both messages named stand as the conversation.
  assert_eq!(instruction.matches("make the panel narrower").count(), 2, "{instruction}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_filing_missing_any_one_of_the_three_or_with_one_empty_writes_nothing() {
  for missing in ["--what", "--do", "--conclusion"] {
    let bench = with_message(MEMBER, "Charlie", "make the panel narrower").await;
    let mut args = four_parts(&ROOM.to_string());
    let at = args.iter().position(|arg| arg == missing).unwrap();

    args.drain(at..at + 2);

    let ran = bench.todo(&borrowed(&args)).await;

    assert_eq!(ran.refused, "filing_parts_missing", "{missing}");
    assert_eq!(items(&bench).await, 0);
  }

  let bench = with_message(MEMBER, "Charlie", "make the panel narrower").await;
  let mut args = four_parts(&ROOM.to_string());
  let at = args.iter().position(|arg| arg == "--conclusion").unwrap();

  args[at + 1] = "   ".to_owned();

  assert_eq!(bench.todo(&borrowed(&args)).await.refused, "filing_parts_missing");
  assert_eq!(items(&bench).await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_filing_short_of_its_parts_names_the_parts_it_is_short_of() {
  let bench = with_message(OWNER_SENDER, "Owner", "do this").await;
  let refused = bench
    .todo(&[
      "--add",
      "--title",
      "a half-filled item",
      "--what",
      "what the work is",
      "--instruction",
      "7",
      "--reply-text",
      "Right.",
    ])
    .await;

  assert_eq!(refused.refused, "filing_parts_missing");
  assert!(
    refused.said.contains("do") && refused.said.contains("conclusion") && !refused.said.contains("what"),
    "{}",
    refused.said
  );
  assert_eq!(items(&bench).await, 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_row_filed_before_the_parts_existed_still_reads_whole() {
  let bench = Bench::new().await;
  let before = "Said instruction (Owner, 2025-09-01T00:00:00.000Z, message 3):\n```text\nthe thing asked for back then\n```";
  let id = bench.item("an item filed back then", before).await;

  assert!(
    bench.shown(id).await["instruction"]
      .as_str()
      .unwrap()
      .contains("the thing asked for back then")
  );
}

const JUDGEMENT: &str =
  "The approval screen ended without its note mark. It is work left, so raise it as an item.";
const LEFTOVER: &str = "The note mark was not put on, and whoever comes next has to.";

// Two items already in the table: one that left an unfinished note, and the one whose work read it.
async fn swept() -> (Bench, i64, i64) {
  let bench = Bench::new().await;
  let source = bench.item("fix the approval screen", "fix the approval screen").await;
  let sweep = bench.item("sweep the notes", "sweep the notes").await;
  let note = format!("1. {} {LEFTOVER}", bench.stand.policy.closing.strands.owner_mark);

  bench
    .todo(&["--done", &source.to_string(), "--report", "The screen is fixed.", "--unfinished", &note])
    .await
    .answer();
  (bench, source, sweep)
}

async fn raise(bench: &Bench, title: &str, from: Option<&str>, by: Option<&str>, extra: &[&str]) -> Ran {
  let chat = OWNER_CHAT.to_string();
  let mut args = vec!["--add", "--title", title, "--chat", &chat, "--judgement", JUDGEMENT];

  if let Some(from) = from {
    args.extend_from_slice(&["--from-todo", from]);
  }

  if let Some(by) = by {
    args.extend_from_slice(&["--raised-by", by]);
  }

  args.extend_from_slice(extra);
  bench.agent_todo(&args).await
}

#[tokio::test(flavor = "multi_thread")]
async fn a_raised_row_stands_as_the_agent_s_own_request_with_the_judgement_and_the_source_note() {
  let (bench, source, sweep) = swept().await;
  let ran =
    raise(&bench, "put the note mark on", Some(&source.to_string()), Some(&sweep.to_string()), &[]).await;
  let id = bench.last().await;
  let instruction = bench.field(id, "instruction").await.unwrap();

  assert_eq!(ran.answer()["code"], "item_raised");
  assert_eq!(ran.answer()["item"], sweep);
  assert_eq!(ran.answer()["sources"][0]["id"], source);
  assert_eq!(bench.field_number(id, "requester_id").await, None);
  // Filing approves nothing.
  assert_eq!(bench.field_number(id, "approved").await, Some(0));
  assert!(instruction.contains(JUDGEMENT) && instruction.contains(LEFTOVER), "{instruction}");
  assert!(!instruction.contains("Said instruction"), "nobody asked for it: {instruction}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_raising_carries_the_one_strand_it_names_out_of_a_note_of_several() {
  let (bench, _, sweep) = swept().await;
  let mark = bench.stand.policy.closing.strands.owner_mark.clone();
  let several = bench.item("situation rules", "situation rules").await;
  let note = format!(
    "1. {mark} decide whether the shadow mode moves to a filter.\n2. {mark} decide whether the console's switch hover changes in the theme."
  );

  bench
    .todo(&["--done", &several.to_string(), "--report", "The rules stand.", "--unfinished", &note])
    .await
    .answer();

  let before = items(&bench).await;
  let unnamed = raise(&bench, "no number", Some(&several.to_string()), Some(&sweep.to_string()), &[]).await;
  let malformed =
    raise(&bench, "number shape", Some(&format!("{several}:two")), Some(&sweep.to_string()), &[]).await;

  assert_eq!(unnamed.refused, "strand_not_named");
  assert_eq!(malformed.refused, "from_todo_unread");
  assert_eq!(items(&bench).await, before);

  let ran = raise(
    &bench,
    "switch hover exception",
    Some(&format!("{several}:2")),
    Some(&sweep.to_string()),
    &[],
  )
  .await;
  let id = bench.last().await;
  let instruction = bench.field(id, "instruction").await.unwrap();

  assert_eq!(ran.answer()["sources"], json!([{ "id": several, "strand": "2" }]));
  assert!(
    instruction.contains("console's switch hover changes in the theme")
      && !instruction.contains("shadow mode"),
    "{instruction}"
  );
  assert_eq!(
    bench
      .count(r#"SELECT COUNT(*) FROM "todo_sources" WHERE "todo_id" = $1"#, vec![id.into()])
      .await,
    1
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_judgement_takes_no_part_of_a_conversation_and_an_empty_one_files_nothing() {
  let (bench, source, sweep) = swept().await;
  let before = items(&bench).await;
  let (source, sweep) = (source.to_string(), sweep.to_string());

  assert_eq!(
    raise(&bench, "two sources", Some(&source), Some(&sweep), &["--reply-text", "Noted."])
      .await
      .refused,
    "judgement_with_conversation"
  );

  for flag in ["--what", "--do", "--conclusion"] {
    assert_eq!(
      raise(
        &bench,
        "a part on a judgement",
        Some(&source),
        Some(&sweep),
        &[flag, "what the work is"]
      )
      .await
      .refused,
      "judgement_with_conversation",
      "{flag}"
    );
  }

  let chat = OWNER_CHAT.to_string();
  let empty = bench
    .agent_todo(&[
      "--add",
      "--title",
      "empty judgement",
      "--chat",
      &chat,
      "--judgement",
      "   ",
      "--from-todo",
      &source,
      "--raised-by",
      &sweep,
    ])
    .await;

  assert_eq!(empty.refused, "judgement_empty");
  assert_eq!(items(&bench).await, before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_raising_files_with_the_judgement_alone() {
  let (bench, source, sweep) = swept().await;

  raise(
    &bench,
    "an item on a judgement alone",
    Some(&source.to_string()),
    Some(&sweep.to_string()),
    &[],
  )
  .await
  .answer();

  let instruction = bench.field(bench.last().await, "instruction").await.unwrap();

  assert!(instruction.contains(JUDGEMENT), "{instruction}");
  assert!(!instruction.contains("what the work is"), "{instruction}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_source_that_is_not_an_item_or_one_that_left_no_note_files_nothing() {
  let (bench, _, sweep) = swept().await;
  let before = items(&bench).await;

  assert_eq!(
    raise(&bench, "no such source", Some("999"), Some(&sweep.to_string()), &[])
      .await
      .refused,
    "source_not_item"
  );
  assert_eq!(
    raise(&bench, "a source with no note", Some(&sweep.to_string()), Some(&sweep.to_string()), &[])
      .await
      .refused,
    "source_left_nothing"
  );
  assert_eq!(items(&bench).await, before);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_row_a_job_raised_from_its_own_work_stands_with_no_source() {
  let (bench, _, sweep) = swept().await;

  raise(&bench, "an improvement out of scope", None, Some(&sweep.to_string()), &[])
    .await
    .answer();

  let id = bench.last().await;

  assert!(bench.field(id, "instruction").await.unwrap().contains(JUDGEMENT));
  assert_eq!(
    bench
      .count(r#"SELECT COUNT(*) FROM "todo_sources" WHERE "todo_id" = $1"#, vec![id.into()])
      .await,
    0
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_raising_item_that_is_missing_or_unknown_files_nothing() {
  let (bench, source, _) = swept().await;
  let before = items(&bench).await;

  assert_eq!(
    raise(&bench, "no raising item", Some(&source.to_string()), None, &[]).await.refused,
    "raised_by_needed"
  );
  assert_eq!(
    raise(&bench, "an unknown item", Some(&source.to_string()), Some("999"), &[])
      .await
      .refused,
    "raised_by_not_item"
  );
  assert_eq!(items(&bench).await, before);
}

// An item filed in the owner's chat, for the worker, co-work and impact verbs to set.
async fn filed_item(extra: &[&str]) -> (Bench, i64) {
  let bench = with_message(OWNER_SENDER, "Owner", "do this").await;

  filed(&bench, "an item to set up", None, extra).await.answer();

  let id = bench.last().await;

  (bench, id)
}

async fn worker(bench: &Bench, name: &str) {
  bench
    .run(
      r#"INSERT INTO "workers" ("name", "label", "cwd", "builtin", "support_count", "created_at") VALUES ($1, 'Infra', '/srv/infra', 0, 0, 1)"#,
      vec![name.into()],
    )
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_registered_worker_is_written_on_the_row_and_read_back_and_a_dash_takes_it_off() {
  let (bench, id) = filed_item(&[]).await;

  worker(&bench, "infra").await;

  let assigned = bench.todo(&["--worker", &id.to_string(), "infra"]).await;

  assert_eq!(
    assigned.answer(),
    &json!({ "code": "worker_set", "id": id, "title": "an item to set up", "worker": "infra" })
  );
  assert_eq!(bench.field(id, "worker").await.as_deref(), Some("infra"));
  assert_eq!(bench.shown(id).await["worker"], "infra");
  assert_eq!(bench.shown(id).await["worker_assigned"], true);

  assert_eq!(bench.todo(&["--worker", &id.to_string(), "-"]).await.answer()["code"], "worker_cleared");
  assert_eq!(bench.field(id, "worker").await, None);
  assert_eq!(bench.shown(id).await["worker_assigned"], false);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_worker_nobody_registered_is_refused_rather_than_written() {
  let (bench, id) = filed_item(&[]).await;

  assert_eq!(
    bench.todo(&["--worker", &id.to_string(), "a vanished worker"]).await.refused,
    "no_such_worker"
  );
  assert_eq!(bench.field(id, "worker").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn co_work_is_filed_off_turned_on_and_off_and_read_wherever_the_item_is_read() {
  let (bench, id) = filed_item(&[]).await;
  let item = id.to_string();

  assert_eq!(bench.shown(id).await["cowork"], false);

  assert_eq!(
    bench.todo(&["--cowork", &item, "on"]).await.answer(),
    &json!({ "code": "cowork_set", "id": id, "title": "an item to set up", "on": true })
  );
  assert_eq!(bench.field_number(id, "cowork").await, Some(1));
  assert_eq!(bench.shown(id).await["cowork"], true);

  assert_eq!(bench.todo(&["--cowork", &item, "off"]).await.answer()["on"], false);
  assert_eq!(bench.field_number(id, "cowork").await.unwrap_or(0), 0);

  assert_eq!(bench.todo(&["--cowork", &item, "yes"]).await.refused, "cowork_needs_switch");
  assert_eq!(bench.field_number(id, "cowork").await.unwrap_or(0), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn filing_takes_co_work_as_a_bare_flag_on_the_new_row_and_turning_another_on_leaves_it_alone() {
  let (bench, id) = filed_item(&["--cowork"]).await;

  assert_eq!(bench.field_number(id, "cowork").await, Some(1));

  filed(&bench, "just one session", None, &[]).await.answer();

  let other = bench.last().await;

  bench.todo(&["--cowork", &id.to_string(), "off"]).await.answer();
  bench.todo(&["--cowork", &id.to_string(), "on"]).await.answer();

  assert_eq!(bench.field_number(other, "cowork").await.unwrap_or(0), 0);
}

// Grounds of one contract that changes and one node changed inside only.
const GROUNDS: &str = r#"{"nodes":[{"id":"bot/work-orders","contract":"the plan JSON gains a field"},{"id":"lang/bmo-lang","contract":null}],"new_home":false}"#;

#[tokio::test(flavor = "multi_thread")]
async fn impact_grounds_are_written_said_with_their_verdict_and_read_back() {
  let (bench, id) = filed_item(&[]).await;
  let set = bench.todo(&["--impact", &id.to_string(), GROUNDS]).await;

  assert_eq!(set.answer()["code"], "impact_set");
  assert_eq!(set.answer()["contracts"], 1);
  assert_eq!(set.answer()["cowork"], false);

  let shown = bench.shown(id).await;

  assert!(
    shown["impact"].to_string().contains("bot/work-orders")
      && shown["impact"].to_string().contains("lang/bmo-lang"),
    "{}",
    shown["impact"]
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn filing_takes_the_grounds_and_grounds_that_do_not_read_file_or_change_nothing() {
  let (bench, id) = filed_item(&["--impact", GROUNDS]).await;

  assert!(bench.field(id, "impact").await.is_some_and(|kept| kept.contains("bot/work-orders")));
  assert_eq!(
    filed(&bench, "weighed on nothing", None, &["--impact", r#"{"nodes":[]}"#])
      .await
      .refused,
    "impact_unread"
  );
  assert_eq!(items(&bench).await, 1);
  assert_eq!(bench.todo(&["--impact", &id.to_string(), "{not json"]).await.refused, "impact_unread");
  assert!(bench.field(id, "impact").await.is_some_and(|kept| kept.contains("bot/work-orders")));
}

// The orders the agent was handed of the item's impact, where its job's grounds raised one.
fn impact_orders(bench: &Bench) -> Vec<String> {
  bench
    .served
    .receiver
    .calls()
    .into_iter()
    .filter(|call| call.route == "POST /instructions")
    .filter_map(|call| call.body)
    .filter(|body| body["sort"] == "impact_mismatch")
    .map(|body| body["said"].to_string())
    .collect()
}

// Grounds of two contracts that change, which come to a co-work.
const WIDER: &str = r#"{"nodes":[{"id":"bot/work-orders","contract":"the plan JSON gains a field"},{"id":"archive/bmo-archive","contract":"todos gains a column"}]}"#;

#[tokio::test(flavor = "multi_thread")]
async fn a_job_s_grounds_that_come_to_the_item_s_setting_are_kept_and_nobody_is_told() {
  let (bench, id) = filed_item(&["--impact", GROUNDS]).await;
  let found = r#"{"nodes":[{"id":"bot/work-orders","contract":"the plan JSON gains a field"}]}"#;
  let ran = bench.job_todo("bmo-task-impact", &["--impact-check", &id.to_string(), found]).await;

  assert_eq!(ran.answer(), &json!({ "code": "impact_check_kept", "id": id }));
  assert!(bench.field(id, "job_impact").await.is_some_and(|kept| kept.contains("bot/work-orders")));
  assert!(bench.field(id, "impact").await.is_some_and(|kept| kept.contains("lang/bmo-lang")));
  assert!(impact_orders(&bench).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn a_job_s_grounds_that_come_to_another_verdict_tell_the_agent_both_and_leave_the_setting() {
  let (bench, id) = filed_item(&["--impact", GROUNDS]).await;
  let ran = bench.job_todo("bmo-task-impact", &["--impact-check", &id.to_string(), WIDER]).await;

  assert_eq!(ran.answer()["code"], "impact_check_told");
  assert!(
    bench
      .field(id, "job_impact")
      .await
      .is_some_and(|kept| kept.contains("archive/bmo-archive"))
  );
  assert_eq!(bench.field_number(id, "cowork").await.unwrap_or(0), 0);

  let told = impact_orders(&bench);

  assert_eq!(told.len(), 1);
  assert!(
    told[0].contains("lang/bmo-lang") && told[0].contains("archive/bmo-archive"),
    "{}",
    told[0]
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn an_item_filed_without_grounds_tells_the_agent_whatever_the_job_s_verdict() {
  let (bench, id) = filed_item(&[]).await;
  let ran = bench
    .job_todo(
      "bmo-task-impact",
      &["--impact-check", &id.to_string(), r#"{"nodes":[{"id":"lang/bmo-lang"}]}"#],
    )
    .await;

  assert_eq!(ran.answer()["code"], "impact_check_told");
  assert!(bench.field(id, "job_impact").await.is_some());
  assert_eq!(bench.field(id, "impact").await, None);
  assert_eq!(impact_orders(&bench).len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repair_link_says_where_the_chain_stands_refuses_past_the_ceiling_and_is_taken_off_by_a_dash() {
  let bench = Bench::new().await;
  let root = bench.item("the work that failed first", "the work that failed first").await;
  let mut target = root;
  let mut repairs = vec![];

  for round in 1..=3 {
    let repair = bench.item(&format!("repair work {round}"), "repair work").await;

    bench
      .run(
        r#"UPDATE "todos" SET "repairs_todo_id" = $2 WHERE "id" = $1"#,
        vec![repair.into(), target.into()],
      )
      .await;
    repairs.push(repair);
    target = repair;
  }

  let fourth = bench.item("repair work 4", "repair work").await;

  assert_eq!(
    bench.todo(&["--repairs", &fourth.to_string(), &repairs[2].to_string()]).await.refused,
    "repairs_chain_full"
  );

  let other = bench.item("other work", "other work").await;
  let linked = bench.todo(&["--repairs", &fourth.to_string(), &other.to_string()]).await;

  assert_eq!(
    linked.answer(),
    &json!({ "code": "repairs_set", "id": fourth, "title": "repair work 4", "target": other, "root": other, "round": 1, "rounds": 3 })
  );
  assert_eq!(bench.field_number(fourth, "repairs_todo_id").await, Some(other));
  assert_eq!(bench.shown(other).await["repaired_by"][0]["id"], fourth);

  assert_eq!(
    bench.todo(&["--repairs", &fourth.to_string(), "-"]).await.answer()["code"],
    "repairs_cleared"
  );
  assert_eq!(bench.field_number(fourth, "repairs_todo_id").await, None);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_repair_link_is_not_taken_with_a_filing() {
  let bench = Bench::new().await;

  assert_eq!(
    bench.todo(&["--add", "--title", "x", "--repairs", "1", "2"]).await.refused,
    "repairs_on_filing"
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_flag_the_verb_does_not_read_files_and_changes_nothing() {
  let bench = with_message(OWNER_SENDER, "Owner", "do this").await;
  let refused = filed(&bench, "an item with a made-up flag", None, &["--made-up-flag"]).await;

  assert_eq!(refused.code, 2);
  assert!(refused.said.contains("--made-up-flag"), "{}", refused.said);
  assert_eq!(items(&bench).await, 0);

  filed(&bench, "an item to set up", None, &[]).await.answer();

  let id = bench.last().await;

  assert_eq!(bench.todo(&["--cowork", &id.to_string(), "on", "--made-up-flag"]).await.code, 2);
  assert_eq!(bench.field_number(id, "cowork").await.unwrap_or(0), 0);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_value_that_begins_with_two_dashes_belongs_to_the_flag_before_it() {
  let bench = with_message(OWNER_SENDER, "Owner", "do this").await;
  let mut args = vec!["--add", "--title", "an answer opening with dashes"];

  args.extend_from_slice(&OWN_WORDS);
  args.extend_from_slice(&["--instruction", "7", "--reply-text", "--made-up-flag is what you typed."]);

  bench.todo(&args).await.answer();
  assert_eq!(items(&bench).await, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_verb_given_the_wrong_thing_is_refused_by_what_it_lacks() {
  let (bench, id) = filed_item(&[]).await;
  let id = id.to_string();
  let cases: [(Vec<&str>, &str); 6] = [
    (vec!["--mark", &id], "mark_needs_status"),
    (vec!["--cowork", &id, "maybe"], "cowork_needs_switch"),
    (vec!["--priority", &id, "high"], "priority_needs_number"),
    (vec!["--worker", &id, ""], "worker_needs_name"),
    (vec!["--resolve", &id, "--why", "  "], "resolve_needs_why"),
    (vec!["--edit", &id], "edit_nothing"),
  ];

  for (args, code) in cases {
    assert_eq!(bench.todo(&args).await.refused, code, "{args:?}");
  }
}
