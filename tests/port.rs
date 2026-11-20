//! The bmo-receiver port through the real server: a room's holdings, its open questions and items, and the
//! owner's answer or talk on a question (docs/api.md, "The bmo-receiver port").

use crate::support;

use bmo_job_client::{
  ApprovalGiven, EventQuery, Item, ItemChange, ItemState, JobClient, NewItem, NewQuestion, NewReply,
  QuestionAddressee, QuestionChange, QuestionRecommendation, QuestionState,
};
use reqwest::Method;
use serde_json::{Value, json};

use support::Service;

async fn item_in(client: &JobClient, scope: &str, title: &str, assignee: Option<&str>) -> Item {
  client
    .create_item(&NewItem {
      title: title.to_owned(),
      brief: format!("the brief of {title}"),
      queue_name: "build".into(),
      scope: Some(scope.to_owned()),
      assignee_name: assignee.map(str::to_owned),
      ..NewItem::default()
    })
    .await
    .expect("raised")
}

async fn approve(client: &JobClient, item: Item) -> Item {
  client
    .change_item(
      item.id,
      &ItemChange {
        approval: Some(ApprovalGiven::default()),
        ..ItemChange::default()
      },
    )
    .await
    .expect("approved")
}

async fn moved(client: &JobClient, item: Item, state: ItemState) -> Item {
  client
    .change_item(
      item.id,
      &ItemChange {
        state: Some(state),
        reason: (state == ItemState::Held).then(|| "waits on a decision".to_owned()),
        ..ItemChange::default()
      },
    )
    .await
    .expect("moved")
}

async fn asked(
  client: &JobClient,
  scope: &str,
  choices: &[&str],
  wait_ms: Option<i64>,
) -> bmo_job_client::Question {
  client
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      scope: Some(scope.to_owned()),
      text: "Which way?".into(),
      choices: choices.iter().map(|choice| (*choice).to_owned()).collect(),
      wait_ms,
      ..NewQuestion::default()
    })
    .await
    .expect("asked")
}

#[tokio::test]
async fn a_rooms_holdings_count_what_still_belongs_to_it() {
  let service = Service::start().await;
  let client = service.client();

  let open = approve(&service.approver(), item_in(&client, "7", "open", None).await).await;
  item_in(&client, "7", "draft", None).await;
  let held = moved(
    &client,
    approve(&service.approver(), item_in(&client, "7", "held", None).await).await,
    ItemState::Held,
  )
  .await;
  let dropped = moved(&client, item_in(&client, "7", "dropped", None).await, ItemState::Dropped).await;
  item_in(&client, "8", "elsewhere", None).await;
  let waiting = asked(&service.supervisor(), "7", &["yes", "no"], None).await;
  let settled = asked(&service.supervisor(), "7", &["yes", "no"], None).await;
  client
    .reply(
      settled.id,
      &NewReply {
        choice: Some("yes".into()),
        text: "yes".into(),
        actor_name: "owner".into(),
        settles: None,
      },
    )
    .await
    .expect("settled");

  assert_eq!(
    (open.state, held.state, dropped.state),
    (ItemState::Open, ItemState::Held, ItemState::Dropped)
  );

  let held_of_room = service
    .raw(Method::GET, "/rooms/7/holdings")
    .send()
    .await
    .expect("answered")
    .json::<Value>()
    .await
    .expect("a list");
  assert_eq!(
    held_of_room,
    json!([
      { "code": "room_open_items", "count": 3 },
      { "code": "room_waiting_questions", "count": 1 },
    ]),
    "the open, draft and held items stand, the dropped one does not; {waiting:?} waits"
  );

  let empty = service
    .raw(Method::GET, "/rooms/9/holdings")
    .send()
    .await
    .expect("answered")
    .json::<Value>()
    .await
    .expect("a list");
  assert_eq!(empty, json!([]), "a room holding nothing is named nowhere");
}

#[tokio::test]
async fn a_rooms_open_questions_are_read_last_asked_first() {
  let service = Service::start().await;
  let client = service.client();

  let first = asked(&service.supervisor(), "7", &["strict", "loose"], Some(60_000)).await;
  let paused = asked(&service.supervisor(), "7", &["a", "b"], Some(60_000)).await;
  client
    .change_question(
      paused.id,
      &QuestionChange {
        state: Some(QuestionState::Paused),
        ..Default::default()
      },
    )
    .await
    .expect("paused");
  let past = asked(&service.supervisor(), "7", &["x", "y"], Some(1_000)).await;
  let answered = asked(&service.supervisor(), "7", &["yes", "no"], None).await;
  client
    .reply(
      answered.id,
      &NewReply {
        choice: Some("no".into()),
        text: "no".into(),
        actor_name: "owner".into(),
        settles: None,
      },
    )
    .await
    .expect("settled");
  asked(&service.supervisor(), "8", &["p", "q"], None).await;
  let suggested = service
    .supervisor()
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      scope: Some("7".into()),
      text: "Which way?".into(),
      choices: vec!["a".into(), "b".into()],
      recommendation: Some(QuestionRecommendation {
        answer: "b".into(),
        why: "It keeps the old name.".into(),
      }),
      wait_ms: Some(60_000),
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  service.pass(2_000).await;

  let open = service
    .raw(Method::GET, "/questions/open?chat=7")
    .send()
    .await
    .expect("answered")
    .json::<Value>()
    .await
    .expect("a list");
  assert_eq!(
    open,
    json!([
      { "id": suggested.id, "options": ["a", "b"], "recommendation": "b", "why": "It keeps the old name.", "talks": true },
      { "id": past.id, "options": ["x", "y"], "recommendation": null, "why": null, "talks": true },
      { "id": paused.id, "options": ["a", "b"], "recommendation": null, "why": null, "talks": true },
      { "id": first.id, "options": ["strict", "loose"], "recommendation": null, "why": null, "talks": true },
    ]),
    "a question past its due time still waits, and the answered one takes no answer"
  );
}

#[tokio::test]
async fn the_open_items_are_read_in_the_ports_shape_oldest_first() {
  let service = Service::start().await;
  let client = service.client();

  let waiting =
    approve(&service.approver(), item_in(&client, "project-7", "waiting", Some("runner-a")).await).await;
  let draft = item_in(&client, "project-7", "draft", None).await;
  // The shared token is no approver: its approval is a recommendation, and the draft is not approved.
  let recommended = approve(&client, item_in(&client, "project-7", "recommended", None).await).await;
  assert_eq!(recommended.state, ItemState::Draft);
  let held = moved(
    &client,
    approve(&service.approver(), item_in(&client, "project-7", "held", None).await).await,
    ItemState::Held,
  )
  .await;
  let dropped =
    moved(&client, item_in(&client, "project-7", "dropped", None).await, ItemState::Dropped).await;

  let listed = service
    .raw(Method::GET, "/items?open=1")
    .send()
    .await
    .expect("answered")
    .json::<Value>()
    .await
    .expect("an object");
  assert_eq!(
    listed,
    json!({
      "items": [
        { "id": waiting.id, "approved": true, "status": "open", "title": "waiting", "worker": "runner-a" },
        { "id": draft.id, "approved": false, "status": "draft", "title": "draft", "worker": null },
        { "id": recommended.id, "approved": false, "status": "draft", "title": "recommended", "worker": null },
      ]
    }),
    "the held {held:?} and dropped {dropped:?} items are not open"
  );

  let refused = service.raw(Method::GET, "/items?open=0").send().await.expect("answered");
  assert_eq!(refused.status(), 400);
  let body = refused.json::<Value>().await.expect("the error shape");
  assert_eq!(body["code"], "bad-request");

  let plain = client.items(&Default::default()).await.expect("the list reads as before");
  assert!(plain.items.iter().any(|item| item.id == waiting.id));
}

#[tokio::test]
async fn a_question_is_settled_by_the_number_of_its_choice() {
  let service = Service::start().await;
  let client = service.client();
  let question = asked(&service.supervisor(), "7", &["strict", "loose"], None).await;

  let settled = service
    .raw(Method::POST, "/question")
    .json(&json!({ "id": question.id, "chosen": 1 }))
    .send()
    .await
    .expect("answered");
  assert_eq!(settled.status(), 204);

  let after = client.question(question.id).await.expect("read");
  assert_eq!(after.state, QuestionState::Answered);
  assert_eq!(after.choices, ["strict", "loose"]);
  let replies = client.replies(question.id, &Default::default()).await.expect("read").items;
  assert_eq!(replies.len(), 1);
  assert_eq!(replies[0].choice.as_deref(), Some("loose"));
  assert!(replies[0].settles);

  let again = service
    .raw(Method::POST, "/question")
    .json(&json!({ "id": question.id, "chosen": 0 }))
    .send()
    .await
    .expect("answered");
  assert_eq!(again.status(), 204, "a settled question is settled all the same");

  let wrong = asked(&service.supervisor(), "7", &["strict", "loose"], None).await;
  let refused = service
    .raw(Method::POST, "/question")
    .json(&json!({ "id": wrong.id, "chosen": 9 }))
    .send()
    .await
    .expect("answered");
  assert_eq!(refused.status(), 400);
  assert_eq!(refused.json::<Value>().await.expect("the error shape")["code"], "question_answer_wrong");

  let absent = service
    .raw(Method::POST, "/question")
    .json(&json!({ "id": 9_999, "chosen": 0 }))
    .send()
    .await
    .expect("answered");
  assert_eq!(absent.status(), 404);
  assert_eq!(absent.json::<Value>().await.expect("the error shape")["code"], "question_absent");
}

#[tokio::test]
async fn a_talk_is_a_word_that_stops_the_clock_and_settles_nothing() {
  let service = Service::start().await;
  let client = service.client();
  let question = asked(&service.supervisor(), "7", &["strict", "loose"], Some(60_000)).await;

  let talked = service
    .raw(Method::POST, "/question-talk")
    .json(&json!({ "id": question.id, "say": "let me think" }))
    .send()
    .await
    .expect("answered");
  assert_eq!(talked.status(), 204);

  let after = client.question(question.id).await.expect("read");
  assert_eq!((after.state, after.due_at), (QuestionState::Paused, None));
  let replies = client.replies(question.id, &Default::default()).await.expect("read").items;
  assert_eq!(replies.len(), 1);
  assert_eq!((replies[0].text.as_str(), replies[0].settles), ("let me think", false));
  assert_eq!(replies[0].actor_name, "owner");

  client
    .reply(
      question.id,
      &NewReply {
        choice: Some("loose".into()),
        text: "loose".into(),
        actor_name: "owner".into(),
        settles: None,
      },
    )
    .await
    .expect("settled");
  let refused = service
    .raw(Method::POST, "/question-talk")
    .json(&json!({ "id": question.id, "say": "still there?" }))
    .send()
    .await
    .expect("answered");
  assert_eq!(refused.status(), 409);
  assert_eq!(refused.json::<Value>().await.expect("the error shape")["code"], "question_talk_settled");

  let silent = service
    .raw(Method::POST, "/question-talk")
    .json(&json!({ "id": question.id, "say": " " }))
    .send()
    .await
    .expect("answered");
  assert_eq!(silent.status(), 400);
  assert_eq!(silent.json::<Value>().await.expect("the error shape")["code"], "question_talk_say");

  let absent = service
    .raw(Method::POST, "/question-talk")
    .json(&json!({ "id": 9_999, "say": "hello" }))
    .send()
    .await
    .expect("answered");
  assert_eq!(absent.status(), 404);
  assert_eq!(absent.json::<Value>().await.expect("the error shape")["code"], "question_absent");
}

#[tokio::test]
async fn the_queue_moved_is_put_into_the_stream() {
  let service = Service::start().await;
  let client = service.client();

  let sent = service
    .raw(Method::POST, "/queue/moved")
    .json(&json!({ "detail": "runner-b" }))
    .send()
    .await
    .expect("answered");
  assert_eq!(sent.status(), 204);
  let bare = service
    .raw(Method::POST, "/queue/moved")
    .json(&json!({ "detail": null }))
    .send()
    .await
    .expect("answered");
  assert_eq!(bare.status(), 204);

  let events = client
    .events_page(&EventQuery {
      kinds: Some("queue.moved".into()),
      ..EventQuery::default()
    })
    .await
    .expect("read")
    .items;
  assert_eq!(events.len(), 2);
  assert_eq!(events[0].data["detail"], "runner-b");
  assert!(events[1].data.get("detail").is_none(), "a moved with no detail carries none");
}

#[tokio::test]
async fn a_question_the_supervisor_holds_is_neither_listed_settled_nor_talked_over_until_put_to_the_owner()
{
  let service = Service::start().await;
  let held = service
    .client()
    .ask(&NewQuestion {
      asker_name: "builder".into(),
      scope: Some("7".into()),
      addressee: Some(QuestionAddressee::Supervisor),
      text: "Which way?".into(),
      choices: vec!["a".into(), "b".into()],
      ..NewQuestion::default()
    })
    .await
    .expect("asked");
  let open = || async {
    service
      .raw(Method::GET, "/questions/open?chat=7")
      .send()
      .await
      .expect("answered")
      .json::<Value>()
      .await
      .expect("a list")
  };
  assert_eq!(open().await, json!([]));

  for (path, body) in [
    ("/question", json!({ "id": held.id, "chosen": 0 })),
    ("/question-talk", json!({ "id": held.id, "say": "what is it for?" })),
  ] {
    let refused = service.raw(Method::POST, path).json(&body).send().await.expect("answered");
    assert_eq!(refused.status(), 404, "{path}");
    assert_eq!(
      refused.json::<Value>().await.expect("the error shape")["code"],
      "question_absent",
      "{path}"
    );
  }
  let holdings = service.raw(Method::GET, "/rooms/7/holdings").send().await.expect("answered");
  assert_eq!(
    holdings.json::<Value>().await.expect("the holdings"),
    json!([{ "code": "room_waiting_questions", "count": 1 }]),
    "the room holds it all the same"
  );

  service
    .supervisor()
    .change_question(
      held.id,
      &QuestionChange {
        addressee: Some(QuestionAddressee::Person),
        recommendation: Some(QuestionRecommendation {
          answer: "a".into(),
          why: "it keeps the change small".into(),
        }),
        ..QuestionChange::default()
      },
    )
    .await
    .expect("put to the owner");
  assert_eq!(
    open().await,
    json!([{ "id": held.id, "options": ["a", "b"], "recommendation": "a", "why": "it keeps the change small", "talks": true }])
  );
  let settled = service
    .raw(Method::POST, "/question")
    .json(&json!({ "id": held.id, "chosen": 1 }))
    .send()
    .await
    .expect("answered");
  assert_eq!(settled.status(), 204);
  assert_eq!(service.client().question(held.id).await.expect("read").state, QuestionState::Answered);
}
