//! `bmo job ask` from a shell (docs/cli.md, 「`bmo-job job ask`」), the command run over the real service and a
//! receiver of the test's own: a question goes to the supervisor first and back only with what the supervisor or a
//! person settled it with, or its recommendation once the owner's due time passed, and a failed send ends it unsent
//! with no option printed.

use crate::support;

use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::Router;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use bmo_job_client::{
  JobClient, NewItem, NewReply, Question, QuestionAddressee, QuestionChange, QuestionQuery,
  QuestionRecommendation, QuestionState, WaitQuery,
};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

use support::{START, Service};

const ROOM: i64 = 4242;

/// A receiver of the test's own: the owner's room, what it was handed, what was taken back, and the orders it let go
/// untaken. Each route answers 503, as a receiver restarting behind its proxy does, as many times as its count of
/// `unavailable` says.
#[derive(Clone, Default)]
struct Heard {
  refuses: bool,
  orders: Arc<Mutex<Vec<Value>>>,
  withdrawn: Arc<Mutex<Vec<i64>>>,
  let_go: Arc<Mutex<Vec<i64>>>,
  delivered: Arc<Mutex<Vec<i64>>>,
  owner_unavailable: Arc<AtomicUsize>,
  orders_unavailable: Arc<AtomicUsize>,
}

// Whether a route answers 503 this time, its count of such answers taken down by one.
fn unavailable(count: &AtomicUsize) -> bool {
  count
    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1))
    .is_ok()
}

async fn owner(State(heard): State<Heard>) -> (StatusCode, axum::Json<Value>) {
  if unavailable(&heard.owner_unavailable) {
    return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({})));
  }

  (StatusCode::OK, axum::Json(json!({ "senderId": 1, "room": ROOM })))
}

async fn instruct(
  State(heard): State<Heard>,
  axum::Json(body): axum::Json<Value>,
) -> (StatusCode, axum::Json<Value>) {
  if unavailable(&heard.orders_unavailable) {
    return (StatusCode::SERVICE_UNAVAILABLE, axum::Json(json!({})));
  }

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

// An order as the receiver reads it back: pending, delivered once the turn that took it ended, or gone once let go
// untaken.
async fn order_state(State(heard): State<Heard>, Path(id): Path<i64>) -> (StatusCode, axum::Json<Value>) {
  if heard.let_go.lock().expect("the orders let go").contains(&id) {
    return (StatusCode::NOT_FOUND, axum::Json(json!({ "code": "not_found", "error": "let go" })));
  }

  let state = if heard.delivered.lock().expect("the orders delivered").contains(&id) {
    "delivered"
  } else {
    "pending"
  };

  (StatusCode::OK, axum::Json(json!({ "id": id, "state": state })))
}

async fn withdraw(State(heard): State<Heard>, Path(id): Path<i64>) -> StatusCode {
  heard.withdrawn.lock().expect("the withdrawals").push(id);

  StatusCode::NO_CONTENT
}

async fn receiver(refuses: bool) -> (String, Heard) {
  receiver_letting_go(refuses, &[]).await
}

// A receiver that lets the orders `let_go` names go untaken.
async fn receiver_letting_go(refuses: bool, let_go: &[i64]) -> (String, Heard) {
  let heard = Heard {
    refuses,
    let_go: Arc::new(Mutex::new(let_go.to_vec())),
    ..Heard::default()
  };
  let app = Router::new()
    .route("/owner", get(owner))
    .route("/instructions", post(instruct))
    .route("/instructions/{id}", get(order_state).delete(withdraw))
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

fn command(service: &Service, receiver: &str, words: &[impl AsRef<std::ffi::OsStr>]) -> Command {
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

// The words of the question every test asks about `item`.
fn asked_words(item: i64) -> Vec<String> {
  [
    "--todo",
    &item.to_string(),
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
  ]
  .map(str::to_owned)
  .to_vec()
}

fn asked(service: &Service, receiver: &str, item: i64) -> Child {
  command(service, receiver, &asked_words(item)).spawn().expect("the command runs")
}

// The same ask, its first rest before waking the supervisor again `ms` milliseconds.
fn asked_waking(service: &Service, receiver: &str, item: i64, ms: u64) -> Child {
  command(service, receiver, &asked_words(item))
    .env("BMO_JOB_ASK_WAKE_MS", ms.to_string())
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

// The question once the notice numbered `notice` is kept on it, which the command writes after handing it over.
async fn kept_notice(client: &JobClient, id: i64, notice: i64) -> Question {
  let mark = format!("\"notice\":{notice}");

  for _ in 0..600 {
    let question = client.question(id).await.expect("the question");

    if question.told_as.as_deref().is_some_and(|kept| kept.contains(&mark)) {
      return question;
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
  }

  panic!("notice {notice} was never kept on question #{id}");
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

// The supervisor puts the question it holds to the owner, as its judgement does with a choice that is his, naming
// its first choice where the job recommended none, since a move carries a recommendation; one already put to him
// stays as it is.
async fn to_owner(service: &Service, id: i64) {
  let question = service.client().question(id).await.expect("the question");

  if question.addressee == QuestionAddressee::Person {
    return;
  }

  service
    .supervisor()
    .change_question(
      id,
      &QuestionChange {
        addressee: Some(QuestionAddressee::Person),
        recommendation: question.recommendation.is_none().then(|| QuestionRecommendation {
          answer: question.choices[0].clone(),
          why: "it is the smaller step".to_owned(),
        }),
        ..QuestionChange::default()
      },
    )
    .await
    .expect("put to the owner");
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
async fn a_receiver_that_gives_no_answer_is_asked_again_and_a_stop_opens_nothing() {
  let service = Service::start().await;
  let client = service.client();
  let item = item(&client).await;
  let mut child = asked(&service, "http://[::1]:9", item);
  let mut said = BufReader::new(child.stderr.take().expect("its standard error")).lines();
  let heard_again = tokio::time::timeout(Duration::from_secs(30), async {
    while let Some(line) = said.next_line().await.expect("a line") {
      if line.contains("asking again") {
        return;
      }
    }
    panic!("the command ended without asking again");
  });

  heard_again.await.expect("the receiver is asked again");

  let pid = i32::try_from(child.id().expect("it runs")).expect("a pid");

  assert_eq!(unsafe { libc::kill(pid, libc::SIGTERM) }, 0);

  let (code, out, _) = ended(child).await;
  let mut rest = String::new();

  while let Ok(Some(line)) = said.next_line().await {
    rest.push_str(&line);
  }

  assert_eq!(code, 1);
  assert_eq!(out, "");
  assert!(rest.contains("nothing was opened"), "{rest}");

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
async fn a_receiver_restarting_is_no_answer_and_the_question_goes_out_once() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;

  heard.owner_unavailable.store(1, Ordering::SeqCst);
  heard.orders_unavailable.store(1, Ordering::SeqCst);

  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;

  to_owner(&service, question.id).await;
  client
    .reply(question.id, &person(Some("strict grammar"), "strict"))
    .await
    .expect("answered");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out.trim(), "strict grammar");
  assert_eq!(heard.orders.lock().expect("the orders").len(), 1);

  let page = client
    .questions(&QuestionQuery {
      item_id: Some(item),
      ..QuestionQuery::default()
    })
    .await
    .expect("the item's questions");

  assert_eq!(page.items.len(), 1, "the question is opened once");
}

#[tokio::test]
async fn the_owners_due_time_settles_the_question_on_its_recommendation_and_the_supervisors_never_does() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;

  assert!(question.told_by_asker);
  assert_eq!(question.scope.as_deref(), Some(ROOM.to_string().as_str()));
  assert_eq!(question.recommendation.as_ref().expect("recommended").answer, "strict grammar");

  // Held by the supervisor it has no due time, however long it waits.
  service.pass(30 * 24 * 3_600_000).await;
  assert_eq!(client.question(question.id).await.expect("the question").state, QuestionState::Open);

  // Put to the owner, his minute runs, and the recommendation settles it when nobody answered.
  to_owner(&service, question.id).await;
  service.pass(60_000).await;

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "strict grammar\n", "the recommendation, as the owner's own answer would come back");
  assert!(
    err.contains(&format!("question #{} was settled on its recommendation", question.id)),
    "{err}"
  );
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
    turn.starts_with(&format!("question-for-supervisor #{}", question.id)),
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

  to_owner(&service, question.id).await;
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

  to_owner(&service, question.id).await;
  client
    .reply(question.id, &person(None, "neither\na third way"))
    .await
    .expect("answered");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "neither a third way\n");
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

  to_owner(&service, question.id).await;
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
      "strict rules",
      "--option",
      "loose rules",
      "--recommend",
      "2",
    ],
  )
  .spawn()
  .expect("runs");

  opened(&client, &heard, 3).await;
  assert_eq!(
    client.question(question.id).await.expect("the question").choices,
    ["strict rules", "loose rules"]
  );

  let put_again = order_text(&heard, 2);

  assert!(put_again.starts_with(&format!("question-talk #{}", question.id)), "{put_again}");
  assert!(put_again.contains("it costs the same."), "the job's words: {put_again}");
  assert!(put_again.contains("question now stands as:"), "{put_again}");
  assert!(put_again.contains("1. strict rules"), "{put_again}");
  assert!(put_again.contains("2. loose rules (recommended)"), "{put_again}");

  for instruction in ["asks the owner", "Put it to him", "Never answer it", "never choose"] {
    assert!(
      !put_again.contains(instruction),
      "the turn holds no instruction ({instruction}): {put_again}"
    );
  }
  to_owner(&service, question.id).await;
  client
    .reply(question.id, &person(Some("strict rules"), "strict rules"))
    .await
    .expect("answered");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "strict rules\n");
}

#[tokio::test]
async fn a_failed_talk_leaves_the_question_open_and_prints_no_option() {
  let service = Service::start().await;
  let client = service.client();
  let (address, _) = receiver(true).await;
  let question = service
    .supervisor()
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

  to_owner(&service, settled.id).await;
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

#[tokio::test]
async fn a_receiver_down_and_an_opening_cut_before_its_answer_put_the_question_once() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let receiving = support::Relay::before(&address).await;
  let serving = support::Relay::before(&service.url).await;
  let item = item(&client).await;

  // The receiver restarts as the run begins, and the job service takes the question and is cut before it answers.
  receiving.go_down();
  serving.cut_answers(1);

  let mut command = command(&service, &receiving.url, &asked_words(item));
  let child = command.env("BMO_JOB_URL", &serving.url).spawn().expect("the command runs");

  receiving.until_cut(2).await;
  receiving.come_up();

  let question = opened(&client, &heard, 1).await;

  assert_eq!(serving.cuts(), 1, "the opening was cut after the service took it");

  to_owner(&service, question.id).await;
  client
    .reply(question.id, &person(Some("strict grammar"), "strict"))
    .await
    .expect("answered");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out.trim(), "strict grammar");
  assert!(err.contains("asking again"), "{err}");
  assert_eq!(heard.orders.lock().expect("the orders").len(), 1);

  let page = client
    .questions(&QuestionQuery {
      item_id: Some(item),
      ..QuestionQuery::default()
    })
    .await
    .expect("the item's questions");

  assert_eq!(page.items.len(), 1, "the question is opened once");
}

#[tokio::test]
async fn a_job_service_restart_in_the_middle_of_the_wait_still_hands_the_owners_talk_over() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let serving = support::Relay::before(&service.url).await;
  let item = item(&client).await;
  let mut command = command(&service, &address, &asked_words(item));
  let mut child = command.env("BMO_JOB_URL", &serving.url).spawn().expect("the command runs");
  let mut said = BufReader::new(child.stderr.take().expect("its standard error")).lines();
  let question = opened(&client, &heard, 1).await;

  // The service restarts while the run waits, and the owner talks while it is down.
  serving.go_down();
  tokio::time::timeout(Duration::from_secs(30), async {
    while let Some(line) = said.next_line().await.expect("a line") {
      if line.contains("asking again") {
        return;
      }
    }
    panic!("the command ended without asking again");
  })
  .await
  .expect("the run met the restart");
  client
    .reply(
      question.id,
      &NewReply {
        choice: None,
        text: "why not both?".to_owned(),
        actor_name: "owner".to_owned(),
        settles: Some(false),
      },
    )
    .await
    .expect("he talks");
  serving.come_up();

  let (code, out, _) = ended(child).await;

  assert_eq!(code, i32::from(bmo_job::job_ask::TALK_EXIT));
  assert!(out.contains("> why not both?"), "{out}");
  assert!(
    client.question(question.id).await.expect("the question").heard_through.is_some(),
    "where the job heard through is kept"
  );
}

#[tokio::test]
async fn a_talk_waits_out_a_receiver_restart_and_reaches_the_room_once() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let receiving = support::Relay::before(&address).await;
  let question = service
    .supervisor()
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

  receiving.go_down();

  let child = command(&service, &receiving.url, &["--talk", &id, "--say", "one keeps it simple"])
    .spawn()
    .expect("runs");

  receiving.until_cut(2).await;
  receiving.come_up();
  opened(&client, &heard, 1).await;
  to_owner(&service, question.id).await;
  client.reply(question.id, &person(Some("one"), "one")).await.expect("answered");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out.trim(), "one");
  assert!(order_text(&heard, 0).contains("one keeps it simple"));
  assert_eq!(heard.orders.lock().expect("the orders").len(), 1);

  let replies = client
    .replies(question.id, &bmo_job_client::WaitQuery::default())
    .await
    .expect("the replies")
    .items;

  assert_eq!(replies.iter().filter(|reply| reply.text == "one keeps it simple").count(), 1);
}

// The questions asked about `item`, every state.
async fn rows(client: &JobClient, item: i64) -> Vec<Question> {
  client
    .questions(&QuestionQuery {
      item_id: Some(item),
      ..QuestionQuery::default()
    })
    .await
    .expect("the item's questions")
    .items
}

#[tokio::test]
async fn a_repeated_ask_for_one_item_is_one_row_told_once() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let first = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;
  let mut second = asked(&service, &address, item);
  let mut said = BufReader::new(second.stderr.take().expect("its standard error")).lines();
  let rejoined = tokio::time::timeout(Duration::from_secs(30), said.next_line())
    .await
    .expect("the second ask says what it stands as")
    .expect("a line")
    .unwrap_or_default();

  assert!(
    rejoined.contains(&format!("question #{} was asked before and still waits", question.id)),
    "{rejoined}"
  );

  to_owner(&service, question.id).await;
  client
    .reply(question.id, &person(Some("strict grammar"), "strict grammar"))
    .await
    .expect("answered");

  let (code, out, err) = ended(first).await;

  assert_eq!((code, out.as_str()), (0, "strict grammar\n"), "{err}");

  let (code, out, err) = ended(second).await;

  assert_eq!((code, out.as_str()), (0, "strict grammar\n"), "{err}");
  assert_eq!(rows(&client, item).await.len(), 1, "one question is one row");
  assert_eq!(heard.orders.lock().expect("the orders").len(), 1, "the owner is told once");
}

#[tokio::test]
async fn an_answered_question_answers_its_second_ask_with_the_settled_line_and_opens_nothing() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let first = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;

  to_owner(&service, question.id).await;
  client
    .reply(question.id, &person(Some("loose grammar"), "loose grammar"))
    .await
    .expect("answered");
  ended(first).await;

  let (code, out, err) = ended(asked(&service, &address, item)).await;

  assert_eq!((code, out.as_str()), (0, "loose grammar\n"), "{err}");
  assert!(err.contains(&format!("question #{} was settled before", question.id)), "{err}");
  assert_eq!(rows(&client, item).await.len(), 1, "nothing is opened");
  assert_eq!(heard.orders.lock().expect("the orders").len(), 1, "nobody is told again");

  let shown = ["--todo", &item.to_string(), "--show"];
  let (code, out, err) = ended(command(&service, &address, &shown).spawn().expect("runs")).await;
  let read: Vec<Value> =
    out.lines().map(|line| serde_json::from_str(line).expect("a line of JSON")).collect();

  assert_eq!(code, 0, "{err}");
  assert_eq!(
    read
      .iter()
      .map(|question| (&question["id"], &question["state"], &question["answer"]))
      .collect::<Vec<_>>(),
    [(&json!(question.id), &json!("answered"), &json!("loose grammar"))]
  );
}

#[tokio::test]
async fn a_withdrawn_question_is_asked_anew_in_a_row_of_its_own() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let first = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;

  client
    .change_question(
      question.id,
      &bmo_job_client::QuestionChange {
        state: Some(QuestionState::Withdrawn),
        ..Default::default()
      },
    )
    .await
    .expect("withdrawn");
  // The run that asked first sees the withdrawal only once its held-open read ends, which another test covers;
  // it is taken down here so this one waits on no clock.
  drop(first);

  let second = asked(&service, &address, item);
  let anew = opened(&client, &heard, 2).await;

  assert_ne!(anew.id, question.id, "a withdrawn question is not stood by");

  to_owner(&service, anew.id).await;
  client
    .reply(anew.id, &person(Some("strict grammar"), "strict grammar"))
    .await
    .expect("answered");

  let (code, out, err) = ended(second).await;

  assert_eq!((code, out.as_str()), (0, "strict grammar\n"), "{err}");
  assert_eq!(rows(&client, item).await.len(), 2, "the withdrawn row stands beside the new one");
}

#[tokio::test]
async fn a_job_asks_its_supervisor_first_and_the_supervisors_answer_comes_back_as_written() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let question = opened(&client, &heard, 1).await;

  assert_eq!(
    (question.addressee, question.due_at),
    (QuestionAddressee::Supervisor, Some(START + 60_000)),
    "the supervisor has the question wait"
  );

  let turn = order_text(&heard, 0);
  assert!(turn.starts_with(&format!("question-for-supervisor #{}", question.id)), "{turn}");
  assert!(
    turn.contains("job_reply") && turn.contains("job_question_change"),
    "the tools that answer it: {turn}"
  );
  assert_eq!(
    heard.orders.lock().expect("the orders")[0]["kind"],
    format!("question-for-supervisor:{}", question.id)
  );

  // Neither the job's own token nor a console account settles what the supervisor holds.
  for (who, answering) in
    [("the shared token", service.client()), ("a console account", service.approver())]
  {
    let refused = answering
      .reply(question.id, &person(Some("strict grammar"), "strict"))
      .await
      .expect_err(who);
    assert_eq!(refused.code(), Some("not-the-addressee"), "{who}");
  }

  service
    .supervisor()
    .reply(question.id, &person(None, "ran the probe on the node:\nall eight cards answer"))
    .await
    .expect("the supervisor settles it");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(
    out, "ran the probe on the node:\nall eight cards answer\n",
    "the supervisor's answer as written"
  );
  assert_eq!(*heard.withdrawn.lock().expect("the withdrawals"), [1]);
}

#[tokio::test]
async fn a_question_the_supervisor_leaves_for_a_minute_goes_to_the_owner_and_his_answer_settles_it() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let mut child = asked(&service, &address, item);
  let mut said = BufReader::new(child.stderr.take().expect("its standard error")).lines();
  let question = opened(&client, &heard, 1).await;

  service.pass(60_000).await;
  // A read of the replies already held open runs out first: a minute at most.
  let told = tokio::time::timeout(Duration::from_secs(150), async {
    while let Some(line) = said.next_line().await.expect("a line") {
      if line.contains("time on question") && line.contains("ran out") && line.contains("to the owner") {
        return;
      }
    }
    panic!("the command never said the service put the question to the owner");
  });
  told.await.expect("the job is told the owner has it");
  assert_eq!(
    *heard.withdrawn.lock().expect("the withdrawals"),
    [1],
    "the supervisor's notice is done with"
  );

  service
    .approver()
    .reply(question.id, &person(None, "strict"))
    .await
    .expect("he answers");
  let (code, out, _) = ended(child).await;
  assert_eq!((code, out.as_str()), (0, "strict\n"));
}

#[tokio::test]
async fn the_supervisor_puts_it_to_the_owner_and_his_answer_settles_it() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let mut child = asked(&service, &address, item);
  let mut said = BufReader::new(child.stderr.take().expect("its standard error")).lines();
  let question = opened(&client, &heard, 1).await;

  to_owner(&service, question.id).await;
  // A read of the replies already held open runs out first: a minute at most.
  let told = tokio::time::timeout(Duration::from_secs(150), async {
    while let Some(line) = said.next_line().await.expect("a line") {
      if line.contains("put question") && line.contains("to the owner") {
        return;
      }
    }
    panic!("the command never said the question went to the owner");
  });
  told.await.expect("the job is told the owner has it");
  assert_eq!(
    *heard.withdrawn.lock().expect("the withdrawals"),
    [1],
    "the supervisor's notice is done with"
  );

  let open = service
    .raw(reqwest::Method::GET, &format!("/questions/open?chat={ROOM}"))
    .send()
    .await
    .expect("answered")
    .json::<Value>()
    .await
    .expect("a list");
  assert_eq!(open[0]["id"], question.id, "his surfaces list it now");

  service
    .approver()
    .reply(question.id, &person(None, "neither,\n  a third way"))
    .await
    .expect("he answers");

  let (code, out, _) = ended(child).await;

  assert_eq!(code, 0);
  assert_eq!(out, "neither, a third way\n", "a person's answer is one line");
}

#[tokio::test]
async fn a_notice_let_go_untaken_wakes_the_supervisor_again_and_the_question_stays_with_it() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver_letting_go(false, &[1]).await;
  let item = item(&client).await;
  let child = asked_waking(&service, &address, item, 50);
  let held = opened(&client, &heard, 2).await;
  let held = kept_notice(&client, held.id, 2).await;

  assert_eq!((held.addressee, held.due_at), (QuestionAddressee::Supervisor, Some(START + 60_000)));
  assert_eq!(rows(&client, item).await.len(), 1, "nothing is put to the owner");
  let woken = order_text(&heard, 1);
  assert!(woken.starts_with(&format!("question-for-supervisor #{}", held.id)), "{woken}");
  assert!(woken.contains("woken again: 1 of 6"), "{woken}");
  assert!(
    held.told_as.as_deref().is_some_and(|kept| kept.contains("\"wakes\":1")),
    "{:?}",
    held.told_as
  );

  service
    .supervisor()
    .reply(held.id, &person(None, "strict grammar"))
    .await
    .expect("settled");
  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out.trim(), "strict grammar");
  assert!(err.contains("woken again, 1 of 6"), "{err}");
}

#[tokio::test]
async fn an_open_question_puts_no_options_and_the_supervisor_answers_it_in_its_own_words() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let id = item.to_string();
  let child = command(
    &service,
    &address,
    &[
      "--todo",
      &id,
      "--question",
      "Deploy the docs unit at main.",
      "--why",
      "a job never deploys",
    ],
  )
  .spawn()
  .expect("runs");
  let question = opened(&client, &heard, 1).await;

  assert!(question.choices.is_empty() && question.recommendation.is_none());
  assert!(order_text(&heard, 0).contains("choices: an open answer"));

  service
    .supervisor()
    .reply(question.id, &person(None, "live at main"))
    .await
    .expect("settled");

  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "live at main\n");

  let (code, _, err) = ended(
    command(
      &service,
      &address,
      &["--todo", &id, "--question", "q", "--why", "w", "--recommend", "1"],
    )
    .spawn()
    .expect("runs"),
  )
  .await;
  assert_eq!(code, 1);
  assert!(err.contains("an open question puts none"), "{err}");
}

fn supervisors_word(text: &str) -> NewReply {
  NewReply {
    choice: None,
    text: text.to_owned(),
    actor_name: "lead".to_owned(),
    settles: Some(false),
  }
}

#[tokio::test]
async fn a_turn_that_ended_with_no_word_on_the_question_wakes_the_supervisor_again_never_the_owner() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked_waking(&service, &address, item, 50);
  let held = opened(&client, &heard, 1).await;

  kept_notice(&client, held.id, 1).await;
  heard.delivered.lock().expect("the orders delivered").push(1);

  let woken = opened(&client, &heard, 2).await;
  assert_eq!((woken.id, woken.addressee), (held.id, QuestionAddressee::Supervisor));
  assert!(order_text(&heard, 1).starts_with(&format!("question-for-supervisor #{}", held.id)));
  assert_eq!(rows(&client, item).await.len(), 1, "nothing is put to the owner");

  service
    .supervisor()
    .reply(held.id, &person(None, "strict grammar"))
    .await
    .expect("settled");
  let (code, out, err) = ended(child).await;

  assert_eq!(code, 0, "{err}");
  assert_eq!(out.trim(), "strict grammar");
}

#[tokio::test]
async fn past_the_sixth_wake_nobody_is_woken_and_the_question_waits_with_the_supervisor() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;

  // Every notice the receiver takes reads delivered, and the supervisor says no word on any of them.
  heard.delivered.lock().expect("the orders delivered").extend(1..=8);
  let mut child = asked_waking(&service, &address, item, 10);
  let mut said = BufReader::new(child.stderr.take().expect("its standard error")).lines();
  let held = opened(&client, &heard, 7).await;

  assert_eq!(held.addressee, QuestionAddressee::Supervisor);
  assert!(order_text(&heard, 6).contains("woken again: 6 of 6"), "{}", order_text(&heard, 6));

  // Past the sixth wake the command says it wakes nobody again, and waits on.
  let bounded = tokio::time::timeout(Duration::from_secs(60), async {
    while let Some(line) = said.next_line().await.expect("a line") {
      if line.contains("nobody is woken again") {
        return;
      }
    }
    panic!("the command never said it wakes nobody again");
  });
  bounded.await.expect("the wakes end");

  service
    .supervisor()
    .reply(held.id, &person(None, "strict grammar"))
    .await
    .expect("settled");
  let (code, out, _) = ended(child).await;

  assert_eq!(code, 0);
  assert_eq!(out.trim(), "strict grammar");
  assert_eq!(heard.orders.lock().expect("the orders").len(), 7, "no seventh wake");
  assert_eq!(rows(&client, item).await.len(), 1, "nothing is put to the owner");
}

#[tokio::test]
async fn a_supervisors_word_keeps_the_question_with_it_and_a_rerun_watches_the_kept_notice() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let held = opened(&client, &heard, 1).await;

  service
    .supervisor()
    .reply(held.id, &supervisors_word("deploying it; I answer once it is live"))
    .await
    .expect("a word");
  heard.delivered.lock().expect("the orders delivered").push(1);

  let (code, out, err) = ended(child).await;
  assert_eq!(code, i32::from(bmo_job::job_ask::TALK_EXIT), "{err}");
  assert!(out.starts_with(&format!("The supervisor talked on question #{}", held.id)), "{out}");
  assert!(out.contains("the supervisor:\n> deploying it"), "{out}");

  // The rerun stands on the same question and watches the notice kept on it: delivered, yet the supervisor's word
  // that it settles later stands since, so the question stays with it and nobody is woken, however short the rest.
  let child = asked_waking(&service, &address, item, 10);
  tokio::time::sleep(Duration::from_millis(1_500)).await;
  assert_eq!(heard.orders.lock().expect("the orders").len(), 1, "nobody is told again");
  assert_eq!(rows(&client, item).await.len(), 1, "nothing is put to the owner");

  service
    .supervisor()
    .reply(held.id, &person(None, "live at the commit"))
    .await
    .expect("settled");
  let (code, out, err) = ended(child).await;
  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "live at the commit\n");
}

#[tokio::test]
async fn a_rerun_whose_kept_notice_was_let_go_wakes_the_supervisor_again() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let first = asked(&service, &address, item);
  let held = opened(&client, &heard, 1).await;
  kept_notice(&client, held.id, 1).await;
  drop(first);

  heard.let_go.lock().expect("the orders let go").push(1);
  let child = asked_waking(&service, &address, item, 50);
  let woken = opened(&client, &heard, 2).await;

  assert_eq!((woken.id, woken.addressee), (held.id, QuestionAddressee::Supervisor));
  assert_eq!(rows(&client, item).await.len(), 1, "nothing is put to the owner");
  service
    .supervisor()
    .reply(held.id, &person(None, "loose grammar"))
    .await
    .expect("settled");
  let (code, out, err) = ended(child).await;
  assert_eq!(code, 0, "{err}");
  assert_eq!(out.trim(), "loose grammar");
}

#[tokio::test]
async fn an_acknowledgement_wakes_nobody_and_the_question_stays_with_the_supervisor() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked_waking(&service, &address, item, 10);
  let held = opened(&client, &heard, 1).await;

  service
    .supervisor()
    .reply(held.id, &supervisors_word("deploying it; I answer once it is live"))
    .await
    .expect("a word");
  heard.delivered.lock().expect("the orders delivered").push(1);
  let (code, _, err) = ended(child).await;
  assert_eq!(code, i32::from(bmo_job::job_ask::TALK_EXIT), "{err}");

  let ack = [
    "--talk",
    &held.id.to_string(),
    "--say",
    "Thank you; I wait for the deployment.",
    "--ack",
  ]
  .map(str::to_owned);
  let child = command(&service, &address, &ack)
    .env("BMO_JOB_ASK_WAKE_MS", "10")
    .spawn()
    .expect("the command runs");

  // The acknowledgement is written before anything else would be handed over: once it stands, a notice it handed
  // would stand too.
  for _ in 0..600 {
    let replies = client.replies(held.id, &WaitQuery::default()).await.expect("the replies");

    if replies.items.iter().any(|reply| reply.actor_name == "job-7") {
      break;
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
  }

  assert_eq!(
    heard.orders.lock().expect("the orders").len(),
    1,
    "nobody is handed the acknowledgement"
  );
  assert_eq!(rows(&client, item).await.len(), 1, "nothing is put to the owner");

  service
    .supervisor()
    .reply(held.id, &person(None, "live at the commit"))
    .await
    .expect("settled");
  let (code, out, err) = ended(child).await;
  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "live at the commit\n");
  assert_eq!(
    heard.orders.lock().expect("the orders").len(),
    1,
    "the supervisor's word stood, so nobody was woken"
  );
}

#[tokio::test]
async fn an_acknowledgement_changes_no_words_of_the_question() {
  let service = Service::start().await;
  let (address, heard) = receiver(false).await;
  let refused =
    ["--talk", "1", "--say", "noted", "--ack", "--option", "a", "--option", "b"].map(str::to_owned);
  let (code, out, err) = ended(command(&service, &address, &refused).spawn().expect("runs")).await;

  assert_eq!((code, out.as_str()), (1, ""), "{err}");
  assert!(err.contains("an acknowledgement asks nothing"), "{err}");

  let bare = ["--say", "noted", "--ack"].map(str::to_owned);
  let (code, _, err) = ended(command(&service, &address, &bare).spawn().expect("runs")).await;

  assert_eq!(code, 1, "{err}");
  assert!(err.contains("--ack acknowledges a talk"), "{err}");
  assert!(heard.orders.lock().expect("the orders").is_empty());
}

#[tokio::test]
async fn a_question_the_supervisor_takes_back_from_the_owner_is_settled_by_the_supervisor() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let held = opened(&client, &heard, 1).await;

  to_owner(&service, held.id).await;
  service
    .supervisor()
    .change_question(
      held.id,
      &QuestionChange {
        addressee: Some(QuestionAddressee::Supervisor),
        ..QuestionChange::default()
      },
    )
    .await
    .expect("taken back");
  service
    .supervisor()
    .reply(held.id, &person(None, "deployed; go on"))
    .await
    .expect("settled");

  let (code, out, err) = ended(child).await;
  assert_eq!(code, 0, "{err}");
  assert_eq!(out, "deployed; go on\n");
}

#[tokio::test]
async fn the_supervisor_cannot_answer_what_it_put_to_the_owner_and_its_word_there_is_told_as_its_own() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let item = item(&client).await;
  let child = asked(&service, &address, item);
  let held = opened(&client, &heard, 1).await;

  to_owner(&service, held.id).await;
  let refused = service.supervisor().reply(held.id, &person(Some("strict grammar"), "strict")).await;
  assert_eq!(refused.expect_err("not its to settle").code(), Some("not-the-addressee"));

  service
    .supervisor()
    .reply(held.id, &supervisors_word("he asked me which grammar the parser takes"))
    .await
    .expect("a word");
  let (code, out, err) = ended(child).await;

  assert_eq!(code, i32::from(bmo_job::job_ask::TALK_EXIT), "{err}");
  assert!(out.starts_with("The supervisor talked"), "{out}");
  assert!(!out.contains("The owner"), "the word is the supervisor's, not the owner's: {out}");
}

// The question the running command opened about `item`, once the room has been handed `orders` orders.
async fn opened_about(client: &JobClient, heard: &Heard, orders: usize, item: i64) -> Question {
  for _ in 0..600 {
    if heard.orders.lock().expect("the orders").len() >= orders {
      return rows(client, item).await.last().expect("the question stands").clone();
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
  }

  panic!("the question was never handed to a room");
}

#[tokio::test]
async fn the_question_goes_to_the_items_own_room_else_to_the_room_of_the_item_it_was_raised_from() {
  let service = Service::start().await;
  let client = service.client();
  let (address, heard) = receiver(false).await;
  let raise = |title: &str, scope: &str, raised_by: Option<i64>| NewItem {
    title: title.to_owned(),
    brief: "ask".to_owned(),
    queue_name: "todo".to_owned(),
    scope: Some(scope.to_owned()),
    raised_by,
    ..NewItem::default()
  };
  let first = client.create_item(&raise("the first", "77", None)).await.expect("raised").id;
  let between = client.create_item(&raise("between", "0", Some(first))).await.expect("raised").id;
  let follow_up = client
    .create_item(&raise("a follow-up", "no room", Some(between)))
    .await
    .expect("raised")
    .id;
  let own = client.create_item(&raise("its own room", "88", Some(first))).await.expect("raised").id;

  for (at, item, room) in [(0, follow_up, 77), (1, own, 88)] {
    let child = asked(&service, &address, item);
    let question = opened_about(&client, &heard, at + 1, item).await;

    assert_eq!(question.scope, Some(room.to_string()));
    assert_eq!(heard.orders.lock().expect("the orders")[at]["room"], room);

    service
      .supervisor()
      .reply(question.id, &person(None, "settled"))
      .await
      .expect("the supervisor settles it");

    let (code, _, err) = ended(child).await;
    assert_eq!(code, 0, "{err}");
  }
}
