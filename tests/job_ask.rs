//! `bmo job ask` from a shell (docs/cli.md, 「`bmo-job job ask`」), the command run over the real service and a
//! receiver of the test's own: a question goes back only with what a person settled it with, a failed send ends it
//! unsent with no option printed, and no clock settles anything.

use crate::support;

use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{delete, get, post};
use bmo_job::job_ask::settled_line;
use bmo_job_client::{
  JobClient, NewItem, NewReply, Question, QuestionQuery, QuestionState, Reply, ReplySource,
};
use serde_json::{Value, json};
use tokio::process::{Child, Command};

use support::Service;

const ROOM: i64 = 4242;

/// A receiver of the test's own: the owner's room, what it was handed, and what was taken back.
#[derive(Clone, Default)]
struct Heard {
  refuses: bool,
  orders: Arc<Mutex<Vec<Value>>>,
  withdrawn: Arc<Mutex<Vec<i64>>>,
}

async fn owner() -> axum::Json<Value> {
  axum::Json(json!({ "senderId": 1, "room": ROOM }))
}

async fn instruct(
  State(heard): State<Heard>,
  axum::Json(body): axum::Json<Value>,
) -> (StatusCode, axum::Json<Value>) {
  if heard.refuses {
    return (
      StatusCode::CONFLICT,
      axum::Json(json!({ "code": "no-supervisor", "error": "nobody answers the room" })),
    );
  }

  let mut orders = heard.orders.lock().expect("the orders");

  orders.push(body);

  (StatusCode::CREATED, axum::Json(json!({ "id": orders.len() })))
}

async fn withdraw(State(heard): State<Heard>, Path(id): Path<i64>) -> StatusCode {
  heard.withdrawn.lock().expect("the withdrawals").push(id);

  StatusCode::NO_CONTENT
}

async fn receiver(refuses: bool) -> (String, Heard) {
  let heard = Heard {
    refuses,
    ..Heard::default()
  };
  let app = Router::new()
    .route("/owner", get(owner))
    .route("/instructions", post(instruct))
    .route("/instructions/{id}", delete(withdraw))
    .with_state(heard.clone());
  let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
  let address = listener.local_addr().expect("the port's address");

  tokio::spawn(async move {
    let _ = axum::serve(listener, app).await;
  });

  (format!("http://{address}"), heard)
}

async fn item(client: &JobClient) -> i64 {
  client
    .create_item(&NewItem {
      title: "an item asked about".to_owned(),
      brief: "ask".to_owned(),
      queue_name: "todo".to_owned(),
      ..NewItem::default()
    })
    .await
    .expect("the item")
    .id
}

fn command(service: &Service, receiver: &str, words: &[&str]) -> Command {
  let mut command = Command::new(env!("CARGO_BIN_EXE_bmo-job"));

  command
    .args(["job", "ask"])
    .args(words)
    .env("BMO_JOB_URL", &service.url)
    .env("BMO_JOB_TOKEN", support::TOKEN)
    .env("BMO_RECEIVER_URL", receiver)
    .env("BMO_RECEIVER_TOKEN", "receiver-token")
    .env("BMO_RUN_NAME", "job-7")
    .stdin(Stdio::null())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .kill_on_drop(true);

  command
}

fn asked(service: &Service, receiver: &str, item: i64) -> Child {
  let item = item.to_string();

  command(
    service,
    receiver,
    &[
      "--todo",
      &item,
      "--question",
      "which one?",
      "--option",
      "strict grammar",
      "--option",
      "loose grammar",
      "--recommend",
      "1",
      "--why",
      "A loose grammar reads a stray token as a rule.",
    ],
  )
  .spawn()
  .expect("the command runs")
}

// The question the running command opened, once it has handed it to the room.
async fn opened(client: &JobClient, heard: &Heard, orders: usize) -> Question {
  for _ in 0..600 {
    if heard.orders.lock().expect("the orders").len() >= orders {
      let page = client
        .questions(&QuestionQuery {
          scope: Some(ROOM.to_string()),
          ..QuestionQuery::default()
        })
        .await
        .expect("the room's questions");

      return page.items.last().expect("the question stands").clone();
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
  }

  panic!("the question was never handed to the room");
}

async fn ended(child: Child) -> (i32, String, String) {
  let output = tokio::time::timeout(Duration::from_secs(60), child.wait_with_output())
    .await
    .expect("the command ends")
    .expect("its output");

  (
    output.status.code().unwrap_or(-1),
    String::from_utf8_lossy(&output.stdout).into_owned(),
    String::from_utf8_lossy(&output.stderr).into_owned(),
  )
}

fn person(choice: Option<&str>, text: &str) -> NewReply {
  NewReply {
    choice: choice.map(str::to_owned),
    text: text.to_owned(),
    actor_name: "owner".to_owned(),
    settles: Some(true),
  }
}

// What the order handed to the room at `at` says.
fn order_text(heard: &Heard, at: usize) -> String {
  heard.orders.lock().expect("the orders")[at]["said"]["order"]
    .as_str()
    .expect("the order's words")
    .to_owned()
}

#[tokio::test]
async fn a_failed_send_ends_non_zero_prints_no_option_and_closes_the_question_unsent() {
  let service = Service::start().await;
  let client = service.client();
  let (address, _heard) = receiver(true).await;
  let item = item(&client).await;
  let (code, out, err) = ended(asked(&service, &address, item)).await;

  assert_eq!(code, 1, "{err}");
  assert_eq!(out, "", "no option is printed");
  assert!(err.contains("closed unsent"), "{err}");

  let page = client
    .questions(&QuestionQuery {
      item_id: Some(item),
      ..QuestionQuery::default()
    })
    .await
    .expect("the item's questions");

  assert_eq!(page.items.len(), 1);
  assert_eq!(page.items[0].state, QuestionState::Unsent);
}

#[tokio::test]
async fn a_receiver_that_cannot_be_reached_opens_no_question() {
  let service = Service::start().await;
  let client = service.client();
  let item = item(&client).await;
  let (code, out, _) = ended(asked(&service, "http://127.0.0.1:9", item)).await;

  assert_eq!(code, 1);
  assert_eq!(out, "");

  let page = client
    .questions(&QuestionQuery {
      item_id: Some(item),
      ..QuestionQuery::default()
    })
    .await
    .expect("the item's questions");

  assert!(page.items.is_empty());
}

#[tokio::test]
async fn a_passed_due_time_settles_nothing_and_a_persons_answer_does() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let mut child = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;

  assert!(question.told_by_asker);
  assert_eq!(question.scope.as_deref(), Some(ROOM.to_string().as_str()));
  assert_eq!(question.recommendation.as_ref().expect("recommended").answer, "strict grammar");

  // Far past its due time: told of as due, and still open, with the command still waiting.
  service.pass(30 * 24 * 3_600_000).await;
  tokio::time::sleep(Duration::from_millis(500)).await;

  assert!(child.try_wait().expect("the command's state").is_none(), "no clock ends the wait");
  assert_eq!(client.question(question.id).await.expect("the question").state, QuestionState::Open);

  client
    .reply(question.id, &person(Some("loose grammar"), "loose grammar"))
    .await
    .expect("answered");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "loose grammar\n", "the person's choice, not the recommendation");
  assert_eq!(*heard.withdrawn.lock().expect("the withdrawals"), [1]);
}

#[tokio::test]
async fn the_question_turn_carries_a_marker_and_the_data_and_no_instruction() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;
  let turn = order_text(&heard, 0);

  assert!(
    turn.starts_with(&format!("question-awaits-answer #{}", question.id)),
    "the notice is named first: {turn}"
  );
  assert!(
    turn.contains(&format!("item #{item}: an item asked about")),
    "the item and its title: {turn}"
  );
  assert!(turn.contains("which one?"), "the question's words: {turn}");
  assert!(turn.contains("1. strict grammar (recommended)"), "{turn}");
  assert!(turn.contains("2. loose grammar"), "{turn}");
  assert!(turn.contains("why: A loose grammar reads a stray token as a rule."), "{turn}");

  for instruction in ["asks the owner", "Put it to him", "Never answer it", "never choose"] {
    assert!(!turn.contains(instruction), "the turn holds no instruction ({instruction}): {turn}");
  }

  client
    .reply(question.id, &person(Some("strict grammar"), "strict grammar"))
    .await
    .expect("answered");

  let (code, _, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
}

#[tokio::test]
async fn a_persons_own_words_come_back_as_one_line() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;

  client
    .reply(question.id, &person(None, "neither\na third way"))
    .await
    .expect("answered");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "neither a third way\n");
}

#[test]
fn a_settling_reply_no_person_gave_is_never_an_answer() {
  let mut reply = Reply {
    id: 1,
    question_id: 2,
    choice: Some("strict grammar".to_owned()),
    text: "strict grammar".to_owned(),
    actor_name: "deadline".to_owned(),
    settles: true,
    source: ReplySource::Recommendation,
    why: Some("the wait ran out".to_owned()),
    created_at: 0,
  };

  assert!(settled_line(&reply).is_err());

  reply.source = ReplySource::Person;

  assert_eq!(settled_line(&reply).as_deref(), Ok("strict grammar"));
}

#[tokio::test]
async fn talk_exits_3_the_question_stays_open_and_the_answer_waits_on_it_again() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;
  let talk = |say: &str| {
    service
      .raw(reqwest::Method::POST, "/question-talk")
      .json(&json!({ "id": question.id, "say": say }))
      .send()
  };

  talk("what does loose mean?").await.expect("the talk");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 3, "{err}");
  assert!(out.contains("> what does loose mean?"), "{out}");
  assert!(out.contains(&format!("bmo job ask --talk {} --say", question.id)), "{out}");
  assert!(!out.contains("strict grammar"), "no option goes back as an answer: {out}");
  assert_eq!(client.question(question.id).await.expect("the question").state, QuestionState::Paused);

  // What he says while the job writes its answer is not lost: the next wait hands it over.
  talk("and what does it cost?").await.expect("the second talk");

  let id = question.id.to_string();
  let (code, out, err) = ended(
    command(&service, &address, &["--talk", &id, "--say", "loose reads a stray token as a rule."])
      .spawn()
      .expect("runs"),
  )
  .await;

  assert_eq!(code, 3, "{err}");
  assert!(out.contains("> and what does it cost?"), "{out}");
  assert!(!out.contains("what does loose mean?"), "what was heard is not handed twice: {out}");
  assert_eq!(heard.orders.lock().expect("the orders").len(), 2, "the answer went to the room");

  let turn = order_text(&heard, 1);

  assert!(turn.starts_with(&format!("question-talk #{}", question.id)), "{turn}");
  assert!(
    turn.contains(&format!("item #{item}: an item asked about")),
    "the item and its title: {turn}"
  );
  assert!(turn.contains("loose reads a stray token as a rule."), "the job's words: {turn}");

  for instruction in [
    "answers the owner's talk",
    "Say this in this room",
    "Never answer it",
    "not taken for your own",
  ] {
    assert!(!turn.contains(instruction), "the turn holds no instruction ({instruction}): {turn}");
  }

  // His choice ends it, the question put again in other words on the way.
  let child = command(
    &service,
    &address,
    &[
      "--talk",
      &id,
      "--say",
      "it costs the same.",
      "--option",
      "strict",
      "--option",
      "loose",
      "--recommend",
      "2",
    ],
  )
  .spawn()
  .expect("runs");

  opened(&client, &heard, 3).await;
  assert_eq!(client.question(question.id).await.expect("the question").choices, ["strict", "loose"]);

  let put_again = order_text(&heard, 2);

  assert!(put_again.starts_with(&format!("question-talk #{}", question.id)), "{put_again}");
  assert!(put_again.contains("it costs the same."), "the job's words: {put_again}");
  assert!(put_again.contains("question now stands as:"), "{put_again}");
  assert!(put_again.contains("1. strict"), "{put_again}");
  assert!(put_again.contains("2. loose (recommended)"), "{put_again}");

  for instruction in ["asks the owner", "Put it to him", "Never answer it", "never choose"] {
    assert!(
      !put_again.contains(instruction),
      "the turn holds no instruction ({instruction}): {put_again}"
    );
  }
  client.reply(question.id, &person(Some("strict"), "strict")).await.expect("answered");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "strict\n");
}

#[tokio::test]
async fn a_failed_talk_leaves_the_question_open_and_prints_no_option() {
  let service = Service::start().await;
  let client = service.client();
  let (address, _) = receiver(true).await;
  let question = client
    .ask(&bmo_job_client::NewQuestion {
      asker_name: "job-7".to_owned(),
      scope: Some(ROOM.to_string()),
      text: "which?".to_owned(),
      choices: vec!["one".to_owned(), "two".to_owned()],
      told_by_asker: true,
      ..Default::default()
    })
    .await
    .expect("asked");
  let id = question.id.to_string();
  let (code, out, err) = ended(
    command(&service, &address, &["--talk", &id, "--say", "an answer"])
      .spawn()
      .expect("runs"),
  )
  .await;

  assert_eq!(code, 1, "{err}");
  assert_eq!(out, "");
  assert_eq!(client.question(question.id).await.expect("the question").state, QuestionState::Open);
}

#[tokio::test]
async fn a_question_already_settled_is_not_answered_into_and_one_withdrawn_gives_no_option() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let ask = |text: &str| bmo_job_client::NewQuestion {
    asker_name: "job-7".to_owned(),
    scope: Some(ROOM.to_string()),
    text: text.to_owned(),
    choices: vec!["one".to_owned(), "two".to_owned()],
    told_by_asker: true,
    ..Default::default()
  };
  let settled = client.ask(&ask("a settled question")).await.expect("asked");

  client.reply(settled.id, &person(Some("two"), "two")).await.expect("answered");

  let id = settled.id.to_string();
  let (code, out, err) = ended(
    command(&service, &address, &["--talk", &id, "--say", "a late answer"])
      .spawn()
      .expect("runs"),
  )
  .await;

  assert_eq!((code, out.as_str()), (0, "two\n"), "{err}");
  assert!(
    heard.orders.lock().expect("the orders").is_empty(),
    "nothing is said into a settled question"
  );

  let withdrawn = client.ask(&ask("a withdrawn question")).await.expect("asked");

  client
    .change_question(
      withdrawn.id,
      &bmo_job_client::QuestionChange {
        state: Some(QuestionState::Withdrawn),
        ..Default::default()
      },
    )
    .await
    .expect("withdrawn");

  let id = withdrawn.id.to_string();
  let (code, out, err) = ended(
    command(&service, &address, &["--talk", &id, "--say", "a late answer"])
      .spawn()
      .expect("runs"),
  )
  .await;

  assert_eq!((code, out.as_str()), (1, ""), "{err}");
}

#[tokio::test]
async fn a_stop_withdraws_the_question_and_prints_no_option() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;
  let pid = i32::try_from(child.id().expect("it runs")).expect("a pid");

  // The command is told to stop only once it stands in its wait.
  tokio::time::sleep(Duration::from_millis(300)).await;
  assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 1, "{err}");
  assert_eq!(out, "");
  assert_eq!(
    client.question(question.id).await.expect("the question").state,
    QuestionState::Withdrawn
  );
  assert_eq!(*heard.withdrawn.lock().expect("the withdrawals"), [1]);
}

#[tokio::test]
async fn words_a_question_cannot_go_out_with_open_nothing() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let item_word = item.to_string();
  let refused = [
    vec![
      "--todo",
      &item_word,
      "--question",
      "q",
      "--option",
      "one",
      "--recommend",
      "1",
      "--why",
      "w",
    ],
    vec![
      "--todo",
      &item_word,
      "--question",
      "q",
      "--option",
      "one",
      "--option",
      "two",
      "--recommend",
      "3",
      "--why",
      "w",
    ],
    vec![
      "--todo",
      &item_word,
      "--question",
      "q",
      "--option",
      "one",
      "--option",
      "two",
      "--recommend",
      "1",
    ],
    vec![
      "--question",
      "q",
      "--option",
      "one",
      "--option",
      "two",
      "--recommend",
      "1",
      "--why",
      "w",
    ],
    vec![
      "--todo",
      &item_word,
      "--question",
      "look here https://claude.ai/public/artifacts/abc",
      "--option",
      "one",
      "--option",
      "two",
      "--recommend",
      "1",
      "--why",
      "w",
    ],
  ];

  for words in refused {
    let (code, out, err) = ended(command(&service, &address, &words).spawn().expect("runs")).await;

    assert_eq!(code, 1, "{words:?}: {err}");
    assert_eq!(out, "");
    assert!(err.contains("usage: bmo job ask"), "{err}");
  }

  assert!(heard.orders.lock().expect("the orders").is_empty());
  assert!(
    client
      .questions(&QuestionQuery {
        item_id: Some(item),
        ..QuestionQuery::default()
      })
      .await
      .expect("the questions")
      .items
      .is_empty()
  );
}
