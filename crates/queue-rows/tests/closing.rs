//! The closing's readings of a report and an unfinished note (docs/rows.md, "The closing's readings"), held to
//! the cases a report and a note are written in, and the notes a closing settles and the strands it hands over.

mod support;

use job_codes::todo_verbs::TodoRefusal;
use job_receiver::Said;
use queue_rows::closing::{
  LIVE_CLAUDE_HEADING, Owner, hand_over, outside_hosting_refusal, reopen_settled_sources,
  report_with_live_claude, resolve_settled_sources,
};
use serde_json::Value;
use support::{OWNER, Scene, reader};

fn cases() -> Value {
  serde_json::from_str(include_str!("fixtures/todo-closing.json")).expect("the closing's cases read")
}

fn texts(value: &Value) -> Vec<String> {
  value
    .as_array()
    .into_iter()
    .flatten()
    .filter_map(Value::as_str)
    .map(str::to_owned)
    .collect()
}

fn report(cases: &Value) -> String {
  texts(&cases["report"]).join("\n")
}

#[test]
fn the_left_section_is_the_list_under_its_heading_and_stops_where_the_list_does() {
  let cases = cases();
  let reader = reader();

  assert_eq!(reader.left_section(Some(&report(&cases))), texts(&cases["left"]));
  assert!(reader.left_section(cases["no_section"].as_str()).is_empty());
  assert!(reader.left_section(None).is_empty());
  assert_eq!(
    reader.left_section(cases["dressed"]["report"].as_str()),
    texts(&cases["dressed"]["left"])
  );
}

#[test]
fn a_line_proposes_where_it_suggests_or_defers_and_not_where_it_is_a_fact_a_row_or_the_owners_alone() {
  let cases = cases();
  let reader = reader();

  for line in texts(&cases["proposing"]) {
    assert!(reader.proposes(&line), "{line}");
  }

  for line in texts(&cases["not_proposing"]) {
    assert!(!reader.proposes(&line), "{line}");
  }
}

#[test]
fn the_leftover_refusal_names_every_proposing_line_and_strand_and_nothing_else() {
  let cases = cases();
  let reader = reader();
  let refusal = reader
    .leftover_refusal(Some(&report(&cases)), cases["refused"]["unfinished"].as_str())
    .expect("a refusal");

  assert!(refusal.contains("These lines of Left propose or defer an improvement:"), "{refusal}");
  assert!(
    refusal.contains("These strands of the Unfinished propose or defer an improvement:"),
    "{refusal}"
  );

  for named in texts(&cases["refused"]["named"]) {
    assert!(refusal.contains(&named), "{named}");
  }

  for left in texts(&cases["refused"]["not_named"]) {
    assert!(!refusal.contains(&left), "{left}");
  }

  assert_eq!(
    reader.leftover_refusal(cases["facts"]["report"].as_str(), cases["facts"]["unfinished"].as_str()),
    None
  );
  assert_eq!(reader.leftover_refusal(None, None), None);
}

#[test]
fn a_check_left_undone_is_named_and_one_beyond_any_jobs_reach_or_quoted_is_not() {
  let cases = cases();
  let reader = reader();
  let undone = texts(&cases["unchecked"]["left_undone"]);
  let settled = texts(&cases["unchecked"]["settled"]);
  let bare: Vec<String> = undone
    .iter()
    .map(|line| line.trim_start_matches(|c: char| c.is_ascii_digit() || c == '.' || c == ' ').to_owned())
    .collect();

  assert_eq!(reader.unchecked_lines(&undone.join("\n")), bare);
  assert!(reader.unchecked_lines(&settled.join("\n")).is_empty());

  let refusal = reader
    .unchecked_refusal(
      Some(&[&undone[..2], &settled[..]].concat().join("\n")),
      Some(&undone[2..].join("\n")),
    )
    .expect("a refusal");

  assert!(refusal.contains("only Owner holds") && !refusal.contains("{owner}"), "{refusal}");

  for line in &bare {
    assert!(refusal.contains(line.as_str()), "{line}");
  }

  for line in &settled {
    assert!(!refusal.contains(line.as_str()), "{line}");
  }

  assert_eq!(reader.unchecked_refusal(Some(&settled.join("\n")), None), None);
  assert_eq!(reader.unchecked_refusal(None, None), None);
}

#[test]
fn an_owner_strand_is_host_work_by_its_words_unless_it_names_what_only_the_owner_can_give() {
  let cases = cases();
  let reader = reader();
  let host = reader.owner_host_strands(cases["owner_strands"]["unfinished"].as_str());

  assert_eq!(host, texts(&cases["owner_strands"]["host"]));
  assert!(reader.owner_host_strands(None).is_empty());

  let refusal = reader.owner_host_refusal(&host);

  assert!(refusal.contains("Agent share:") && refusal.contains("Owner"), "{refusal}");
  assert!(
    refusal.contains("Install openssh-server") && !refusal.contains("Pick the voice"),
    "{refusal}"
  );
}

#[test]
fn a_note_is_read_as_its_strands_each_with_whose_it_is() {
  let reader = reader();
  let strands = reader.unfinished_strands(Some("Above the list.\n1. Agent share: Restart it.\n   On both nodes.\n2. Owner share: Pick one.\n3. Nobody's."));

  assert_eq!(strands.len(), 3);
  assert_eq!(
    (strands[0].text.as_str(), strands[0].owner),
    ("Restart it.\n   On both nodes.", Some(Owner::Agent))
  );
  assert_eq!((strands[1].text.as_str(), strands[1].owner), ("Pick one.", Some(Owner::Person)));
  assert_eq!(reader.unmarked_strands(Some("1. Agent share: a\n2. b")), ["b"]);
  assert_eq!(reader.unfinished_strands(Some("One whole note.")).len(), 1);
  assert_eq!(reader.marked_strand(&strands[1]), "Owner share: Pick one.");

  let note = "Lead\n1. Agent share: a\n2. b";
  let pieces = reader.strand_pieces(note);

  assert_eq!(
    pieces
      .iter()
      .map(|piece| format!("{}{}", piece.syntax, piece.words))
      .collect::<Vec<_>>()
      .join("\n"),
    note
  );
  assert_eq!((pieces[1].syntax.as_str(), pieces[1].words.as_str()), ("1. Agent share: ", "a"));
}

#[test]
fn a_strand_is_picked_by_its_number_and_needs_none_in_a_note_of_one() {
  let reader = reader();
  let two = "1. Agent share: first\n2. Owner share: second";

  assert_eq!(reader.pick_strand(3, "T", two, Some(2)).unwrap().text, "Owner share: second");
  assert_eq!(reader.pick_strand(3, "T", "Only one.", None).unwrap().strand, 1);

  let unnamed = reader.pick_strand(3, "T", two, None).unwrap_err();
  let missing = reader.pick_strand(3, "T", two, Some(5)).unwrap_err();

  assert_eq!((unnamed.code, missing.code), ("strand_not_named", "no_such_strand"));
  assert_eq!(
    missing.error,
    TodoRefusal::NoSuchStrand {
      id: 3,
      strand: 5,
      strands: 2
    }
    .to_string()
  );
}

#[test]
fn an_unmarked_note_is_refused_naming_its_strands() {
  let reader = reader();
  let refusal = reader.strand_format_refusal(&["first strand".to_owned()]);

  assert!(refusal.contains("Agent share:") && refusal.contains("Owner share:"), "{refusal}");
  assert!(refusal.contains("  1. first strand"), "{refusal}");
}

#[test]
fn a_page_hosted_outside_the_archive_is_refused_once_each() {
  let refusal = outside_hosting_refusal(
    &[Some("See https://gist.github.com/a/b. And https://gist.github.com/a/b again."), None],
    "Owner",
  )
  .expect("a refusal");

  assert_eq!(refusal.matches("https://gist.github.com/a/b").count(), 1);
  assert!(refusal.contains("shown to Owner"));
  assert_eq!(outside_hosting_refusal(&[Some("https://example.com/page")], "Owner"), None);
}

#[test]
fn the_live_claude_openings_are_written_afresh_under_their_heading() {
  let openings = vec![("Data collection".to_owned(), 2), ("Bench".to_owned(), 0)];
  let once = report_with_live_claude(Some("Fixed\n1. One"), &openings).expect("a report");

  assert_eq!(
    once,
    format!(
      "Fixed\n1. One\n\n{LIVE_CLAUDE_HEADING}\n1. Data collection: 2 real Claude runs\n2. Bench: 0 real Claude runs"
    )
  );
  assert_eq!(report_with_live_claude(Some(&once), &openings), Some(once.clone()));
  assert_eq!(report_with_live_claude(Some(&once), &[]), Some("Fixed\n1. One".to_owned()));
  assert_eq!(report_with_live_claude(None, &[]), None);
}

#[test]
fn english_is_read_past_its_code_links_and_set_aside_words() {
  let reader = reader();
  let syllable = '\u{AC00}';

  assert!(reader.written_in_english("Fix the launcher, as Owner share asked."));
  assert!(reader.written_in_english(&format!(
    "Quote `{syllable}{syllable}` and https://example.com/{syllable} freely."
  )));
  assert!(
    !reader.written_in_english(&format!("{syllable}{syllable}{syllable}{syllable}{syllable} few words"))
  );
  assert!(!reader.written_in_english(""));
}

#[tokio::test]
async fn a_note_is_settled_once_its_rows_are_all_finished_and_opened_again_when_one_is_not() {
  let scene = Scene::new().await;

  scene.item(1, "Source").await;
  scene.item(2, "First row").await;
  scene.item(3, "Second row").await;
  scene
    .run(
      r#"UPDATE "todos" SET "unfinished" = '1. Agent share: two strands' WHERE "id" = 1"#,
      vec![],
    )
    .await;
  scene
    .run(
      r#"INSERT INTO "todo_sources" ("todo_id", "source_todo_id") VALUES (2, 1), (3, 1)"#,
      vec![],
    )
    .await;
  scene.run(r#"UPDATE "todos" SET "done" = 1 WHERE "id" = 2"#, vec![]).await;

  assert!(resolve_settled_sources(&scene.db, &scene.reader, &[1], 5).await.unwrap().is_empty());

  scene.run(r#"UPDATE "todos" SET "done" = 1 WHERE "id" = 3"#, vec![]).await;

  let resolved = resolve_settled_sources(&scene.db, &scene.reader, &[1], 5).await.unwrap();

  assert_eq!(resolved.len(), 1);
  assert!(resolved[0].1.contains("#2 First row, #3 Second row"), "{}", resolved[0].1);
  assert_eq!(
    scene.column::<Option<String>>(1, "unfinished_resolution").await,
    Some(resolved[0].1.clone())
  );

  scene.run(r#"UPDATE "todos" SET "done" = 0 WHERE "id" = 3"#, vec![]).await;

  assert_eq!(reopen_settled_sources(&scene.db, &[1]).await.unwrap(), [1]);
  assert_eq!(scene.column::<Option<String>>(1, "unfinished_resolution").await, None);
}

#[tokio::test]
async fn the_agents_strands_are_handed_over_once_each_as_memories_and_orders() {
  let scene = Scene::new().await;
  let note = "1. Agent share: Restart the n2 mount.\n2. Owner share: Pick the voice.\n3. Agent share: Renew the backup key.";
  let ground = scene.ground();

  let handed = hand_over(&ground, 9, "Fix it", Some(note)).await.unwrap();

  assert_eq!(
    handed,
    [(1, "Restart the n2 mount.".to_owned()), (2, "Renew the backup key.".to_owned())]
  );
  assert!(
    hand_over(&ground, 9, "Fix it", Some(note)).await.unwrap().is_empty(),
    "a strand is handed over once"
  );

  let memories = scene.units.memories();

  assert!(
    memories.iter().all(|memory| memory.todo_id == Some(9)
      && memory.chat_id == Some(OWNER)
      && memory.kind == "followup")
  );

  let orders = scene.units.instructions();

  assert_eq!(orders.len(), 2);
  assert_eq!(orders[0].sort, "unfinished");

  let Said::Order(text) = &orders[1].said else {
    panic!("a strand is handed over as an order")
  };

  assert!(
    text.contains("Renew the backup key.")
      && text.contains("memory 2")
      && text.contains(&format!("chat {OWNER}")),
    "{text}"
  );
}
