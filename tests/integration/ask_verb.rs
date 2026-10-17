//! A job's question to the owner (docs/verbs.md, "A job's question to the owner"): the refusals, the facts
//! the owner's chat is told at each step, and the wait itself on a stand of the test's own, the facts
//! queued through the bmo-receiver stand-in and the owner's answers, stops and talk written onto the row as
//! the server writes them.

use std::time::Duration;

use bmo_job::now_ms;
use bmo_job::queue::questions::{self, Answer as Settling, NewQuestion, Question};
use bmo_job::verbs::ask::{AskRequest, Asked, Asking, Revised, ask_refusal};
use job_codes::job_questions::{Lapse, QuestionFact};
use job_codes::{Fact, JobAskAnswer, JobAskRefusal};
use job_receiver::{Instruction, Said};
use queue_rows::wording::BannedTerm;
use serde_json::json;

use crate::support::desk::{Desk, OWNER_CHAT};

const WORDING: &str = "## Banned terms\n\n- en · blastoff → start\n";

fn options() -> Vec<String> {
  vec!["a table of its own".into(), "on the todos row".into()]
}

fn glossary() -> Vec<BannedTerm> {
  vec![BannedTerm {
    lang: "en".into(),
    term: "blastoff".into(),
    instead: "start".into(),
    keeps: vec![],
  }]
}

fn request() -> AskRequest {
  AskRequest {
    room: Some(OWNER_CHAT),
    todo_id: Some(12),
    question: "where should the questions be written?".into(),
    options: options(),
    recommended: Some(1),
    why: "it is hard to undo".into(),
  }
}

#[test]
fn a_question_with_nothing_to_choose_between_is_refused_before_the_row_is_written_or_the_chat_is_told() {
  let refused = |change: fn(&mut AskRequest)| {
    let mut asked = request();

    change(&mut asked);
    ask_refusal(&asked, OWNER_CHAT, &glossary())
  };
  let banned = Some(JobAskRefusal::QuestionBannedTerms {
    terms: vec!["blastoff".into()],
  });

  assert_eq!(ask_refusal(&request(), OWNER_CHAT, &glossary()), None);
  assert_eq!(refused(|asked| asked.room = None), None, "a question naming no room is the owner's");
  assert_eq!(refused(|asked| asked.todo_id = None), Some(JobAskRefusal::QuestionItem));
  assert_eq!(
    refused(|asked| asked.options = vec!["only one".into()]),
    Some(JobAskRefusal::QuestionOptions)
  );

  for recommended in [Some(3), Some(0), None] {
    let mut asked = request();

    asked.recommended = recommended;
    assert_eq!(
      ask_refusal(&asked, OWNER_CHAT, &glossary()),
      Some(JobAskRefusal::QuestionRecommend { count: 2 })
    );
  }

  assert_eq!(refused(|asked| asked.why = "  ".into()), Some(JobAskRefusal::QuestionWhy));
  assert_eq!(refused(|asked| asked.question = " ".into()), Some(JobAskRefusal::QuestionText));
  assert_eq!(
    refused(|asked| asked.options[1] = "two\nlines".into()),
    Some(JobAskRefusal::QuestionOptionEmpty)
  );
  // A question put anywhere but the owner's private chat.
  assert_eq!(refused(|asked| asked.room = Some(-500)), Some(JobAskRefusal::QuestionRoom));
  // A question that hands the owner a page hosted outside the deployment's own archive.
  assert_eq!(
    refused(|asked| asked.question = "the designs at https://claude.ai/artifact/abc, which one?".into()),
    Some(JobAskRefusal::QuestionOutsideHosting {
      addresses: vec!["https://claude.ai/artifact/abc".into()]
    })
  );
  // A question, an option or a why using a word the owner set aside.
  assert_eq!(refused(|asked| asked.question = "which blastoff first?".into()), banned);
  assert_eq!(refused(|asked| asked.options[0] = "blastoff now".into()), banned);
  assert_eq!(refused(|asked| asked.why = "the blastoff is hard to undo".into()), banned);
  assert_eq!(refused(|asked| asked.why = "the `blastoff` is quoted".into()), None);
}

fn question(over: impl FnOnce(&mut Question)) -> Question {
  let mut question = Question {
    id: 1,
    job_id: None,
    todo_id: 12,
    chat_id: OWNER_CHAT,
    question: "where should the questions be written?".into(),
    options: options(),
    recommended: 0,
    why: "it is hard to undo".into(),
    asked_at: 1_756_800_000_000,
    due_at: 1_756_800_060_000,
    chosen: None,
    answer: None,
    paused_left: None,
    settled_at: None,
    settled_by: None,
    talks: false,
  };

  over(&mut question);
  question
}

#[test]
fn the_question_as_the_chat_is_told_it_carries_the_options_the_recommendation_and_the_wait() {
  assert_eq!(
    question(|q| q.recommended = 1).asked(None),
    QuestionFact::QuestionAsked {
      item: 12,
      question_id: 1,
      question: "where should the questions be written?".into(),
      options: options(),
      recommended: 2,
      why: "it is hard to undo".into(),
      wait_seconds: 60,
      talk: false,
      removed_room: None,
    }
  );
  assert!(matches!(
    question(|q| q.talks = true).asked(None),
    QuestionFact::QuestionAsked { talk: true, .. }
  ));
}

#[test]
fn the_deadline_s_fact_says_which_silence_ran_out_and_the_recommendation_stood() {
  let stop_ms = 1_800_000;
  let deadline = question(|q| {
    q.chosen = Some(0);
    q.settled_by = Some("deadline".into());
  });
  let stopped = question(|q| {
    q.chosen = Some(0);
    q.paused_left = Some(40_000);
    q.settled_by = Some("deadline".into());
  });
  let lapsed = |lapsed: Lapse, wait_seconds: u64| QuestionFact::QuestionLapsed {
    item: 12,
    lapsed,
    option: 1,
    named: "a table of its own".into(),
    wait_seconds,
  };

  assert_eq!(deadline.settled(false, stop_ms), lapsed(Lapse::Waiting, 60));
  assert_eq!(stopped.settled(true, stop_ms), lapsed(Lapse::Talking, 1_800));
  assert_eq!(stopped.settled(false, stop_ms), lapsed(Lapse::Stopped, 1_800));
}

#[test]
fn the_owner_s_own_choice_or_words_are_read_back_as_they_landed() {
  let chosen = question(|q| {
    q.chosen = Some(1);
    q.settled_by = Some("owner".into());
  });
  let typed = question(|q| {
    q.answer = Some("neither, one per room".into());
    q.settled_by = Some("owner".into());
  });

  assert_eq!(
    chosen.settled(false, 0),
    QuestionFact::QuestionChosen {
      item: 12,
      option: 2,
      named: "on the todos row".into(),
    }
  );
  assert_eq!(chosen.answer_text(), "on the todos row");
  assert_eq!(
    typed.settled(false, 0),
    QuestionFact::QuestionTyped {
      item: 12,
      answer: "neither, one per room".into(),
    }
  );
  assert_eq!(typed.answer_text(), "neither, one per room");
}

async fn desk() -> Desk {
  let desk = Desk::worded(WORDING).await;

  desk.receiver.answer("POST /instructions", 200, json!({ "id": 1 }));
  desk
}

// The facts the owner's chat was told, in order.
fn told(desk: &Desk) -> Vec<QuestionFact> {
  desk
    .receiver
    .calls()
    .into_iter()
    .filter(|call| call.route == "POST /instructions")
    .filter_map(|call| serde_json::from_value::<Instruction>(call.body?).ok())
    .filter(|told| told.chat == Some(OWNER_CHAT))
    .filter_map(|told| match told.said {
      Said::Fact(Fact::Question(fact)) => Some(fact),
      _ => None,
    })
    .collect()
}

// The question the asking opened, once it stands open.
async fn opened(desk: &Desk) -> Question {
  for _ in 0..400 {
    let id = desk
      .count(
        r#"SELECT COALESCE(MAX("id"), 0) FROM "job_questions" WHERE "settled_at" IS NULL"#,
        vec![],
      )
      .await;

    if id > 0 {
      return questions::question(&desk.stand.db, id).await.unwrap().unwrap();
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  panic!("the asking never opened a question");
}

async fn last(desk: &Desk) -> Question {
  let id = desk.count(r#"SELECT MAX("id") FROM "job_questions""#, vec![]).await;

  questions::question(&desk.stand.db, id).await.unwrap().unwrap()
}

async fn settle(desk: &Desk, id: i64, settling: Settling) {
  assert!(questions::settle(&desk.stand.db, id, &settling, now_ms()).await.unwrap());
}

// The owner's line of talk over the question, as the server writes it.
async fn talk(desk: &Desk, id: i64) {
  let hold = desk.stand.policy.queue.question_stop_seconds * 1000;

  assert!(
    questions::talk(&desk.stand.db, id, "owner", "what is the difference between 1 and 2?", hold, now_ms())
      .await
      .unwrap()
  );
}

// Waits until the job's own line of talk has landed on the question.
async fn job_talked(desk: &Desk, id: i64) {
  for _ in 0..400 {
    if desk
      .count(
        r#"SELECT COUNT(*) FROM "question_talk" WHERE "question_id" = $1 AND "by" = 'job'"#,
        vec![id.into()],
      )
      .await
      > 0
    {
      return;
    }

    tokio::time::sleep(Duration::from_millis(25)).await;
  }

  panic!("the job's line never landed");
}

fn settled(asked: Asked) -> String {
  match asked {
    Asked::Settled(line) => line,
    other => panic!("not settled: {other:?}"),
  }
}

#[tokio::test(flavor = "multi_thread")]
async fn carries_on_with_the_option_the_owner_picked_and_tells_the_chat_it_landed() {
  let desk = desk().await;
  let asking = Asking { stand: &desk.stand };
  let asked = request();
  let (answer, ()) = tokio::join!(asking.ask(&asked, 60_000), async {
    let row = opened(&desk).await;

    settle(&desk, row.id, Settling::Chosen(1)).await;
  });
  let row = last(&desk).await;

  assert_eq!(settled(answer.unwrap()), "on the todos row");
  assert_eq!(
    (row.settled_by.as_deref(), row.chosen, row.chat_id, row.talks),
    (Some("owner"), Some(1), OWNER_CHAT, true)
  );

  let told = told(&desk);

  assert!(
    matches!(&told[0], QuestionFact::QuestionAsked { question, recommended: 1, .. } if question == "where should the questions be written?")
  );
  assert_eq!(
    told[1],
    QuestionFact::QuestionChosen {
      item: 12,
      option: 2,
      named: "on the todos row".into(),
    }
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_refused_question_writes_no_row_and_tells_nobody() {
  let desk = desk().await;
  let asking = Asking { stand: &desk.stand };
  let refused = asking
    .ask(
      &AskRequest {
        question: "which blastoff first?".into(),
        ..request()
      },
      60_000,
    )
    .await
    .unwrap_err();

  assert_eq!(refused.code, "question_banned_terms");
  assert_eq!(desk.count(r#"SELECT COUNT(*) FROM "job_questions""#, vec![]).await, 0);
  assert!(told(&desk).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn carries_on_with_the_words_the_owner_wrote_where_the_options_covered_none_of_it() {
  let desk = desk().await;
  let asking = Asking { stand: &desk.stand };
  let asked = request();
  let (answer, ()) = tokio::join!(asking.ask(&asked, 60_000), async {
    let row = opened(&desk).await;

    settle(&desk, row.id, Settling::Words("neither, one per room".into())).await;
  });
  let row = last(&desk).await;

  assert_eq!(settled(answer.unwrap()), "neither, one per room");
  assert_eq!((row.chosen, row.settled_by.as_deref()), (None, Some("owner")));
  assert!(told(&desk).contains(&QuestionFact::QuestionTyped {
    item: 12,
    answer: "neither, one per room".into(),
  }));
}

#[tokio::test(flavor = "multi_thread")]
async fn keeps_waiting_past_its_wait_while_the_owner_has_the_clock_stopped_and_still_takes_the_number() {
  let desk = desk().await;
  let asking = Asking { stand: &desk.stand };
  let stop_ms = desk.stand.policy.queue.question_stop_seconds * 1000;
  let asked = request();
  let (answer, ()) = tokio::join!(asking.ask(&asked, 2_000), async {
    let row = opened(&desk).await;

    assert!(questions::pause(&desk.stand.db, row.id, stop_ms, now_ms()).await.unwrap());
    // Past the asker's own two-second wait, nothing settles it.
    tokio::time::sleep(Duration::from_millis(2_500)).await;
    assert_eq!(questions::question(&desk.stand.db, row.id).await.unwrap().unwrap().settled_at, None);
    settle(&desk, row.id, Settling::Chosen(1)).await;
  });

  assert_eq!(settled(answer.unwrap()), "on the todos row");
  assert_eq!(last(&desk).await.settled_by.as_deref(), Some("owner"));
}

#[tokio::test(flavor = "multi_thread")]
async fn goes_with_the_recommendation_when_nobody_answers_and_says_so_in_the_chat() {
  let desk = desk().await;
  let answer = Asking { stand: &desk.stand }.ask(&request(), 1_000).await.unwrap();
  let row = last(&desk).await;

  assert_eq!(settled(answer), "a table of its own");
  assert_eq!((row.settled_by.as_deref(), row.chosen), (Some("deadline"), Some(0)));
  assert_eq!(
    told(&desk).last(),
    Some(&QuestionFact::QuestionLapsed {
      item: 12,
      lapsed: Lapse::Waiting,
      option: 1,
      named: "a table of its own".into(),
      wait_seconds: 1,
    })
  );
}

#[tokio::test(flavor = "multi_thread")]
async fn hands_the_job_the_owner_s_talk_with_the_clock_stopped_and_carries_on_with_the_number_then_picked()
{
  let desk = desk().await;
  let asking = Asking { stand: &desk.stand };
  let asked = request();
  let (heard, ()) = tokio::join!(asking.ask(&asked, 30_000), async {
    let row = opened(&desk).await;

    talk(&desk, row.id).await;
  });
  let row = last(&desk).await;

  assert_eq!(
    heard.unwrap(),
    Asked::Talked(JobAskAnswer::OwnerTalked {
      id: row.id,
      heard: vec!["what is the difference between 1 and 2?".into()],
    })
  );
  // A half-minute wait, stopped for the talk: nothing settled it, and it is due a hold on.
  assert_eq!(row.settled_at, None);
  assert!(row.due_at >= now_ms() + desk.stand.policy.queue.question_stop_seconds * 1000 - 100_000);

  let explanation = "1 keeps its own table, 2 writes onto the item's row";
  let (answer, ()) = tokio::join!(asking.talk(Some(row.id), explanation, None), async {
    job_talked(&desk, row.id).await;
    settle(&desk, row.id, Settling::Chosen(1)).await;
  });
  let told = told(&desk);

  assert_eq!(settled(answer.unwrap()), "on the todos row");
  assert!(told.contains(&QuestionFact::QuestionTalkHeld { item: 12 }));
  assert!(told.contains(&QuestionFact::JobReplied {
    item: 12,
    said: explanation.into(),
  }));
  assert!(matches!(told.last(), Some(QuestionFact::QuestionChosen { option: 2, .. })));
}

#[tokio::test(flavor = "multi_thread")]
async fn puts_the_changed_options_to_the_owner_again_where_the_talk_changed_the_choice() {
  let desk = desk().await;
  let asking = Asking { stand: &desk.stand };
  let asked = request();
  let (heard, ()) = tokio::join!(asking.ask(&asked, 30_000), async {
    let row = opened(&desk).await;

    talk(&desk, row.id).await;
  });

  assert!(matches!(heard.unwrap(), Asked::Talked(_)));

  let row = last(&desk).await;
  let revised = Revised {
    question: None,
    options: vec!["the hook alone".into(), "the hook and the gateway".into()],
    recommended: Some(2),
    why: None,
  };
  let (answer, ()) = tokio::join!(
    asking.talk(Some(row.id), "they can, so the options become these", Some(revised)),
    async {
      job_talked(&desk, row.id).await;
      settle(&desk, row.id, Settling::Chosen(0)).await;
    }
  );

  assert_eq!(settled(answer.unwrap()), "the hook alone");
  assert!(told(&desk).iter().any(|fact| matches!(
    fact,
    QuestionFact::QuestionRevised { options, recommended: 2, .. } if options[1] == "the hook and the gateway"
  )));
}

#[tokio::test(flavor = "multi_thread")]
async fn a_talk_names_an_open_question_that_takes_talk_and_says_something() {
  let desk = desk().await;
  let asking = Asking { stand: &desk.stand };
  let options = options();
  let open = |talks: bool| NewQuestion {
    job_id: None,
    todo_id: 12,
    chat_id: OWNER_CHAT,
    question: "which one?",
    options: &options,
    recommended: 0,
    why: "it is hard to undo",
    asked_at: 1,
    due_at: 61_000,
    talks,
  };
  let confirmation = questions::open(&desk.stand.db, &open(false)).await.unwrap();
  let talking = questions::open(&desk.stand.db, &open(true)).await.unwrap();
  let code = |refused: Result<Asked, job_base::Refusal>| refused.unwrap_err().code;

  assert_eq!(code(asking.talk(None, "x", None).await), "question_talk_number");
  assert_eq!(code(asking.talk(Some(99), "x", None).await), "question_absent");
  assert_eq!(code(asking.talk(Some(confirmation.id), "x", None).await), "question_takes_no_talk");
  assert_eq!(code(asking.talk(Some(talking.id), " ", None).await), "question_talk_say");
  assert_eq!(
    code(asking.talk(Some(talking.id), "the blastoff comes first", None).await),
    "question_banned_terms"
  );
  assert_eq!(desk.count(r#"SELECT COUNT(*) FROM "question_talk""#, vec![]).await, 0);
}
