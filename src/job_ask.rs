//! `bmo job ask` from a job's own shell (docs/cli.md): a question put to the job's supervisor in the middle of a
//! job, in the item's room, which settles it itself or puts it to the owner, and the job's run waiting for it to be
//! settled. Its run is what waits, holding its checkout, so nothing else is held up.
//!
//! The wait ends on an event and on nothing else: a settling reply the supervisor or a person gave, the question
//! withdrawn, the run told to stop, or the supervisor or the owner talking instead of settling. No clock settles
//! it. A recommendation is shown to whoever answers and comes back only where a person settled the question with
//! it. A supervisor's notice the receiver lets go untaken wakes the supervisor again, and the question stays with
//! it.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bmo_job_client::{
  ClientFailure, JobClient, NewQuestion, NewReply, Page, Question, QuestionAddressee, QuestionChange,
  QuestionQuery, QuestionRecommendation, QuestionState, Reply, ReplySource, WaitQuery,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::again::{Halt, Rests, Unanswered, until_answered};
use crate::ask_words::{say_refusal, words_refusal};
use crate::outside::outside_refusal;

/// The exit of a call that comes back with talk instead of an answer: the question is still open.
pub const TALK_EXIT: u8 = 3;

/// The verb's forms, printed under every refusal of its words.
pub const USAGE: &str = "usage: bmo job ask --todo <id> --question '<what is to be settled or done>' --why '<why you cannot settle it yourself, in one line>' [--option '<one>' --option '<another>' --recommend <n>]
       bmo job ask --talk <question> --say '<your answer>' [--option '<one>' --option '<another>' --recommend <n> [--question '<words>'] [--why '<words>']]
       bmo job ask --talk <question> --say '<words that ask nothing>' --ack
       bmo job ask --show <question>
       bmo job ask --todo <id> --show";

// How long one read of the replies is held open; the wait goes on past it. While the supervisor's notice is
// watched, a read is held shorter, so a turn that ended with no word on the question is read soon after.
const HELD_OPEN_MS: i64 = 60_000;
const WATCHING_MS: i64 = 15_000;

// How often a supervisor that did not take a question up is woken again, the first rest before a wake when the
// run's environment names none, and the variable that names one (docs/cli.md, 「`bmo-job job ask`」).
const MOST_WAKES: u32 = 6;
const FIRST_WAKE_MS: u64 = 60_000;
const WAKE_VARIABLE: &str = "BMO_JOB_ASK_WAKE_MS";

// The states a question an ask stands as may be in: every one but those that will never be answered.
const STANDING: &str = "open,paused,answered";

// The most of a list one read takes.
const MOST: i64 = 500;

// The most items the search for a question's room reads, the item asked about and those it was raised from.
const MOST_RAISERS: usize = 16;

/// How a wait ended, as the job reads it. A question that cannot be answered any more is an `Err`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ending {
  /// The supervisor or a person settled it: the answer the job carries on with.
  Settled(String),
  /// The owner's due time passed and its recommendation settled it: the one line the job carries on with, and the
  /// question's number.
  SettledOnRecommendation { question: i64, line: String },
  /// A person settled the same question before this ask: the line it was settled with, and its number. Nothing
  /// was opened and nobody was told.
  SettledBefore { question: i64, line: String },
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
  /// The words ask nothing, an acknowledgement such as that the job waits: written on the question and handed to
  /// nobody.
  pub ack: bool,
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

/// Where an order to the room's supervisor stands (the receiver's `GET /instructions/{id}`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Notice {
  /// Pending or handed: no turn has finished with it yet.
  Waiting,
  /// The round that carried it ended after the turn that took it ended.
  Delivered,
  /// Let go untaken: no turn will ever take it.
  LetGo,
}

/// The notice a question was last put before the room's supervisor with, the last reply before that notice, and how
/// many times the supervisor was woken again on it: kept on the question as its `toldAs`, so a rerun of the same ask
/// watches it, and counts its wakes, as the first run did.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watch {
  pub notice: i64,
  pub since: Option<i64>,
  #[serde(default)]
  pub wakes: u32,
}

impl Watch {
  /// The watch a question keeps, where it keeps one this verb wrote.
  pub fn kept(question: &Question) -> Option<Self> {
    question.told_as.as_deref().and_then(|kept| serde_json::from_str(kept).ok())
  }
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

  /// Where an order stands as the receiver reads it back: still waiting for a turn, delivered once the round that
  /// carried it ended after its turn ended, or let go untaken, which it answers as gone.
  pub async fn notice(&self, id: i64) -> Result<Notice, ReceiverFailure> {
    let answer = self
      .http
      .get(format!("{}/instructions/{id}", self.url))
      .bearer_auth(&self.token)
      .send()
      .await
      .map_err(|why| ReceiverFailure::Unanswered(why.to_string()))?;
    let status = answer.status();

    if status == reqwest::StatusCode::NOT_FOUND {
      return Ok(Notice::LetGo);
    }

    if status.is_server_error() {
      return Err(ReceiverFailure::Unanswered(format!(
        "it answered {} to GET /instructions",
        status.as_u16()
      )));
    }

    if !status.is_success() {
      return Err(ReceiverFailure::Answered(format!(
        "the receiver answered {} to GET /instructions",
        status.as_u16()
      )));
    }

    let kept: Value = answer.json().await.map_err(|why| ReceiverFailure::Unanswered(why.to_string()))?;

    Ok(match kept["state"].as_str() {
      Some("delivered") => Notice::Delivered,
      _ => Notice::Waiting,
    })
  }
}

fn refused(reason: &str) -> String {
  format!("ask: {reason}\n{USAGE}")
}

// What a set of options must hold: none, for an open question, which takes no recommendation; or at least two, each
// one line and said, the recommendation one of them.
fn options_refusal(options: &[String], recommend: Option<i64>) -> Option<String> {
  if options.is_empty() {
    return recommend
      .map(|_| "--recommend names one of the options, and an open question puts none.".to_owned());
  }

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

// The question as data: its words, its options numbered as the owner answers them or the word that it is open, and
// why it is asked.
fn question_data(text: &str, options: &[String], recommend: i64, why: &str) -> String {
  if options.is_empty() {
    return format!("{text}\n\nchoices: an open answer\n\nwhy: {why}");
  }

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

// The tools a supervisor's order names: the ones that settle the question, talk on it and put it to the owner
// (docs/api.md, 「The tools」).
const SUPERVISOR_TOOLS: &str = "settle: job_reply with settles true; talk: job_reply with settles false; \
  to the owner: job_question_change with addressee person";

// The kind of order a question is handed to the room's supervisor under: one it holds itself, or one put to the
// owner that it puts before him.
fn order_kind(addressee: QuestionAddressee) -> &'static str {
  match addressee {
    QuestionAddressee::Supervisor => "question-for-supervisor",
    QuestionAddressee::Person => "question-awaits-answer",
  }
}

// The question as the room's supervisor is handed it: the notice, the item it is about and the question's data,
// and for one it holds itself the tools that answer it.
fn question_order(
  item: i64,
  title: Option<&str>,
  question: &Question,
  recommend: i64,
  why: &str,
) -> String {
  let tools = match question.addressee {
    QuestionAddressee::Supervisor => format!("\n\n{SUPERVISOR_TOOLS}"),
    QuestionAddressee::Person => String::new(),
  };

  format!(
    "{}\n{}\n\n{}{tools}",
    notice(order_kind(question.addressee), question.id),
    item_line(item, title),
    question_data(&question.text, &question.choices, recommend, why),
  )
}

// Who answers a question as the job is told of it.
fn answerer(addressee: QuestionAddressee) -> &'static str {
  match addressee {
    QuestionAddressee::Supervisor => "supervisor",
    QuestionAddressee::Person => "owner",
  }
}

// The job's answer in a talk, as the room's supervisor is handed it: the notice, the item and the words, with the
// question's data again where the talk changed it.
fn talk_order(item: i64, title: Option<&str>, id: i64, say: &str, put_again: Option<String>) -> String {
  let again = put_again.map_or_else(String::new, |data| format!("\n\nquestion now stands as:\n{data}"));

  format!("{}\n{}\n\n{say}{again}", notice("question-talk", id), item_line(item, title))
}

// The voice a reply was given in, as the job is told of it.
fn voice(reply: &Reply) -> &'static str {
  match reply.source {
    ReplySource::Supervisor => "supervisor",
    ReplySource::Person | ReplySource::Recommendation => "owner",
  }
}

// What the waiting job is handed when the supervisor or the owner talks: each word under the voice that said it,
// and the call that answers them.
fn heard_message(id: i64, heard: &[Reply]) -> String {
  let mut voices: Vec<&str> = vec![];

  for reply in heard {
    if !voices.contains(&voice(reply)) {
      voices.push(voice(reply));
    }
  }

  let who = voices.join(" and the ");
  let quoted = heard
    .iter()
    .map(|line| {
      let words = line.text.split('\n').map(|part| format!("> {part}")).collect::<Vec<_>>().join("\n");

      format!("the {}:\n{words}", voice(line))
    })
    .collect::<Vec<_>>()
    .join("\n");

  format!(
    "The {who} talked on question #{id} instead of settling it, and it is still open:\n{quoted}\n\nAnswer, and wait on it again, with:\n  bmo job ask --talk {id} --say '<your answer>'\nWhere the talk changed the choice, put it again in the same call: --option for every option anew and --recommend, with --question or --why where those changed.\nWhere the supervisor only says it is still at work and asks nothing, run the same ask again, or say that you wait with --talk {id} --say '<words>' --ack, which wakes nobody: either waits on the question as it stands."
  )
}

/// What a settling reply comes back as, by the voice it was given in: the supervisor's answer as it wrote it, a
/// result of several lines among it, and a person's one line, the option taken or his own words, which the
/// recommendation that settled a question the owner left past its due time comes back as too.
pub fn settled_answer(reply: &Reply) -> String {
  let said = reply.choice.as_deref().unwrap_or(&reply.text);

  match reply.source {
    ReplySource::Supervisor => said.trim().to_owned(),
    ReplySource::Person | ReplySource::Recommendation => {
      said.split_whitespace().collect::<Vec<_>>().join(" ")
    }
  }
}

// How the wait ends on a settling reply: its answer, said apart where the recommendation gave it.
fn settled_ending(reply: &Reply) -> Ending {
  let line = settled_answer(reply);

  match reply.source {
    ReplySource::Recommendation => Ending::SettledOnRecommendation {
      question: reply.question_id,
      line,
    },
    ReplySource::Person | ReplySource::Supervisor => Ending::Settled(line),
  }
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

// The question opened once. Where opening it went unanswered, the try may have landed, so the item's open
// questions are read before it is opened again, and one of the same asker, words and options is the one opened
// (docs/cli.md, 「`bmo-job job ask`」).
async fn opened_once(
  client: &JobClient,
  asked: &NewQuestion,
  stop: std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<Question, Halt<ClientFailure>> {
  let mut tried = false;

  until_answered("ask", stop, || {
    let again = std::mem::replace(&mut tried, true);

    async move {
      if again && let Some(landed) = same_question(client, asked, "open").await? {
        return Ok(landed);
      }

      client.ask(asked).await
    }
  })
  .await
}

// The last question of the item in one of `states` that the same asker asked in the same words and options.
async fn same_question(
  client: &JobClient,
  asked: &NewQuestion,
  states: &str,
) -> Result<Option<Question>, ClientFailure> {
  let Some(item) = asked.item_id else {
    return Ok(None);
  };
  let questions = item_questions(client, item, Some(states)).await?;

  Ok(questions.into_iter().rev().find(|question| {
    question.asker_name == asked.asker_name
      && question.text == asked.text
      && question.choices == asked.choices
  }))
}

// The reply that settled a question, where one did.
async fn settling_reply(client: &JobClient, id: i64) -> Result<Option<Reply>, ClientFailure> {
  let mut query = WaitQuery {
    wait_ms: None,
    after: None,
    limit: Some(MOST),
  };

  loop {
    let page = client.replies(id, &query).await?;

    query.after = page.items.last().map(|reply| reply.id);

    if let Some(settling) = page.items.into_iter().find(|reply| reply.settles) {
      return Ok(Some(settling));
    }

    if !page.more {
      return Ok(None);
    }
  }
}

// The question an ask already stands as: one settled comes back with its answer, and one still waiting is waited
// on as it stands, told to nobody again.
async fn stood(
  client: &JobClient,
  receiver: &Receiver,
  question: Question,
  stop: impl Future<Output = ()>,
) -> Result<Ending, String> {
  let id = question.id;

  if question.state != QuestionState::Answered {
    eprintln!("ask: question #{id} was asked before and still waits; waiting on it again");

    let watch = Watch::kept(&question);

    return wait_on(client, receiver, question, watch, stop).await;
  }

  let mut stop = std::pin::pin!(stop);

  match until_answered("ask", stop.as_mut(), || settling_reply(client, id)).await {
    Ok(Some(reply)) => Ok(Ending::SettledBefore {
      question: id,
      line: settled_answer(&reply),
    }),
    Ok(None) => Err(format!("ask: question #{id} reads answered, yet no reply settled it")),
    Err(Halt::Answered(why)) => {
      Err(format!("ask: question #{id} was settled before, and its answer was not read: {why}"))
    }
    Err(Halt::Stopped) => {
      Err(format!("ask: the run was told to stop before question #{id}'s settled answer was read"))
    }
  }
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

// A scope read as a room's number: an integer other than zero.
fn room_of(scope: Option<&str>) -> Option<i64> {
  scope.and_then(|scope| scope.trim().parse::<i64>().ok()).filter(|room| *room != 0)
}

// The room the item's own scope names, else the one the nearest item it was raised from names, following `raisedBy`
// through at most `MOST_RAISERS` items and stopping at one met twice or one the service refuses to read
// (docs/cli.md, 「`bmo-job job ask`」); none where no such item names one, and a failure where the run was told to
// stop.
async fn item_room(
  client: &JobClient,
  item: i64,
  mut stop: std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<Option<i64>, ()> {
  let mut met = Vec::new();
  let mut next = Some(item);

  while let Some(id) = next.filter(|id| met.len() < MOST_RAISERS && !met.contains(id)) {
    met.push(id);

    let read = match until_answered("ask", stop.as_mut(), || client.item(id)).await {
      Ok(read) => read,
      Err(Halt::Answered(_)) => return Ok(None),
      Err(Halt::Stopped) => return Err(()),
    };

    if let Some(room) = room_of(read.scope.as_deref()) {
      return Ok(Some(room));
    }

    next = read.raised_by;
  }

  Ok(None)
}

/// A new question opened in the item's room for its supervisor, handed to that supervisor and waited on.
/// A question that could not be handed over is closed `unsent` and comes back as a failure: no option goes out with
/// it.
pub async fn ask(
  client: &JobClient,
  receiver: &Receiver,
  asked: &NewAsk,
  stop: impl Future<Output = ()>,
) -> Result<Ending, String> {
  if asked.question.trim().is_empty() {
    return Err(refused(
      "say what is to be settled or done: --question '<what is to be settled or done>'.",
    ));
  }

  if let Some(refusal) = options_refusal(&asked.options, asked.recommend) {
    return Err(refused(&refusal));
  }

  if asked.why.trim().is_empty() {
    return Err(refused(
      "say why you cannot settle it yourself: --why '<why you cannot settle it yourself, in one line>'.",
    ));
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

  if let Some(refusal) = words_refusal(&asked.question, &asked.options, &asked.why) {
    return Err(refused(&refusal));
  }

  let mut stop = std::pin::pin!(stop);
  let mut opening = NewQuestion {
    item_id: Some(item),
    asker_name: asked.by.clone(),
    scope: None,
    // A job asks its supervisor, which settles it or puts it to the owner (docs/cli.md, 「`bmo-job job ask`」).
    addressee: Some(QuestionAddressee::Supervisor),
    text: asked.question.clone(),
    choices: asked.options.clone(),
    recommendation: recommendation(&asked.options, recommend, &asked.why),
    // No wait is named: the question goes to the supervisor, which holds it with no due time, and one it puts to
    // the owner has none either, so no clock settles it in his place (docs/api.md, 「The due time」).
    wait_ms: None,
    told_by_asker: true,
  };

  // One question is one row, told once: one the same ask already stands as is not opened again.
  let standing = until_answered("ask", stop.as_mut(), || same_question(client, &opening, STANDING))
    .await
    .map_err(|halt| match halt {
      Halt::Answered(why) => format!("ask: the question was not put, and nothing was opened: {why}"),
      Halt::Stopped => "ask: the run was told to stop, and nothing was opened".to_owned(),
    })?;

  if let Some(question) = standing {
    return stood(client, receiver, question, stop).await;
  }

  // Nothing is opened until the room it goes to is known: the item's, else the owner's private room, so a
  // receiver that refuses to name that leaves no question.
  let stopped = || "ask: the run was told to stop, and nothing was opened".to_owned();
  let room = match item_room(client, item, stop.as_mut()).await {
    Ok(Some(room)) => room,
    Ok(None) => {
      until_answered("ask", stop.as_mut(), || receiver.owner_room())
        .await
        .map_err(|halt| match halt {
          Halt::Answered(why) => format!("ask: the question was not put, and nothing was opened: {why}"),
          Halt::Stopped => stopped(),
        })?
    }
    Err(()) => return Err(stopped()),
  };
  opening.scope = Some(room.to_string());

  let (question, watch) =
    put(client, receiver, &opening, room, recommend, &asked.why, stop.as_mut()).await?;

  wait_on(client, receiver, question, watch, stop).await
}

// The question opened and handed to the room's supervisor under the order its addressee takes: the question and
// the notice it was handed with where the receiver answered one, kept on a question the supervisor holds. A question
// that could not be handed over is closed `unsent`.
async fn put(
  client: &JobClient,
  receiver: &Receiver,
  opening: &NewQuestion,
  room: i64,
  recommend: i64,
  why: &str,
  mut stop: std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<(Question, Option<Watch>), String> {
  let question = opened_once(client, opening, stop.as_mut()).await.map_err(|halt| match halt {
    Halt::Answered(why) => format!("ask: the question was not opened: {why}"),
    Halt::Stopped => "ask: the run was told to stop, and nothing was opened".to_owned(),
  })?;
  let item = question.item_id.unwrap_or_default();
  let title = match item > 0 {
    true => match title_of(client, item, stop.as_mut()).await {
      Ok(title) => title,
      Err(()) => return Err(withdrawn(client, receiver, question.id, None).await),
    },
    false => None,
  };
  let order = question_order(item, title.as_deref(), &question, recommend, why);
  let summary = format!("Question #{} awaits the {}", question.id, answerer(question.addressee));
  let kind = order_kind(question.addressee);
  let ordered =
    until_answered("ask", stop.as_mut(), || receiver.order(room, kind, question.id, &summary, &order))
      .await;

  match ordered {
    Ok(told) => {
      let watch = told.map(|notice| Watch {
        notice,
        since: None,
        wakes: 0,
      });

      kept_watch(client, &question, watch, stop).await?;

      Ok((question, watch))
    }
    Err(Halt::Stopped) => Err(withdrawn(client, receiver, question.id, None).await),
    Err(Halt::Answered(why)) => {
      let left =
        match until_answered("ask", stop.as_mut(), || closed(client, question.id, QuestionState::Unsent))
          .await
        {
          Ok(()) => String::new(),
          Err(Halt::Stopped) => return Err(withdrawn(client, receiver, question.id, None).await),
          Err(Halt::Answered(failed)) => format!("; closing it failed too: {failed}"),
        };

      Err(format!(
        "ask: question #{} could not be put to the {}, so nothing will ever answer it, and it is closed unsent: {why}{left}",
        question.id,
        answerer(question.addressee)
      ))
    }
  }
}

// The notice a question the supervisor holds was put before it with, kept on the question so a rerun watches it.
// Keeping it is the rerun's help and no condition of this wait: a service that refuses it is said and passed by.
async fn kept_watch(
  client: &JobClient,
  question: &Question,
  watch: Option<Watch>,
  stop: std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<(), String> {
  let Some(watch) = watch.filter(|_| question.addressee == QuestionAddressee::Supervisor) else {
    return Ok(());
  };
  let change = QuestionChange {
    told_as: serde_json::to_string(&watch).ok(),
    ..QuestionChange::default()
  };

  match until_answered("ask", stop, || client.change_question(question.id, &change)).await {
    Ok(_) => Ok(()),
    Err(Halt::Answered(why)) => {
      eprintln!("ask: the notice of question #{} was not kept on it: {why}", question.id);

      Ok(())
    }
    Err(Halt::Stopped) => {
      Err(format!("ask: the run was told to stop while question #{} was put", question.id))
    }
  }
}

// Whether the supervisor did not take up the question its notice put before it: the notice was let go untaken,
// or the round that carried it ended after its turn ended while no word of the supervisor's stands on the question
// past the last reply before the notice (docs/cli.md, 「`bmo-job job ask`」).
async fn not_taken_up(
  client: &JobClient,
  receiver: &Receiver,
  id: i64,
  watch: Watch,
) -> Result<bool, String> {
  match receiver.notice(watch.notice).await.map_err(|why| why.to_string())? {
    Notice::Waiting => Ok(false),
    Notice::LetGo => Ok(true),
    Notice::Delivered => {
      let mut query = WaitQuery {
        wait_ms: None,
        after: watch.since,
        limit: Some(MOST),
      };

      loop {
        let page = client.replies(id, &query).await.map_err(|why| why.to_string())?;

        if page.items.iter().any(|reply| reply.source == ReplySource::Supervisor) {
          return Ok(false);
        }

        query.after = page.items.last().map(|reply| reply.id);

        if !page.more {
          return Ok(true);
        }
      }
    }
  }
}

// The rest before the supervisor is woken again on a question it has been woken on `wakes` times: the first rest
// the run's environment names, or a minute, doubled for each wake before.
fn wake_rest(wakes: u32) -> Duration {
  let first = std::env::var(WAKE_VARIABLE)
    .ok()
    .and_then(|named| named.trim().parse::<u64>().ok())
    .filter(|ms| *ms > 0)
    .unwrap_or(FIRST_WAKE_MS);

  Duration::from_millis(first.saturating_mul(1 << wakes.min(16)))
}

// A question its supervisor did not take up, handed to the room's supervisor anew as `question-for-supervisor`, never
// to the owner (docs/cli.md, 「`bmo-job job ask`」): the watch on the new notice, its count of wakes one more. A
// receiver that refuses the order counts the wake as made, so the next rest is longer.
async fn woken(
  client: &JobClient,
  receiver: &Receiver,
  question: &Question,
  watched: Watch,
  heard_through: Option<i64>,
  mut stop: std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<Watch, String> {
  let id = question.id;
  let wakes = watched.wakes + 1;
  let room = question
    .scope
    .as_deref()
    .and_then(|scope| scope.parse::<i64>().ok())
    .ok_or_else(|| format!("ask: question #{id} stands in no room, so its supervisor cannot be woken"))?;
  let item = question.item_id.unwrap_or_default();
  let title = match item > 0 {
    true => match title_of(client, item, stop.as_mut()).await {
      Ok(title) => title,
      Err(()) => return Err(withdrawn(client, receiver, id, None).await),
    },
    false => None,
  };
  let recommend = question
    .recommendation
    .as_ref()
    .and_then(|kept| question.choices.iter().position(|choice| *choice == kept.answer))
    .and_then(|at| i64::try_from(at + 1).ok())
    .unwrap_or(0);
  let why = question.recommendation.as_ref().map(|kept| kept.why.clone()).unwrap_or_default();
  let order = format!(
    "{}\n\nwoken again: {wakes} of {MOST_WAKES}, as the question was not taken up",
    question_order(item, title.as_deref(), question, recommend, &why)
  );
  let summary = format!("Question #{id} still awaits the supervisor");
  let kind = order_kind(QuestionAddressee::Supervisor);
  let ordered =
    until_answered("ask", stop.as_mut(), || receiver.order(room, kind, id, &summary, &order)).await;
  let since = heard_through.max(watched.since);

  match ordered {
    Ok(Some(notice)) => {
      let watch = Watch {
        notice,
        since,
        wakes,
      };

      kept_watch(client, question, Some(watch), stop).await?;
      eprintln!(
        "ask: the supervisor did not take up question #{id} (its notice was let go untaken, or the turn that took it ended with no word on it), so it is woken again, {wakes} of {MOST_WAKES}; the question stays with it"
      );

      Ok(watch)
    }
    Ok(None) => Ok(Watch { wakes, ..watched }),
    Err(Halt::Answered(why)) => {
      eprintln!("ask: the supervisor could not be woken again on question #{id}: {why}");

      Ok(Watch { wakes, ..watched })
    }
    Err(Halt::Stopped) => Err(withdrawn(client, receiver, id, None).await),
  }
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

  if let Some(refusal) = say_refusal(&asked.say) {
    return Err(refused(&refusal));
  }

  if asked.ack && (!asked.options.is_empty() || asked.text.is_some() || asked.why.is_some()) {
    return Err(refused(
      "an acknowledgement asks nothing and leaves the question's words as they stand: leave out --option, --recommend, --question and --why.",
    ));
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

  if asked.ack {
    return acknowledged(client, receiver, question, &asked.say, stop).await;
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

    if let Some(refusal) = words_refusal(&text, &asked.options, &why) {
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

  let told = match ordered {
    Ok(told) => told,
    Err(Halt::Answered(why)) => {
      return Err(format!(
        "ask: your answer did not reach the room, and question #{} stays open; say it again: {why}",
        question.id
      ));
    }
    Err(Halt::Stopped) => return stopped(client, receiver, question.id, None).await,
  };

  let said = NewReply {
    choice: None,
    text: asked.say.clone(),
    actor_name: question.asker_name.clone(),
    settles: Some(false),
  };

  let reply = match until_answered("ask", stop.as_mut(), || client.reply(question.id, &said)).await {
    Ok(reply) => reply,
    Err(Halt::Answered(why)) => return Err(format!("ask: {why}")),
    Err(Halt::Stopped) => return stopped(client, receiver, question.id, None).await,
  };

  // The job's answer wakes the supervisor's turn on a question it holds, and that turn is watched as the first was.
  let watch = told.map(|notice| Watch {
    notice,
    since: Some(reply.id),
    wakes: 0,
  });

  kept_watch(client, &question, watch, stop.as_mut()).await?;

  let watch = watch.filter(|_| question.addressee == QuestionAddressee::Supervisor);

  wait_on(client, receiver, question, watch, stop).await
}

// The job's words that ask nothing, written on the question and handed to nobody, and the same question waited on
// again: the notice kept on it is watched as it stood, so the acknowledgement wakes nobody and asks no word.
async fn acknowledged(
  client: &JobClient,
  receiver: &Receiver,
  question: Question,
  say: &str,
  mut stop: std::pin::Pin<&mut impl Future<Output = ()>>,
) -> Result<Ending, String> {
  let said = NewReply {
    choice: None,
    text: say.to_owned(),
    actor_name: question.asker_name.clone(),
    settles: Some(false),
  };

  match until_answered("ask", stop.as_mut(), || client.reply(question.id, &said)).await {
    Ok(_) => {}
    Err(Halt::Answered(why)) => return Err(format!("ask: {why}")),
    Err(Halt::Stopped) => return stopped(client, receiver, question.id, None).await,
  }

  let watch = Watch::kept(&question).filter(|_| question.addressee == QuestionAddressee::Supervisor);

  wait_on(client, receiver, question, watch, stop).await
}

// The run told to stop: the question withdrawn and what was queued for it taken back.
async fn stopped(
  client: &JobClient,
  receiver: &Receiver,
  id: i64,
  told: Option<i64>,
) -> Result<Ending, String> {
  Err(withdrawn(client, receiver, id, told).await)
}

// The question withdrawn as the run was told to stop, what was queued for it taken back, and what the job is told.
async fn withdrawn(client: &JobClient, receiver: &Receiver, id: i64, told: Option<i64>) -> String {
  let closing = closed(client, id, QuestionState::Withdrawn).await;

  if let Some(told) = told {
    receiver.withdraw(told).await;
  }

  let left = closing
    .err()
    .map_or_else(String::new, |failed| format!("; withdrawing it failed: {failed}"));

  format!("ask: the run was told to stop, and question #{id} is withdrawn unanswered{left}")
}

// The question and its replies read until the supervisor or a person settles it, or one of them talks. A stop
// withdraws it. While the supervisor holds it, its notice is read too: a supervisor that did not take it up is woken
// again after a rest that doubles with each wake, and past the last wake nobody is; the question stays with the
// supervisor either way. A service that does not answer is asked again, since a restart is no answer, and so is
// keeping where the job heard through; a service that no longer holds the question ends the wait.
async fn wait_on(
  client: &JobClient,
  receiver: &Receiver,
  question: Question,
  watch: Option<Watch>,
  stop: impl Future<Output = ()>,
) -> Result<Ending, String> {
  let mut question = question;
  let mut watch = watch;
  let mut after = question.heard_through;
  let mut stop = std::pin::pin!(stop);
  let mut rests = Rests::default();
  // When the supervisor that did not take the question up is woken again, once a rest has passed.
  let mut wake_at: Option<tokio::time::Instant> = None;

  loop {
    let id = question.id;
    let told = watch.map(|watched| watched.notice);

    if question.addressee == QuestionAddressee::Supervisor
      && let Some(watched) = watch
    {
      let read = tokio::select! {
        () = &mut stop => return stopped(client, receiver, id, told).await,
        read = not_taken_up(client, receiver, id, watched) => read,
      };

      match read {
        Ok(false) => wake_at = None,
        Ok(true) if watched.wakes >= MOST_WAKES => {
          eprintln!(
            "ask: the supervisor was woken {MOST_WAKES} times on question #{id} and has not taken it up; it stays with the supervisor, and nobody is woken again"
          );
          watch = None;

          continue;
        }
        Ok(true) => {
          let at = *wake_at.get_or_insert_with(|| tokio::time::Instant::now() + wake_rest(watched.wakes));

          if tokio::time::Instant::now() >= at {
            watch = Some(woken(client, receiver, &question, watched, after, stop.as_mut()).await?);
            wake_at = None;

            continue;
          }
        }
        Err(why) => {
          eprintln!("ask: the supervisor's notice of question #{id} was not read: {why}; reading it again");
          tokio::select! {
            () = &mut stop => return stopped(client, receiver, id, told).await,
            () = tokio::time::sleep(rests.take()) => {}
          }

          continue;
        }
      }
    }

    // A read is held shorter while the notice is watched, and no longer than until a wake is due.
    let hold = match (watch.is_some(), wake_at) {
      (false, _) => HELD_OPEN_MS,
      (true, None) => WATCHING_MS,
      (true, Some(at)) => {
        let left = at.saturating_duration_since(tokio::time::Instant::now()).as_millis();

        i64::try_from(left).unwrap_or(WATCHING_MS).clamp(1, WATCHING_MS)
      }
    };
    let looked = tokio::select! {
      () = &mut stop => return stopped(client, receiver, id, told).await,
      looked = look(client, id, after, question.addressee, hold) => looked,
    };
    let (current, page) = match looked {
      Ok(Some(looked)) => {
        rests = Rests::default();
        looked
      }
      Ok(None) => {
        return Err(format!("ask: question #{id} was closed unanswered, and no answer is coming"));
      }
      Err(failure) if gone(&failure) => {
        return Err(format!("ask: the service no longer holds question #{id}"));
      }
      Err(failure) => {
        eprintln!("ask: {failure}; asking again");
        tokio::select! {
          () = &mut stop => return stopped(client, receiver, id, told).await,
          () = tokio::time::sleep(rests.take()) => {}
        }

        continue;
      }
    };

    // The supervisor put it to the owner: its notice is done with, and his answer is waited on.
    if question.addressee == QuestionAddressee::Supervisor && current.addressee == QuestionAddressee::Person
    {
      if let Some(notice) = watch.take().map(|watched| watched.notice) {
        receiver.withdraw(notice).await;
      }

      eprintln!("ask: the supervisor put question #{id} to the owner; waiting on his answer");
    }

    // The supervisor took it back from the owner: it holds it itself, so there is no notice to watch.
    if question.addressee == QuestionAddressee::Person && current.addressee == QuestionAddressee::Supervisor
    {
      eprintln!("ask: the supervisor took question #{id} back from the owner; waiting on it");
    }

    let asker = question.asker_name.clone();
    let told = watch.map(|watched| watched.notice);
    question = current;

    let mut heard = vec![];

    for reply in page.items {
      after = Some(reply.id);

      if reply.settles {
        if let Some(told) = told {
          receiver.withdraw(told).await;
        }

        return Ok(settled_ending(&reply));
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

// One look at the question: none where it was closed unanswered, else the question as it stands and the replies
// past the cursor, the read held open for a while where none stands yet and the question is still put to whom the
// wait last saw it put to, so a move to the owner is told at once.
async fn look(
  client: &JobClient,
  id: i64,
  after: Option<i64>,
  holding: QuestionAddressee,
  hold_ms: i64,
) -> Result<Option<(Question, Page<Reply>)>, ClientFailure> {
  let current = client.question(id).await?;

  if matches!(current.state, QuestionState::Withdrawn | QuestionState::Unsent) {
    return Ok(None);
  }

  let query = WaitQuery {
    wait_ms: (current.addressee == holding).then_some(hold_ms),
    after,
    limit: None,
  };

  client.replies(id, &query).await.map(|page| Some((current, page)))
}

/// What `--show` reads: one question by its number, or every question asked about an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
  Question(i64),
  Item(i64),
}

/// A question as a caller reads it: the question as the service keeps it, and the line a person settled it with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reading {
  #[serde(flatten)]
  pub question: Question,
  pub answer: Option<String>,
}

/// The questions `--show` names, read once: nothing is opened, waited on or told.
pub async fn show(client: &JobClient, shown: Shown) -> Result<Vec<Reading>, String> {
  let questions = match shown {
    Shown::Question(id) => {
      vec![client.question(id).await.map_err(|why| format!("ask: question #{id}: {why}"))?]
    }
    Shown::Item(item) => {
      client.item(item).await.map_err(|why| format!("ask: item #{item}: {why}"))?;
      item_questions(client, item, None)
        .await
        .map_err(|why| format!("ask: item #{item}: {why}"))?
    }
  };
  let mut readings = Vec::with_capacity(questions.len());

  for question in questions {
    let answer = match question.state {
      QuestionState::Answered => settling_reply(client, question.id)
        .await
        .map_err(|why| format!("ask: question #{}: {why}", question.id))?
        .map(|reply| settled_answer(&reply)),
      _ => None,
    };

    readings.push(Reading { question, answer });
  }

  Ok(readings)
}

// The questions asked about an item, in key order: those in `states` where it names some, else every one.
async fn item_questions(
  client: &JobClient,
  item: i64,
  states: Option<&str>,
) -> Result<Vec<Question>, ClientFailure> {
  let mut query = QuestionQuery {
    item_id: Some(item),
    state: states.map(str::to_owned),
    limit: Some(MOST),
    ..QuestionQuery::default()
  };
  let mut questions = Vec::new();

  loop {
    let page = client.questions(&query).await?;

    query.after = page.items.last().map(|question| question.id);
    questions.extend(page.items);

    if !page.more {
      return Ok(questions);
    }
  }
}
