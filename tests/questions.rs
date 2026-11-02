//! Questions, their clocks and their replies, through the real server (docs/api.md, "Questions and replies").

use crate::support;

use bmo_job_client::{
  EventQuery, NewItem, NewQuestion, NewReply, QuestionQuery, QuestionRecommendation, QuestionState,
  ReplySource, WaitQuery,
};

use support::{START, Service};

fn reply(choice: Option<&str>, text: &str) -> NewReply {
  NewReply {
    choice: choice.map(str::to_owned),
    text: text.to_owned(),
    actor_name: "decider".to_owned(),
    settles: None,
  }
}

#[tokio::test]
async fn a_question_with_choices_is_settled_by_a_choice_and_takes_words_beside() {
  let service = Service::start().await;
  let client = service.client();
  let mut new = NewItem {
    title: "parser".into(),
    queue_name: "build".into(),
    ..NewItem::default()
  };
  new.scope = Some("project-7".into());
  let item = client.create_item(&new).await.expect("raised");
  let question = client
    .ask(&NewQuestion {
      item_id: Some(item.id),
      asker_name: "builder".into(),
      text: "Which grammar?".into(),
      choices: vec!["strict".into(), "loose".into()],
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  assert_eq!((question.state, question.scope.as_deref()), (QuestionState::Open, Some("project-7")));

  let word = client.reply(question.id, &reply(None, "looking into it")).await.expect("a word");
  assert!(!word.settles);
  assert_eq!(
    client
      .reply(question.id, &reply(Some("neither"), "x"))
      .await
      .expect_err("no such")
      .code(),
    Some("not-a-choice")
  );
  let answer = client
    .reply(question.id, &reply(Some("strict"), "strict, please"))
    .await
    .expect("settled");
  assert!(answer.settles);
  assert_eq!(client.question(question.id).await.expect("read").state, QuestionState::Answered);
  assert_eq!(
    client
      .reply(question.id, &reply(Some("loose"), "x"))
      .await
      .expect_err("answered")
      .code(),
    Some("conflict")
  );
  client.reply(question.id, &reply(None, "thanks")).await.expect("a word after");

  let replies = client.replies(question.id, &WaitQuery::default()).await.expect("read");
  assert_eq!(replies.items.len(), 3);
  let mine = QuestionQuery {
    item_id: Some(item.id),
    state: Some("answered".into()),
    ..QuestionQuery::default()
  };
  assert_eq!(client.questions(&mine).await.expect("listed").items.len(), 1);
  let unknown = NewQuestion {
    item_id: Some(9_999),
    asker_name: "x".into(),
    text: "y".into(),
    ..NewQuestion::default()
  };
  assert_eq!(client.ask(&unknown).await.expect_err("no item").code(), Some("unknown-item"));
}

#[tokio::test]
async fn the_clock_pauses_resumes_and_tells_once_when_due() {
  let service = Service::start().await;
  let client = service.client();
  let question = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "May I go ahead?".into(),
      wait_ms: Some(10_000),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  let due = EventQuery {
    kinds: Some("question.due".into()),
    ..EventQuery::default()
  };

  service.pass(4_000).await;
  let paused = client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        state: QuestionState::Paused,
      },
    )
    .await
    .expect("paused");
  assert_eq!(paused.due_at, None);
  service.pass(100_000).await;
  assert!(
    client.events_page(&due).await.expect("read").items.is_empty(),
    "a paused clock does not run"
  );
  let open = client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        state: QuestionState::Open,
      },
    )
    .await
    .expect("open again");
  assert_eq!(open.due_at, Some(START + 104_000 + 6_000));

  service.pass(6_000).await;
  service.pass(1_000).await;
  let told = client.events_page(&due).await.expect("read").items;
  assert_eq!(told.len(), 1);
  assert_eq!(client.question(question.id).await.expect("read").state, QuestionState::Open);

  let withdrawn = client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        state: QuestionState::Withdrawn,
      },
    )
    .await
    .expect("withdrawn");
  assert_eq!(withdrawn.state, QuestionState::Withdrawn);
  let back = client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        state: QuestionState::Open,
      },
    )
    .await;
  assert_eq!(back.expect_err("withdrawn is final").code(), Some("conflict"));
}

#[tokio::test]
async fn an_open_question_is_settled_by_its_first_reply_and_a_wait_ends_on_it() {
  let service = Service::start().await;
  let client = service.client();
  let question = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "What should it be called?".into(),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  let waiting = {
    let client = client.clone();
    tokio::spawn(async move {
      client
        .replies(
          question.id,
          &WaitQuery {
            wait_ms: Some(60_000),
            ..WaitQuery::default()
          },
        )
        .await
    })
  };
  client.reply(question.id, &reply(None, "call it reader")).await.expect("answered");
  let replies = waiting.await.expect("the wait ends").expect("read");
  assert!(replies.items[0].settles);
  assert_eq!(client.question(question.id).await.expect("read").state, QuestionState::Answered);
}

#[tokio::test]
async fn the_wait_defaults_to_a_minute_and_the_asker_can_set_another() {
  let service = Service::start().await;
  let client = service.client();
  let by_default = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "May I?".into(),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  assert_eq!(by_default.due_at, Some(START + 60_000));

  let longer = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "May I?".into(),
      wait_ms: Some(120_000),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  assert_eq!(longer.due_at, Some(START + 120_000));

  let too_short = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "May I?".into(),
      wait_ms: Some(999),
      ..NewQuestion::default()
    })
    .await
    .expect_err("too short");
  assert_eq!(too_short.code(), Some("bad-request"));
}

#[tokio::test]
async fn a_due_question_with_a_recommendation_stays_open_for_a_person_and_tells_once() {
  let service = Service::start().await;
  let client = service.client();
  let question = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "Which grammar?".into(),
      choices: vec!["strict".into(), "loose".into()],
      recommendation: Some(QuestionRecommendation {
        answer: "strict".into(),
        why: "A loose grammar reads a stray token as a rule.".into(),
      }),
      wait_ms: Some(1_000),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  assert_eq!(question.recommendation.as_ref().map(|one| one.answer.as_str()), Some("strict"));

  service.pass(1_000).await;
  assert_eq!(client.question(question.id).await.expect("read").state, QuestionState::Open);
  assert!(
    client
      .replies(question.id, &WaitQuery::default())
      .await
      .expect("replies")
      .items
      .is_empty()
  );
  let due = EventQuery {
    kinds: Some("question.due".into()),
    ..EventQuery::default()
  };
  assert_eq!(client.events_page(&due).await.expect("events").items.len(), 1);

  let answer = client
    .reply(question.id, &reply(Some("strict"), "strict, as recommended"))
    .await
    .expect("answered");
  assert_eq!((answer.source, answer.why.as_deref()), (ReplySource::Person, None));
  assert_eq!(client.question(question.id).await.expect("read").state, QuestionState::Answered);
}

#[tokio::test]
async fn a_due_question_without_a_recommendation_stays_open_and_tells_once() {
  let service = Service::start().await;
  let client = service.client();
  let question = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "May I?".into(),
      wait_ms: Some(1_000),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");

  service.pass(1_000).await;
  assert_eq!(client.question(question.id).await.expect("read").state, QuestionState::Open);
  assert!(
    client
      .replies(question.id, &WaitQuery::default())
      .await
      .expect("replies")
      .items
      .is_empty()
  );
  let due = EventQuery {
    kinds: Some("question.due".into()),
    ..EventQuery::default()
  };
  assert_eq!(client.events_page(&due).await.expect("events").items.len(), 1);

  service.pass(1_000).await;
  assert_eq!(client.events_page(&due).await.expect("events").items.len(), 1);
}

#[tokio::test]
async fn a_persons_reply_before_the_due_time_wins() {
  let service = Service::start().await;
  let client = service.client();
  let question = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "Which grammar?".into(),
      choices: vec!["strict".into(), "loose".into()],
      recommendation: Some(QuestionRecommendation {
        answer: "strict".into(),
        why: "A loose grammar reads a stray token as a rule.".into(),
      }),
      wait_ms: Some(1_000),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");

  let answer = client
    .reply(question.id, &reply(Some("loose"), "loose, please"))
    .await
    .expect("answered");
  assert_eq!((answer.source, answer.why.as_deref()), (ReplySource::Person, None));

  service.pass(1_000).await;
  assert_eq!(client.question(question.id).await.expect("read").state, QuestionState::Answered);
  let replies = client.replies(question.id, &WaitQuery::default()).await.expect("replies");
  assert_eq!(replies.items.len(), 1);
  assert_eq!(replies.items[0].source, ReplySource::Person);
  assert_eq!(replies.items[0].choice.as_deref(), Some("loose"));
}

#[tokio::test]
async fn a_recommendation_carries_an_answer_and_a_reason() {
  let service = Service::start().await;
  let client = service.client();
  let no_answer = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "Which?".into(),
      choices: vec!["a".into()],
      recommendation: Some(QuestionRecommendation {
        answer: " ".into(),
        why: "because".into(),
      }),
      ..NewQuestion::default()
    })
    .await
    .expect_err("no answer");
  assert_eq!(no_answer.code(), Some("bad-request"));

  let no_reason = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "Which?".into(),
      choices: vec!["a".into()],
      recommendation: Some(QuestionRecommendation {
        answer: "a".into(),
        why: " ".into(),
      }),
      ..NewQuestion::default()
    })
    .await
    .expect_err("no reason");
  assert_eq!(no_reason.code(), Some("bad-request"));
}
