//! `bmo job ask` from a job's own shell (docs/cli.md): a choice put to the owner in the middle of a job, and its
//! run waiting for a person to settle it. Its run is what waits, holding its checkout, so nothing else is held up.
//!
//! The wait ends on an event and on nothing else: a settling reply a person gave, the question withdrawn, the
//! run told to stop, or the owner talking instead of choosing. No clock settles it. A recommendation is shown to
//! whoever answers and comes back only where a person settled the question with it.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bmo_job_client::{
  ClientFailure, JobClient, NewQuestion, NewReply, Page, Question, QuestionChange, QuestionQuery,
  QuestionRecommendation, QuestionState, Reply, ReplySource, WaitQuery,
};
use serde_json::{Value, json};

use crate::again::{Halt, Unanswered, until_answered};
use crate::outside::outside_refusal;

/// The exit of a call that comes back with talk instead of an answer: the question is still open.
pub const TALK_EXIT: u8 = 3;

/// The verb's forms, printed under every refusal of its words.
pub const USAGE: &str = "usage: bmo job ask --todo <id> --question '<what is to be settled>' --option '<one>' --option '<another>' --recommend <n> --why '<why you ask, in one line>'
       bmo job ask --talk <question> --say '<your answer>' [--option '<one>' --option '<another>' --recommend <n> [--question '<words>'] [--why '<words>']]";

// How far ahead a question's due time stands. It paces the one notice of a question left unanswered and settles
// nothing.
const DUE_MS: i64 = 3_600_000;
// How long one read of the replies is held open; the wait goes on past it.
const HELD_OPEN_MS: i64 = 60_000;
// How long the verb lets a service that did not answer rest before asking it again.
const AGAIN_AFTER: Duration = Duration::from_secs(5);

/// How a wait ended, as the job reads it. A question that cannot be answered any more is an `Err`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ending {
  /// A person settled it: the one line the job carries on with.
  Settled(String),
  /// The owner talked instead of choosing: what the job is handed, and the question is still open.
  Talk(String),
}

/// A new question as the job asked it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NewAsk {
  pub item: Option<i64>,
  pub question: String,
  pub options: Vec<String>,
  /// The option the job would take by itself, by the number the chat shows it under: 1 is the first.
  pub recommend: Option<i64>,
  pub why: String,
  /// Who asks: the question's asker, and the actor of what the job says in a talk.
  pub by: String,
}

/// The job's answer in a talk, and the question put again where the talk changed it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TalkAsk {
  pub question: i64,
  pub say: String,
  pub options: Vec<String>,
  pub recommend: Option<i64>,
  pub text: Option<String>,
  pub why: Option<String>,
}

/// What a call to the receiver came to short of its answer.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReceiverFailure {
  /// Nothing answered at its address, the connection broke, or it answered a 5xx: no answer, so the call is made
  /// again.
  #[error("the receiver gave no answer: {0}")]
  Unanswered(String),
  /// The receiver answered, refusing or with an answer that does not read.
  #[error("{0}")]
  Answered(String),
}

impl Unanswered for ReceiverFailure {
  fn is_unanswered(&self) -> bool {
    matches!(self, Self::Unanswered(_))
  }
}

// A request to the receiver sent, and its answer where its status is a success: a 5xx is no answer, any other
// status a refusal of `route`.
async fn receiver_answer(
  request: reqwest::RequestBuilder,
  route: &str,
) -> Result<reqwest::Response, ReceiverFailure> {
  let answer = request.send().await.map_err(|why| ReceiverFailure::Unanswered(why.to_string()))?;
  let status = answer.status();

  if status.is_server_error() {
    return Err(ReceiverFailure::Unanswered(format!("it answered {} to {route}", status.as_u16())));
  }

  if !status.is_success() {
    let said = answer.text().await.unwrap_or_default();

    return Err(ReceiverFailure::Answered(format!(
      "the receiver answered {} to {route}: {said}",
      status.as_u16()
    )));
  }

  Ok(answer)
}

/// The receiver as this verb reaches it: where the words are handed to the room's supervisor.
#[derive(Clone, Debug)]
pub struct Receiver {
  url: String,
  token: String,
  http: reqwest::Client,
}

impl Receiver {
  pub fn new(url: &str, token: &str) -> Self {
    Self {
      url: url.trim_end_matches('/').to_owned(),
      token: token.trim().to_owned(),
      http: reqwest::Client::new(),
    }
  }

  /// The receiver as the run's environment names it, else as its env file under the data root does.
  pub fn of_process(data_root: &Path) -> Result<Self, String> {
    let named = |name: &str| {
      std::env::var(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    };

    if let Some(url) = named("BMO_RECEIVER_URL") {
      let token = named("BMO_RECEIVER_TOKEN")
        .or_else(|| named("BMO_RECEIVER_TOKEN_FILE").and_then(|file| std::fs::read_to_string(file).ok()))
        .ok_or(
          "BMO_RECEIVER_URL names the receiver but no BMO_RECEIVER_TOKEN or BMO_RECEIVER_TOKEN_FILE does",
        )?;

      return Ok(Self::new(&url, &token));
    }

    let file: PathBuf = data_root.join("env/receiver.env");
    let kept = std::fs::read_to_string(&file)
      .map_err(|why| format!("the receiver's address is named nowhere: {}: {why}", file.display()))?;
    let value = |name: &str| {
      kept.lines().find_map(|line| {
        let (key, value) = line.trim().split_once('=')?;

        (key.trim() == name).then(|| value.trim().trim_matches(['"', '\'']).to_owned())
      })
    };
    let url =
      value("RECEIVER_ADDRESS").ok_or_else(|| format!("{} names no RECEIVER_ADDRESS", file.display()))?;
    let token =
      value("RECEIVER_TOKEN").ok_or_else(|| format!("{} names no RECEIVER_TOKEN", file.display()))?;

    Ok(Self::new(&url, &token))
  }

  /// The owner's private room, the only room his answer can be given in.
  pub async fn owner_room(&self) -> Result<i64, ReceiverFailure> {
    let request = self.http.get(format!("{}/owner", self.url)).bearer_auth(&self.token);
    let answer = receiver_answer(request, "GET /owner").await?;
    let body = answer.bytes().await.map_err(|why| ReceiverFailure::Unanswered(why.to_string()))?;
    let owner: Value = serde_json::from_slice(&body).map_err(|why| {
      ReceiverFailure::Answered(format!("the receiver's GET /owner does not read: {why}"))
    })?;

    owner["room"]
      .as_i64()
      .ok_or_else(|| ReceiverFailure::Answered("the receiver's GET /owner names no room".to_owned()))
  }

  /// An order queued for the room's supervisor: the instruction's number where the receiver answers one.
  pub async fn order(
    &self,
    room: i64,
    kind: &str,
    number: i64,
    summary: &str,
    order: &str,
  ) -> Result<Option<i64>, ReceiverFailure> {
    let request =
      self
        .http
        .post(format!("{}/instructions", self.url))
        .bearer_auth(&self.token)
        .json(&json!({
          "room": room,
          "said": { "order": order },
          "agentName": Value::Null,
          "sort": "job",
          "kind": format!("{kind}:{number}"),
          "summary": summary,
          "events": [format!("{kind}:#{number}")],
          "run": Value::Null,
        }));
    let answer = receiver_answer(request, "POST /instructions").await?;
    let kept: Value = answer.json().await.unwrap_or(Value::Null);

    Ok(kept["id"].as_i64())
  }

  /// A pending order withdrawn, where it has not reached the supervisor yet. It is a courtesy to the supervisor:
  /// the answer stands whether or not this lands.
  pub async fn withdraw(&self, id: i64) {
    let _ = self
      .http
      .delete(format!("{}/instructions/{id}", self.url))
      .bearer_auth(&self.token)
      .send()
      .await;
  }
}

fn refused(reason: &str) -> String {
  format!("ask: {reason}\n{USAGE}")
}

// What a set of options must hold: at least two, each one line and said, the recommendation one of them.
fn options_refusal(options: &[String], recommend: Option<i64>) -> Option<String> {
  if options.len() < 2 {
    return Some("a question puts two options or more: --option '<one>' --option '<another>'.".to_owned());
  }

  if options.iter().any(|option| option.trim().is_empty() || option.contains('\n')) {
    return Some("an option is one line of words.".to_owned());
  }

  let count = i64::try_from(options.len()).unwrap_or(i64::MAX);

  if !recommend.is_some_and(|number| (1..=count).contains(&number)) {
    return Some(format!("--recommend names the option you would take by its number, 1 to {count}."));
  }

  None
}

// The recommendation as the service keeps it: the option's words and why it is asked.
fn recommendation(options: &[String], recommend: i64, why: &str) -> Option<QuestionRecommendation> {
  let at = usize::try_from(recommend - 1).ok()?;

  options.get(at).map(|answer| QuestionRecommendation {
    answer: answer.clone(),
    why: why.to_owned(),
  })
}

// The short marker a turn opens on: the notice it names, and its number. What to do with a notice stands in the
// receiving session's own instructions, never in the turn.
fn notice(kind: &str, id: i64) -> String {
  format!("{kind} #{id}")
}

// The item a turn is about: its number and, where it could be read, its title.
fn item_line(item: i64, title: Option<&str>) -> String {
  match (item > 0, title) {
    (true, Some(title)) => format!("item #{item}: {title}"),
    (true, None) => format!("item #{item}"),
    (false, _) => "item: none".to_owned(),
  }
}

// The question as data: its words, its options numbered as the owner answers them, and why it is asked.
fn question_data(text: &str, options: &[String], recommend: i64, why: &str) -> String {
  let lines: Vec<String> = options
    .iter()
    .enumerate()
    .map(|(at, option)| {
      let mark = if i64::try_from(at + 1) == Ok(recommend) { " (recommended)" } else { "" };

      format!("{}. {option}{mark}", at + 1)
    })
    .collect();

  format!("{text}\n\n{}\n\nwhy: {why}", lines.join("\n"))
}

// The question as the room's supervisor is handed it: the notice, the item it is about and the question's data.
fn question_order(
  item: i64,
  title: Option<&str>,
  id: i64,
  text: &str,
  options: &[String],
  recommend: i64,
  why: &str,
) -> String {
  format!(
    "{}\n{}\n\n{}",
    notice("question-awaits-answer", id),
    item_line(item, title),
    question_data(text, options, recommend, why),
  )
}

// The job's answer in a talk, as the room's supervisor is handed it: the notice, the item and the words, with the
// question's data again where the talk changed it.
fn talk_order(item: i64, title: Option<&str>, id: i64, say: &str, put_again: Option<String>) -> String {
  let again = put_again.map_or_else(String::new, |data| format!("\n\nquestion now stands as:\n{data}"));

  format!("{}\n{}\n\n{say}{again}", notice("question-talk", id), item_line(item, title))
}

// What the waiting job is handed when the owner talks: his words as he wrote them, and the call that answers him.
fn heard_message(id: i64, heard: &[Reply]) -> String {
  let quoted = heard
    .iter()
    .map(|line| line.text.split('\n').map(|part| format!("> {part}")).collect::<Vec<_>>().join("\n"))
    .collect::<Vec<_>>()
    .join("\n");

  format!(
    "The owner talked on question #{id} instead of choosing, and it is still open:\n{quoted}\n\nAnswer him, and wait on it again, with:\n  bmo job ask --talk {id} --say '<your answer>'\nWhere the talk changed the choice, put it again in the same call: --option for every option anew and --recommend, with --question or --why where those changed."
  )
}

/// The one line a settling reply comes back as, the option taken or the person's own words, and a refusal of one
/// no person gave: only a person's answer is ever printed as an answer.
pub fn settled_line(reply: &Reply) -> Result<String, String> {
  if reply.source != ReplySource::Person {
    return Err(format!(
      "ask: question #{} was closed by something no person said, so it comes back with no answer",
      reply.question_id
    ));
  }

  let said = reply.choice.as_deref().unwrap_or(&reply.text);

  Ok(said.split_whitespace().collect::<Vec<_>>().join(" "))
}

// Whether a failure says the service no longer holds the question, rather than that it did not answer now.
fn gone(failure: &ClientFailure) -> bool {
  matches!(failure, ClientFailure::Answered { status: 404, .. })
}

async fn closed(client: &JobClient, id: i64, state: QuestionState) -> Result<(), ClientFailure> {
  let change = QuestionChange {
    state: Some(state),
    ..QuestionChange::default()
  };

  client.change_question(id, &change).await.map(drop)
}

// The question opened once. Where opening it went unanswered, the try may have landed, so the room's open
// questions are read before it is opened again, and one of the same asker, item, words and options is the one
// opened (docs/cli.md, 「`bmo-job job ask`」).
async fn opened_once(
  client: &JobClient,
  asked: &NewQuestion,
  stop: std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<Question, Halt<ClientFailure>> {
  let mut tried = false;

  until_answered("ask", stop, || {
    let again = std::mem::replace(&mut tried, true);

    async move {
      if again && let Some(landed) = already_opened(client, asked).await? {
        return Ok(landed);
      }

      client.ask(asked).await
    }
  })
  .await
}

async fn already_opened(
  client: &JobClient,
  asked: &NewQuestion,
) -> Result<Option<Question>, ClientFailure> {
  let query = QuestionQuery {
    scope: asked.scope.clone(),
    item_id: asked.item_id,
    state: Some("open".to_owned()),
    limit: Some(500),
    ..QuestionQuery::default()
  };
  let page = client.questions(&query).await?;

  Ok(page.items.into_iter().rev().find(|question| {
    question.asker_name == asked.asker_name
      && question.text == asked.text
      && question.choices == asked.choices
  }))
}

// The item's title, read again while the service gives no answer; none where it answers otherwise.
async fn title_of(
  client: &JobClient,
  item: i64,
  stop: std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<Option<String>, ()> {
  match until_answered("ask", stop, || client.item(item)).await {
    Ok(item) => Ok(Some(item.title)),
    Err(Halt::Answered(_)) => Ok(None),
    Err(Halt::Stopped) => Err(()),
  }
}

/// A new question opened in the owner's private room, handed to its supervisor and waited on. A question that
/// could not be handed over is closed `unsent` and comes back as a failure: no option goes out with it.
pub async fn ask(
  client: &JobClient,
  receiver: &Receiver,
  asked: &NewAsk,
  stop: impl Future<Output = ()>,
) -> Result<Ending, String> {
  if asked.question.trim().is_empty() {
    return Err(refused("say what is to be settled: --question '<what is to be settled>'."));
  }

  if let Some(refusal) = options_refusal(&asked.options, asked.recommend) {
    return Err(refused(&refusal));
  }

  if asked.why.trim().is_empty() {
    return Err(refused("say why you cannot settle it yourself: --why '<why you ask, in one line>'."));
  }

  let Some(item) = asked.item.filter(|item| *item > 0) else {
    return Err(refused("name the item it is asked about: --todo <id>."));
  };
  let recommend = asked.recommend.unwrap_or(1);
  let mut texts = vec![asked.question.as_str(), asked.why.as_str()];

  texts.extend(asked.options.iter().map(String::as_str));

  if let Some(refusal) = outside_refusal(&texts) {
    return Err(refused(&refusal));
  }

  let mut stop = std::pin::pin!(stop);

  // Nothing is opened until the room it goes to is known, so a receiver that refuses to name it leaves no
  // question.
  let room =
    until_answered("ask", stop.as_mut(), || receiver.owner_room())
      .await
      .map_err(|halt| match halt {
        Halt::Answered(why) => format!("ask: the question was not put, and nothing was opened: {why}"),
        Halt::Stopped => "ask: the run was told to stop, and nothing was opened".to_owned(),
      })?;
  let opening = NewQuestion {
    item_id: Some(item),
    asker_name: asked.by.clone(),
    scope: Some(room.to_string()),
    text: asked.question.clone(),
    choices: asked.options.clone(),
    recommendation: recommendation(&asked.options, recommend, &asked.why),
    wait_ms: Some(DUE_MS),
    told_by_asker: true,
  };
  let question = opened_once(client, &opening, stop.as_mut()).await.map_err(|halt| match halt {
    Halt::Answered(why) => format!("ask: the question was not opened: {why}"),
    Halt::Stopped => "ask: the run was told to stop, and nothing was opened".to_owned(),
  })?;
  let Ok(title) = title_of(client, item, stop.as_mut()).await else {
    return stopped(client, receiver, question.id, None).await;
  };
  let order = question_order(
    item,
    title.as_deref(),
    question.id,
    &asked.question,
    &asked.options,
    recommend,
    &asked.why,
  );
  let summary = format!("Question #{} awaits the owner", question.id);
  let ordered = until_answered("ask", stop.as_mut(), || {
    receiver.order(room, "question-awaits-answer", question.id, &summary, &order)
  })
  .await;
  let told = match ordered {
    Ok(told) => told,
    Err(Halt::Stopped) => return stopped(client, receiver, question.id, None).await,
    Err(Halt::Answered(why)) => {
      let left =
        match until_answered("ask", stop.as_mut(), || closed(client, question.id, QuestionState::Unsent))
          .await
        {
          Ok(()) => String::new(),
          Err(Halt::Stopped) => return stopped(client, receiver, question.id, None).await,
          Err(Halt::Answered(failed)) => format!("; closing it failed too: {failed}"),
        };

      return Err(format!(
        "ask: question #{} could not be put to the owner, so nothing will ever answer it, and it is closed unsent: {why}{left}",
        question.id
      ));
    }
  };

  wait_on(client, receiver, question, told, stop).await
}

/// The job's answer in a talk put in the owner's room, the question put again where the answer changed it, and
/// the same question waited on again. A question settled while the job was writing is not answered into: the wait
/// finds it settled at once. A talk that could not be handed over leaves the question open and fails, for the job
/// to say again.
pub async fn talk(
  client: &JobClient,
  receiver: &Receiver,
  asked: &TalkAsk,
  stop: impl Future<Output = ()>,
) -> Result<Ending, String> {
  if asked.say.trim().is_empty() {
    return Err(refused("say what you answer him: --say '<your answer>'."));
  }

  if let Some(refusal) = outside_refusal(&[asked.say.as_str()]) {
    return Err(refused(&refusal));
  }

  let mut stop = std::pin::pin!(stop);
  let question = match until_answered("ask", stop.as_mut(), || client.question(asked.question)).await {
    Ok(question) => question,
    Err(Halt::Answered(why)) => return Err(format!("ask: question #{}: {why}", asked.question)),
    Err(Halt::Stopped) => return stopped(client, receiver, asked.question, None).await,
  };

  if !matches!(question.state, QuestionState::Open | QuestionState::Paused) {
    return wait_on(client, receiver, question, None, stop).await;
  }

  let room = question
    .scope
    .as_deref()
    .and_then(|scope| scope.parse::<i64>().ok())
    .ok_or_else(|| format!("ask: question #{} stands in no room, so it takes no talk", question.id))?;
  let item = question.item_id.unwrap_or_default();
  let title = match item > 0 {
    true => match title_of(client, item, stop.as_mut()).await {
      Ok(title) => title,
      Err(()) => return stopped(client, receiver, question.id, None).await,
    },
    false => None,
  };
  let mut put_again = None;
  let mut question = question;

  if !asked.options.is_empty() {
    if let Some(refusal) = options_refusal(&asked.options, asked.recommend) {
      return Err(refused(&refusal));
    }

    let recommend = asked.recommend.unwrap_or(1);
    let text = asked.text.clone().unwrap_or_else(|| question.text.clone());
    let why = asked
      .why
      .clone()
      .or_else(|| question.recommendation.as_ref().map(|kept| kept.why.clone()))
      .unwrap_or_default();

    if why.trim().is_empty() {
      return Err(refused("say why you cannot settle it yourself: --why '<why you ask, in one line>'."));
    }

    let mut texts = vec![text.as_str(), why.as_str()];

    texts.extend(asked.options.iter().map(String::as_str));

    if let Some(refusal) = outside_refusal(&texts) {
      return Err(refused(&refusal));
    }

    let change = QuestionChange {
      text: Some(text.clone()),
      choices: Some(asked.options.clone()),
      recommendation: recommendation(&asked.options, recommend, &why),
      ..QuestionChange::default()
    };

    question =
      match until_answered("ask", stop.as_mut(), || client.change_question(question.id, &change)).await {
        Ok(changed) => changed,
        Err(Halt::Answered(why)) => return Err(format!("ask: {why}")),
        Err(Halt::Stopped) => return stopped(client, receiver, question.id, None).await,
      };
    put_again = Some(question_data(&text, &asked.options, recommend, &why));
  }

  let order = talk_order(item, title.as_deref(), question.id, &asked.say, put_again);
  let summary = format!("Question #{} goes on in talk", question.id);

  let ordered = until_answered("ask", stop.as_mut(), || {
    receiver.order(room, "question-talk", question.id, &summary, &order)
  })
  .await;

  match ordered {
    Ok(_) => {}
    Err(Halt::Answered(why)) => {
      return Err(format!(
        "ask: your answer did not reach the owner, and question #{} stays open; say it again: {why}",
        question.id
      ));
    }
    Err(Halt::Stopped) => return stopped(client, receiver, question.id, None).await,
  }

  let said = NewReply {
    choice: None,
    text: asked.say.clone(),
    actor_name: question.asker_name.clone(),
    settles: Some(false),
  };

  match until_answered("ask", stop.as_mut(), || client.reply(question.id, &said)).await {
    Ok(_) => {}
    Err(Halt::Answered(why)) => return Err(format!("ask: {why}")),
    Err(Halt::Stopped) => return stopped(client, receiver, question.id, None).await,
  }

  wait_on(client, receiver, question, None, stop).await
}

// The run told to stop: the question withdrawn and what was queued for it taken back.
async fn stopped(
  client: &JobClient,
  receiver: &Receiver,
  id: i64,
  told: Option<i64>,
) -> Result<Ending, String> {
  let closing = closed(client, id, QuestionState::Withdrawn).await;

  if let Some(told) = told {
    receiver.withdraw(told).await;
  }

  let left = closing
    .err()
    .map_or_else(String::new, |failed| format!("; withdrawing it failed: {failed}"));

  Err(format!(
    "ask: the run was told to stop, and question #{id} is withdrawn unanswered{left}"
  ))
}

// The question and its replies read until a person settles it or the owner talks. A stop withdraws it. A service
// that does not answer is asked again, since a restart is no answer, and so is keeping where the job heard
// through; a service that no longer holds the question ends the wait.
async fn wait_on(
  client: &JobClient,
  receiver: &Receiver,
  question: Question,
  told: Option<i64>,
  stop: impl Future<Output = ()>,
) -> Result<Ending, String> {
  let id = question.id;
  let asker = question.asker_name.clone();
  let mut after = question.heard_through;
  let mut stop = std::pin::pin!(stop);

  loop {
    let looked = tokio::select! {
      () = &mut stop => return stopped(client, receiver, id, told).await,
      looked = look(client, id, after) => looked,
    };
    let page = match looked {
      Ok(page) => page,
      Err(failure) if gone(&failure) => {
        return Err(format!("ask: the service no longer holds question #{id}"));
      }
      Err(failure) => {
        eprintln!("ask: {failure}; asking again");
        tokio::select! {
          () = &mut stop => return stopped(client, receiver, id, told).await,
          () = tokio::time::sleep(AGAIN_AFTER) => {}
        }

        continue;
      }
    };
    let Some(page) = page else {
      return Err(format!("ask: question #{id} was closed unanswered, and no answer is coming"));
    };
    let mut heard = vec![];

    for reply in page.items {
      after = Some(reply.id);

      if reply.settles {
        if let Some(told) = told {
          receiver.withdraw(told).await;
        }

        return settled_line(&reply).map(Ending::Settled);
      }

      if reply.actor_name != asker {
        heard.push(reply);
      }
    }

    if !heard.is_empty() {
      let change = QuestionChange {
        heard_through: after,
        ..QuestionChange::default()
      };

      return match until_answered("ask", stop.as_mut(), || client.change_question(id, &change)).await {
        Ok(_) => Ok(Ending::Talk(heard_message(id, &heard))),
        Err(Halt::Answered(why)) => Err(format!("ask: {why}")),
        Err(Halt::Stopped) => stopped(client, receiver, id, told).await,
      };
    }
  }
}

// One look at the question: none where it was closed unanswered, else the replies past the cursor, the read held
// open for a while where none stands yet.
async fn look(
  client: &JobClient,
  id: i64,
  after: Option<i64>,
) -> Result<Option<Page<Reply>>, ClientFailure> {
  let current = client.question(id).await?;

  if matches!(current.state, QuestionState::Withdrawn | QuestionState::Unsent) {
    return Ok(None);
  }

  let query = WaitQuery {
    wait_ms: Some(HELD_OPEN_MS),
    after,
    limit: None,
  };

  client.replies(id, &query).await.map(Some)
}
