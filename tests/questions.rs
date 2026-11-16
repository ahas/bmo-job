//! Questions, their clocks and their replies, through the real server (docs/api.md, "Questions and replies").

use crate::support;

use bmo_job_client::{
  EventQuery, NewItem, NewQuestion, NewReply, QuestionAddressee, QuestionChange, QuestionQuery,
  QuestionRecommendation, QuestionState, ReplySource, WaitQuery,
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
        state: Some(QuestionState::Paused),
        ..Default::default()
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
        state: Some(QuestionState::Open),
        ..Default::default()
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
        state: Some(QuestionState::Withdrawn),
        ..Default::default()
      },
    )
    .await
    .expect("withdrawn");
  assert_eq!(withdrawn.state, QuestionState::Withdrawn);
  let back = client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        state: Some(QuestionState::Open),
        ..Default::default()
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

#[tokio::test]
async fn a_question_closed_unsent_takes_no_answer_and_stands_with_nothing_waiting() {
  let service = Service::start().await;
  let client = service.client();
  let question = client
    .ask(&NewQuestion {
      asker_name: "job-1".into(),
      scope: Some("7".into()),
      text: "which grammar?".into(),
      choices: vec!["strict".into(), "loose".into()],
      told_by_asker: true,
      ..NewQuestion::default()
    })
    .await
    .expect("asked");

  assert!(question.told_by_asker);

  let unsent = client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        state: Some(QuestionState::Unsent),
        ..Default::default()
      },
    )
    .await
    .expect("closed");

  assert_eq!(unsent.state, QuestionState::Unsent);
  client
    .reply(question.id, &reply(Some("strict"), "strict"))
    .await
    .expect_err("a question that reached nobody is never answered");

  let waiting = service.raw(reqwest::Method::GET, "/questions/open?chat=7").send().await.expect("read");
  let waiting: serde_json::Value = waiting.json().await.expect("the room's questions");

  assert_eq!(waiting, serde_json::json!([]));
}

#[tokio::test]
async fn a_question_is_put_again_in_other_words_and_keeps_what_its_asker_was_handed() {
  let service = Service::start().await;
  let client = service.client();
  let question = client
    .ask(&NewQuestion {
      asker_name: "job-1".into(),
      text: "which grammar?".into(),
      choices: vec!["strict".into(), "loose".into()],
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  let again = client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        text: Some("which parser?".into()),
        choices: Some(vec!["pest".into(), "nom".into()]),
        recommendation: Some(QuestionRecommendation {
          answer: "nom".into(),
          why: "No generated code.".into(),
        }),
        ..Default::default()
      },
    )
    .await
    .expect("put again");

  assert_eq!(again.text, "which parser?");
  assert_eq!(again.choices, ["pest", "nom"]);
  assert_eq!(again.state, QuestionState::Open);

  for (heard, stands) in [(5, 5), (3, 5), (9, 9)] {
    let moved = client
      .change_question(
        question.id,
        &bmo_job_client::QuestionChange {
          heard_through: Some(heard),
          ..Default::default()
        },
      )
      .await
      .expect("moved");

    assert_eq!(moved.heard_through, Some(stands), "it never moves back");
  }

  client
    .change_question(question.id, &bmo_job_client::QuestionChange::default())
    .await
    .expect_err("a change naming nothing is refused");
  client.reply(question.id, &reply(Some("nom"), "nom")).await.expect("answered");
  client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        text: Some("too late".into()),
        ..Default::default()
      },
    )
    .await
    .expect_err("a settled question is not put again");
}

#[tokio::test]
async fn an_asker_tracking_what_it_heard_does_not_tell_the_due_time_again() {
  let service = Service::start().await;
  let client = service.client();
  let question = client
    .ask(&NewQuestion {
      asker_name: "job-1".into(),
      text: "which grammar?".into(),
      choices: vec!["strict".into(), "loose".into()],
      wait_ms: Some(1_000),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");

  service.pass(1_000).await;
  client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        heard_through: Some(1),
        ..Default::default()
      },
    )
    .await
    .expect("moved");
  service.pass(1_000).await;

  let told = client
    .events_page(&EventQuery {
      kinds: Some("question.due".into()),
      ..EventQuery::default()
    })
    .await
    .expect("the events");

  assert_eq!(told.items.len(), 1, "the due time is told once");
}

fn put_to_the_supervisor(item_id: Option<i64>) -> NewQuestion {
  NewQuestion {
    item_id,
    asker_name: "builder".into(),
    addressee: Some(QuestionAddressee::Supervisor),
    text: "Run the read-only probe on the build node.".into(),
    wait_ms: Some(30_000),
    ..NewQuestion::default()
  }
}

fn settling(text: &str) -> NewReply {
  NewReply {
    choice: None,
    text: text.to_owned(),
    actor_name: "lead".to_owned(),
    settles: Some(true),
  }
}

#[tokio::test]
async fn a_question_is_a_persons_where_the_ask_names_no_addressee() {
  let service = Service::start().await;
  let question = service
    .client()
    .ask(&NewQuestion {
      asker_name: "rehearsal".into(),
      text: "Which way?".into(),
      choices: vec!["one".into(), "two".into()],
      ..NewQuestion::default()
    })
    .await
    .expect("asked");

  assert_eq!(question.addressee, QuestionAddressee::Person);
  assert_eq!(question.due_at, Some(START + 60_000), "a person's clock runs from the ask");
  assert_eq!((question.readdressed_by, question.readdressed_at), (None, None));
}

#[tokio::test]
async fn a_question_the_supervisor_holds_has_no_clock_and_is_never_told_due() {
  let service = Service::start().await;
  let question = service.client().ask(&put_to_the_supervisor(None)).await.expect("asked");
  assert_eq!((question.addressee, question.due_at), (QuestionAddressee::Supervisor, None));

  service.pass(86_400_000).await;
  let due = EventQuery {
    kinds: Some("question.due".into()),
    ..EventQuery::default()
  };
  assert!(service.client().events_page(&due).await.expect("read").items.is_empty());
}

#[tokio::test]
async fn the_list_filters_by_addressee() {
  let service = Service::start().await;
  let client = service.client();
  let held = client.ask(&put_to_the_supervisor(None)).await.expect("asked");
  let owned = client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      text: "Which way?".into(),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  let listed = |addressee: &str| QuestionQuery {
    addressee: Some(addressee.to_owned()),
    ..QuestionQuery::default()
  };
  let ids = |page: bmo_job_client::Page<bmo_job_client::Question>| {
    page.items.iter().map(|q| q.id).collect::<Vec<_>>()
  };

  assert_eq!(ids(client.questions(&listed("person")).await.expect("read")), [owned.id]);
  assert_eq!(ids(client.questions(&listed("supervisor")).await.expect("read")), [held.id]);
  assert_eq!(
    ids(client.questions(&listed("person,supervisor")).await.expect("read")),
    [held.id, owned.id]
  );
  let refused = client
    .questions(&listed("nobody"))
    .await
    .expect_err("an addressee the service does not know");
  assert_eq!(refused.code(), Some("bad-request"));
}

#[tokio::test]
async fn only_the_supervising_caller_settles_a_question_the_supervisor_holds() {
  let service = Service::start().await;
  let question = service.client().ask(&put_to_the_supervisor(None)).await.expect("asked");

  for (who, client) in [("the shared token", service.client()), ("an approver", service.approver())] {
    let refused = client.reply(question.id, &settling("done")).await.expect_err(who);
    assert_eq!(refused.code(), Some("not-the-addressee"), "{who}");
  }
  let word = service
    .client()
    .reply(
      question.id,
      &NewReply {
        settles: Some(false),
        ..reply(None, "which node?")
      },
    )
    .await
    .expect("a word from anyone");
  assert!(!word.settles);

  let settled = service
    .supervisor()
    .reply(question.id, &settling("probe ran: all well"))
    .await
    .expect("settled");
  assert!(settled.settles);
  assert_eq!(
    service.client().question(question.id).await.expect("read").state,
    QuestionState::Answered
  );
}

#[tokio::test]
async fn the_supervisor_puts_its_question_to_a_person_in_its_own_words_and_the_clock_starts_then() {
  let service = Service::start().await;
  let question = service.client().ask(&put_to_the_supervisor(None)).await.expect("asked");
  let to_person = QuestionChange {
    addressee: Some(QuestionAddressee::Person),
    text: Some("Deploy the new unit tonight?".into()),
    choices: Some(vec!["tonight".into(), "tomorrow".into()]),
    recommendation: Some(QuestionRecommendation {
      answer: "tomorrow".into(),
      why: "nothing waits on it tonight".into(),
    }),
    ..QuestionChange::default()
  };

  let refused = service
    .approver()
    .change_question(question.id, &to_person)
    .await
    .expect_err("no supervisor");
  assert_eq!(refused.code(), Some("not-the-addressee"));

  service.pass(5_000).await;
  let put = service
    .supervisor()
    .change_question(question.id, &to_person)
    .await
    .expect("put to a person");
  assert_eq!(put.addressee, QuestionAddressee::Person);
  assert_eq!((put.readdressed_by.as_deref(), put.readdressed_at), (Some("lead"), Some(START + 5_000)));
  assert_eq!(put.due_at, Some(START + 5_000 + 30_000), "the clock takes the ask's wait from the move");
  assert_eq!((put.text.as_str(), put.choices.len()), ("Deploy the new unit tonight?", 2));

  let back = QuestionChange {
    addressee: Some(QuestionAddressee::Supervisor),
    ..QuestionChange::default()
  };
  let refused = service.supervisor().change_question(question.id, &back).await.expect_err("never back");
  assert_eq!(refused.code(), Some("conflict"));
  let again = service
    .supervisor()
    .change_question(question.id, &to_person)
    .await
    .expect_err("already a person's");
  assert_eq!(again.code(), Some("conflict"));

  let answered = service.approver().reply(question.id, &reply(Some("tomorrow"), "tomorrow")).await;
  assert!(answered.expect("a person answers it now").settles);
}

#[tokio::test]
async fn a_settled_question_is_put_to_nobody_else() {
  let service = Service::start().await;
  let question = service.client().ask(&put_to_the_supervisor(None)).await.expect("asked");
  service.supervisor().reply(question.id, &settling("done")).await.expect("settled");

  let to_person = QuestionChange {
    addressee: Some(QuestionAddressee::Person),
    ..QuestionChange::default()
  };
  let refused = service
    .supervisor()
    .change_question(question.id, &to_person)
    .await
    .expect_err("settled");
  assert_eq!(refused.code(), Some("conflict"));
}

async fn put_to_a_person(service: &Service, id: i64) -> bmo_job_client::Question {
  service
    .supervisor()
    .change_question(
      id,
      &QuestionChange {
        addressee: Some(QuestionAddressee::Person),
        ..QuestionChange::default()
      },
    )
    .await
    .expect("put to a person")
}

#[tokio::test]
async fn the_supervisor_never_settles_what_it_put_to_a_person_and_its_word_there_is_its_own() {
  let service = Service::start().await;
  let question = service.client().ask(&put_to_the_supervisor(None)).await.expect("asked");
  put_to_a_person(&service, question.id).await;

  let refused = service
    .supervisor()
    .reply(question.id, &settling("done"))
    .await
    .expect_err("not its to settle");
  assert_eq!(refused.code(), Some("not-the-addressee"));

  let word = service
    .supervisor()
    .reply(
      question.id,
      &NewReply {
        settles: Some(false),
        ..settling("the node is the one in the rack")
      },
    )
    .await
    .expect("a word for context");
  assert_eq!((word.settles, word.source), (false, ReplySource::Supervisor));

  let answer = service
    .approver()
    .reply(question.id, &reply(None, "the rack one"))
    .await
    .expect("the console answers");
  assert_eq!((answer.settles, answer.source), (true, ReplySource::Person));
}

#[tokio::test]
async fn a_change_on_the_condition_of_an_addressee_changes_nothing_where_it_no_longer_holds() {
  let service = Service::start().await;
  let question = service.client().ask(&put_to_the_supervisor(None)).await.expect("asked");
  put_to_a_person(&service, question.id).await;

  let refused = service
    .client()
    .change_question(
      question.id,
      &QuestionChange {
        state: Some(QuestionState::Withdrawn),
        if_addressee: Some(QuestionAddressee::Supervisor),
        ..QuestionChange::default()
      },
    )
    .await
    .expect_err("no longer the supervisor's");
  assert_eq!(refused.code(), Some("conflict"));
  let standing = service.client().question(question.id).await.expect("read");
  assert_eq!((standing.state, standing.addressee), (QuestionState::Open, QuestionAddressee::Person));
}

#[tokio::test]
async fn the_asker_keeps_its_notice_on_the_question_and_a_move_to_a_person_is_told_as_changed() {
  let service = Service::start().await;
  let client = service.client();
  let question = client.ask(&put_to_the_supervisor(None)).await.expect("asked");
  let kept = client
    .change_question(
      question.id,
      &QuestionChange {
        told_as: Some(r#"{"notice":7,"since":null}"#.into()),
        ..QuestionChange::default()
      },
    )
    .await
    .expect("kept");
  assert_eq!(kept.told_as.as_deref(), Some(r#"{"notice":7,"since":null}"#));

  put_to_a_person(&service, question.id).await;
  let changed = EventQuery {
    kinds: Some("question.changed".into()),
    ..EventQuery::default()
  };
  let told = client.events_page(&changed).await.expect("read").items;
  let last = told.last().expect("the move is told");
  assert_eq!(
    (last.data["id"].as_i64(), last.data["addressee"].as_str()),
    (Some(question.id), Some("person"))
  );
}
