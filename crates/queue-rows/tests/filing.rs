//! The verbs of filing (docs/rows.md, "The verbs of filing"): words added, a brief rewritten, words taken back,
//! an approval given and taken back, and a row filed out of a conversation or raised on the agent's judgement.

mod support;

use job_agency::TurnRecordKind;
use job_codes::todo_verbs::{FilingPart, StrandSource, TodoAnswer};
use job_receiver::Said;
use queue_rows::filing::{
  Append, Approving, Brief, Filing, add, append, approve, brief, is_job_model, notes, strand_picks,
  unappend,
};
use queue_rows::wording::BannedTerm;
use queue_rows::{Ground, Written};
use support::{NOW, OWNER, Scene};

async fn job_row(scene: &Scene, id: i64, todo: i64, unit: &str) {
  scene
    .run(
      r#"INSERT INTO "jobs" ("id", "chat_id", "unit", "launched_at", "prompt", "todo_id") VALUES ($1, $2, $3, $4, 'Work.', $5)"#,
      vec![id.into(), OWNER.into(), unit.into(), NOW.into(), todo.into()],
    )
    .await;
}

#[tokio::test]
async fn a_persons_words_are_told_to_bmo_and_a_jobs_on_its_own_item_are_the_items_record() {
  let scene = Scene::new().await;

  scene.item(1, "Fix it").await;
  scene.item(2, "Other").await;
  job_row(&scene, 5, 1, "run-5").await;

  let said = Append {
    text: "  Also the header. ",
    by: None,
  };

  assert_eq!(
    append(&scene.ground(), "1", &said).await.unwrap(),
    TodoAnswer::WordsAppendedTold {
      id: 1,
      title: "Fix it".into()
    }
  );
  assert_eq!(
    scene.hands.words(),
    [r#"{"code":"words_appended","id":1,"title":"Fix it","by":"Owner","job":5}"#]
  );

  let job = scene.job("run-5");

  assert_eq!(
    append(&job, "Fix it", &said).await.unwrap(),
    TodoAnswer::WordsAppendedKept {
      id: 1,
      title: "Fix it".into()
    }
  );
  assert_eq!(
    append(&job, "2", &said).await.unwrap(),
    TodoAnswer::WordsAppendedTold {
      id: 2,
      title: "Other".into()
    }
  );
  assert_eq!(scene.hands.words().len(), 2, "a job's words on its own item are not told");

  let read = notes(&scene.ground(), "1").await.unwrap();

  assert_eq!(
    read
      .standing
      .iter()
      .map(|note| (note.author.as_str(), note.text.as_str()))
      .collect::<Vec<_>>(),
    [("Owner", "Also the header."), ("Job 5", "Also the header."),]
  );

  let records = scene.units.records();

  assert_eq!(records.len(), 2, "a turn record is kept only where a run wrote");
  assert_eq!(
    (records[0].kind, records[0].record_id, records[0].run.as_str()),
    (TurnRecordKind::TodoAppend, Some(1), "run-5")
  );
  assert_eq!(
    scene.hands.moved()[1].1,
    Some(Written {
      todo: 1,
      texts: vec![],
      notes: vec![read.standing[1].id]
    })
  );
}

#[tokio::test]
async fn words_reach_no_finished_or_merged_item_and_are_never_empty() {
  let scene = Scene::new().await;

  scene.item(1, "Done").await;
  scene.item(2, "Target").await;
  scene.item(3, "Merged").await;
  scene.run(r#"UPDATE "todos" SET "done" = 1 WHERE "id" = 1"#, vec![]).await;
  scene.run(r#"UPDATE "todos" SET "merged_into" = 2 WHERE "id" = 3"#, vec![]).await;

  let ground = scene.ground();
  let said = Append {
    text: "More.",
    by: None,
  };
  let codes = [
    append(&ground, "1", &said).await.unwrap_err().code,
    append(&ground, "3", &said).await.unwrap_err().code,
    append(
      &ground,
      "2",
      &Append {
        text: " ",
        by: None,
      },
    )
    .await
    .unwrap_err()
    .code,
    append(&ground, "nothing", &said).await.unwrap_err().code,
  ];

  assert_eq!(codes, ["item_closed_already", "item_merged", "append_empty", "no_such_item"]);
}

#[tokio::test]
async fn the_brief_is_chat_bmos_alone_in_english_and_its_order_follows_it() {
  let scene = Scene::new().await;

  scene.item(1, "Fix it").await;
  scene
    .run(
      r#"INSERT INTO "work_orders" ("todo_id", "body") VALUES (1, '{"todoId":1,"instruction":"old"}')"#,
      vec![],
    )
    .await;

  let asked = Brief {
    brief: "Fix the launcher and its test.",
    change: "Added the test.",
  };

  assert_eq!(brief(&scene.ground(), "1", &asked).await.unwrap_err().code, "brief_agent_only");
  assert_eq!(brief(&scene.job("run"), "1", &asked).await.unwrap_err().code, "brief_agent_only");

  let chat = scene.chat();

  assert_eq!(
    brief(&chat, "1", &asked).await.unwrap(),
    TodoAnswer::BriefRewritten {
      id: 1,
      title: "Fix it".into(),
      job: None
    }
  );
  assert_eq!(
    scene.column::<Option<String>>(1, "brief").await.as_deref(),
    Some("Fix the launcher and its test.")
  );

  let order = queue_rows::work_orders::read(&scene.db, 1).await.unwrap().unwrap();

  assert_eq!(order["instruction"], "Fix the launcher and its test.");
  assert_eq!(scene.hands.moved().last().unwrap().0, Some(1));

  let korean = format!("{}{}{}{}", '\u{AC00}', '\u{AC01}', '\u{AC02}', '\u{AC03}');
  let refusals = [
    Brief {
      brief: "Whole.",
      change: " ",
    },
    Brief {
      brief: " ",
      change: "Changed.",
    },
    Brief {
      brief: &korean,
      change: "Changed.",
    },
  ];
  let codes: Vec<&str> =
    futures_util::future::join_all(refusals.iter().map(|asked| brief(&chat, "1", asked)))
      .await
      .into_iter()
      .map(|answered| answered.unwrap_err().code)
      .collect();

  assert_eq!(codes, ["brief_needs_change", "brief_empty", "brief_not_english"]);
}

#[tokio::test]
async fn words_are_taken_back_by_their_number_and_say_whether_a_job_had_them() {
  let scene = Scene::new().await;

  scene.item(1, "Fix it").await;

  let ground = scene.ground();

  append(
    &ground,
    "1",
    &Append {
      text: "First.",
      by: None,
    },
  )
  .await
  .unwrap();
  append(
    &ground,
    "1",
    &Append {
      text: "Second.",
      by: None,
    },
  )
  .await
  .unwrap();
  scene.run(r#"UPDATE "todo_notes" SET "delivered_at" = 1 WHERE "id" = 2"#, vec![]).await;

  assert_eq!(
    unappend(&ground, "1", Some(1), None).await.unwrap(),
    TodoAnswer::NoteRemoved {
      id: 1,
      title: "Fix it".into(),
      note: 1
    }
  );
  assert_eq!(
    unappend(&ground, "1", Some(2), Some("console")).await.unwrap(),
    TodoAnswer::NoteRemovedAfterDelivery {
      id: 1,
      title: "Fix it".into(),
      note: 2
    }
  );
  assert_eq!(unappend(&ground, "1", Some(1), None).await.unwrap_err().code, "no_such_note");
  assert_eq!(unappend(&ground, "1", None, None).await.unwrap_err().code, "note_needs_number");

  let read = notes(&ground, "1").await.unwrap();

  assert!(read.standing.is_empty());
  assert_eq!(
    read.withdrawn.iter().map(|note| note.removed_by.clone()).collect::<Vec<_>>(),
    [Some("Owner".to_owned()), Some("console".to_owned())]
  );
}

#[tokio::test]
async fn an_approval_is_recorded_with_its_hand_and_tells_the_queue() {
  let scene = Scene::new().await;

  scene.item(1, "Fix it").await;
  scene
    .run(r#"UPDATE "todos" SET "approved" = 0, "note" = 'kept' WHERE "id" = 1"#, vec![])
    .await;

  let ground = scene.ground();

  assert_eq!(
    approve(
      &ground,
      "1",
      &Approving {
        approve: true,
        note: None
      }
    )
    .await
    .unwrap(),
    TodoAnswer::ItemApproved {
      id: 1,
      noted: false
    }
  );
  assert_eq!(scene.column::<Option<String>>(1, "note").await.as_deref(), Some("kept"));
  assert_eq!(scene.hands.told(), ["approved false"]);
  assert_eq!(
    approve(
      &scene.job("run-7"),
      "1",
      &Approving {
        approve: false,
        note: None
      }
    )
    .await
    .unwrap(),
    TodoAnswer::ItemUnapproved { id: 1 }
  );
  assert_eq!(scene.column::<i64>(1, "approved").await, 0);
  assert_eq!(
    approve(
      &ground,
      "1",
      &Approving {
        approve: true,
        note: Some(" go ")
      }
    )
    .await
    .unwrap(),
    TodoAnswer::ItemApproved { id: 1, noted: true }
  );
  assert_eq!(scene.column::<Option<String>>(1, "note").await.as_deref(), Some("go"));

  let record = job_base::all(
    &scene.db,
    r#"SELECT "approved", "via", "who_kind", "who", "road" FROM "todo_approvals" WHERE "todo_id" = 1 ORDER BY "rowid""#,
    vec![],
  )
  .await
  .unwrap()
  .iter()
  .map(|row| {
    (
      row.try_get::<i64>("", "approved").unwrap(),
      row.try_get::<Option<String>>("", "via").unwrap(),
      row.try_get::<String>("", "who_kind").unwrap(),
      row.try_get::<String>("", "who").unwrap(),
      row.try_get::<String>("", "road").unwrap(),
    )
  })
  .collect::<Vec<_>>();

  assert_eq!(
    record[0],
    (1, Some("owner".to_owned()), "person".to_owned(), "person".to_owned(), "verb".to_owned())
  );
  assert_eq!(record[1], (0, None, "job".to_owned(), "run-7".to_owned(), "verb".to_owned()));
}

fn conversation() -> Filing {
  Filing {
    title: Some("Fix the launcher".into()),
    chat: Some(-200),
    what: Some("The launcher breaks.".into()),
    to_do: Some("Fix the launcher.".into()),
    conclusion: Some("It is a bug.".into()),
    instructions: vec![11],
    replies: vec![12],
    cowork: true,
    ..Filing::default()
  }
}

#[tokio::test]
async fn a_row_is_filed_out_of_a_conversation_its_messages_copied_in_order() {
  let scene = Scene::new().await;

  scene
    .units
    .said(-200, 11, Some(OWNER), "owner-login", "The launcher is broken again,\nplease look.");
  scene.units.said(-200, 12, Some(1), "agent-bot", "I will file it.");

  let job = Ground {
    run_name: Some("chat-run"),
    ..scene.chat()
  };
  let (id, answer) = add(&job, &conversation()).await.unwrap();

  assert_eq!(
    answer,
    TodoAnswer::ItemFiled {
      id,
      title: "Fix the launcher".into(),
      cowork: true,
      clip: "The launcher is broken again, please look.".into(),
    }
  );

  let instruction: String = scene.column(id, "instruction").await;

  assert!(
    instruction.contains("Instruction as said (Owner, 2026-09-20T11:08:00.000Z, message 11):"),
    "{instruction}"
  );
  assert!(
    instruction.contains("Reply as said (Agent, 2026-09-20T11:08:00.000Z, message 12):"),
    "{instruction}"
  );
  assert!(
    instruction.contains("What the work is (written by Agent as it filed the row):"),
    "{instruction}"
  );
  assert_eq!(scene.column::<Option<i64>>(id, "requester_id").await, Some(OWNER));
  assert_eq!(scene.column::<String>(id, "requester").await, "Owner");
  assert_eq!(scene.column::<String>(id, "status").await, "waiting");
  assert_eq!(scene.column::<Option<i64>>(id, "cowork").await, Some(1));
  assert_eq!(scene.column::<Option<String>>(id, "agent").await.as_deref(), Some("agent"));

  let order = queue_rows::work_orders::read(&scene.db, id).await.unwrap().expect("a draft is kept");

  assert_eq!(
    (order["status"].as_str(), order["instruction"].as_str()),
    (Some("draft"), Some("Fix the launcher."))
  );
  assert_eq!(scene.units.records()[0].kind, TurnRecordKind::TodoAdd);
}

#[tokio::test]
async fn a_filing_missing_its_parts_or_quoting_what_is_not_kept_or_worded_wrong_is_refused() {
  let mut scene = Scene::new().await;
  let empty = Filing {
    title: Some("T".into()),
    ..Filing::default()
  };

  let missing = add(&scene.ground(), &empty).await.unwrap_err();

  assert_eq!(missing.code, "filing_parts_missing");
  assert!(missing.error.contains("what, do, conclusion, instruction, reply"), "{}", missing.error);
  assert_eq!(add(&scene.ground(), &conversation()).await.unwrap_err().code, "message_not_archived");

  let model = Filing {
    model: Some("nobody::model".into()),
    ..conversation()
  };

  assert_eq!(add(&scene.ground(), &model).await.unwrap_err().code, "job_model_wrong");

  let unread = Filing {
    impact: Some("{".into()),
    ..conversation()
  };

  assert_eq!(add(&scene.ground(), &unread).await.unwrap_err().code, "impact_unread");

  scene.banned = vec![BannedTerm {
    lang: "en".into(),
    term: "launcher".into(),
    instead: "starter".into(),
    keeps: vec![],
  }];

  let banned = add(&scene.ground(), &conversation()).await.unwrap_err();

  assert_eq!((banned.code, banned.error.contains("launcher")), ("banned_terms", true));
}

#[tokio::test]
async fn a_row_is_raised_on_judgement_out_of_the_strands_it_names_and_bmo_is_told_it_is_its_own() {
  let scene = Scene::new().await;

  scene.item(1, "Source").await;
  scene.item(2, "Raiser").await;
  scene.run(r#"UPDATE "todos" SET "unfinished" = $1, "unfinished_resolved_at" = 5, "unfinished_resolution" = 'settled' WHERE "id" = 1"#, vec![
    "1. Agent share: Restart it.\n2. Agent share: Renew it.".into(),
  ])
  .await;

  let raised = Filing {
    title: Some("Renew the key".into()),
    judgement: Some("The key runs out.".into()),
    raised_by: Some("2".into()),
    from_todo: Some("1:2".into()),
    ..Filing::default()
  };
  let (id, answer) = add(&scene.job("run-9"), &raised).await.unwrap();

  assert_eq!(
    answer,
    TodoAnswer::ItemRaised {
      id,
      title: "Renew the key".into(),
      item: 2,
      sources: vec![StrandSource {
        id: 1,
        strand: "2".into()
      }],
      cowork: false,
      reopened: vec![1],
    }
  );

  let instruction: String = scene.column(id, "instruction").await;

  assert!(
    instruction.starts_with("Judgement (Agent, 2026-09-20T11:09:00.000Z, the work of item #2):"),
    "{instruction}"
  );
  assert!(
    instruction.contains("Source note (item #1 Source, strand 2):\n```text\nAgent share: Renew it.\n```"),
    "{instruction}"
  );
  assert_eq!(scene.column::<String>(id, "requester").await, "Agent");
  assert_eq!(scene.column::<Option<String>>(1, "unfinished_resolution").await, None);

  let orders = scene.units.instructions();
  let Said::Order(told) = &orders[0].said else {
    panic!("the agent is told with an order")
  };

  assert_eq!(
    (orders[0].sort.as_str(), orders[0].events.clone()),
    ("raised_row", vec!["queue_changed".to_owned()])
  );
  assert!(
    told.contains(&format!("raised row #{id} Renew the key, out of the Unfinished of #1.")),
    "{told}"
  );
  assert_eq!(
    scene.hands.moved().last().unwrap().1,
    Some(Written {
      todo: id,
      texts: vec!["title", "judgement"],
      notes: vec![]
    })
  );
}

#[tokio::test]
async fn a_raised_row_is_refused_where_its_provenance_does_not_read() {
  let scene = Scene::new().await;

  scene.item(1, "Source").await;
  scene.item(2, "Raiser").await;

  let base = Filing {
    title: Some("T".into()),
    judgement: Some("Why.".into()),
    raised_by: Some("2".into()),
    ..Filing::default()
  };
  let cases = [
    (
      Filing {
        what: Some("x".into()),
        ..base.clone()
      },
      "judgement_with_conversation",
    ),
    (
      Filing {
        judgement: Some(" ".into()),
        ..base.clone()
      },
      "judgement_empty",
    ),
    (
      Filing {
        title: None,
        ..base.clone()
      },
      "judgement_needs_title_and_room",
    ),
    (
      Filing {
        raised_by: None,
        ..base.clone()
      },
      "raised_by_needed",
    ),
    (
      Filing {
        raised_by: Some("77".into()),
        ..base.clone()
      },
      "raised_by_not_item",
    ),
    (
      Filing {
        from_todo: Some("1:x".into()),
        ..base.clone()
      },
      "from_todo_unread",
    ),
    (
      Filing {
        from_todo: Some("77:1".into()),
        ..base.clone()
      },
      "source_not_item",
    ),
    (
      Filing {
        from_todo: Some("1:1".into()),
        ..base.clone()
      },
      "source_left_nothing",
    ),
  ];

  for (filing, wanted) in cases {
    assert_eq!(add(&scene.ground(), &filing).await.unwrap_err().code, wanted, "{filing:?}");
  }

  let conversation = add(
    &scene.ground(),
    &Filing {
      replies: vec![1],
      ..base
    },
  )
  .await
  .unwrap_err();

  assert!(conversation.error.contains("reply"), "{}", conversation.error);
  assert_eq!(conversation.code, "judgement_with_conversation");
  assert_eq!(FilingPart::Reply.to_string(), "reply");
}

#[test]
fn a_job_model_is_auto_a_listed_providers_model_or_a_claude_model() {
  let providers = ["claude-code".to_owned(), "codex".to_owned()];

  for model in ["auto", "codex::gpt-5.1", "claude-opus-5", "claude-code::claude-opus-5"] {
    assert!(is_job_model(model, &providers), "{model}");
  }

  for model in ["", "gpt", "nobody::x", "codex::", "codex::-x", "claude-", "Codex::x"] {
    assert!(!is_job_model(model, &providers), "{model}");
  }
}

#[test]
fn the_strands_a_row_came_out_of_are_item_and_strand_each() {
  assert_eq!(strand_picks("3:2, 4"), [Some((3, Some(2))), Some((4, None))]);
  assert_eq!(strand_picks("3:0,x:1,3:"), [None, None, None]);
  assert!(strand_picks(" , ").is_empty());
}
