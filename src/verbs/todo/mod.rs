//! The TODO verb, `todo` (docs/verbs.md, "The TODO's edits and holds"): one verb of the queue per call —
//! filing a row, adding words to one, rewriting, approving, holding, stopping, winding down, merging, closing
//! and the queue's own pause — each answering its `TodoAnswer` codes. The rows are written through
//! `queue-rows`; the server is told the queue moved, and a stop goes to the server, whose runs are its to end.

pub mod edits;
pub mod holds;

use std::path::Path;
use std::time::Duration;

use clap::Args;
use futures_util::future::BoxFuture;
use job_agency::{Agency, AgencyClient};
use job_base::Refusal;
use job_brain::BrainClient;
use job_codes::todo_verbs::TodoAnswer;
use job_codes::{Fact, JobError, QueueTold, TodoRefusal, UnitRefusal};
use job_receiver::ReceiverClient;
use job_testbed::Testbed;
use queue_rows::filing::{self, Append, Approving, Brief, Filing};
use queue_rows::{Filer, Ground, Hands, Ports, WordsAdded, Written, impact, refused};
use serde_json::{Value, json};

use self::edits::{Closing, Edit};
use self::holds::Holding;
use super::stand::{JobProcess, Stand};
use crate::queue::units::agency_failed;

/// What the verbs of holding stand on beside the rows' ground: the checkout a commit is read in, and
/// bmo-testbed, which says what waits to go live and what is live.
pub struct Beside<'a, T> {
  pub source: &'a Path,
  pub testbed: &'a T,
}

/// The verb's words. Each verb is one flag, the item it acts on its value; the flags beside it are what that
/// verb takes.
#[derive(Args, Clone, Debug, Default)]
pub struct TodoArgs {
  #[arg(long)]
  pub add: bool,
  #[arg(long)]
  pub pause: bool,
  #[arg(long)]
  pub resume: bool,
  #[arg(long)]
  pub release_deploy_hold: bool,
  /// Bare with --add; the item and on or off otherwise.
  #[arg(long, num_args = 0..=2, allow_hyphen_values = true)]
  pub cowork: Option<Vec<String>>,
  /// The grounds as JSON with --add; the item and its grounds otherwise.
  #[arg(long, num_args = 1..=2, allow_hyphen_values = true)]
  pub impact: Option<Vec<String>>,
  /// The item and the grounds its job weighed again.
  #[arg(long, num_args = 2, allow_hyphen_values = true)]
  pub impact_check: Option<Vec<String>>,
  #[arg(long, num_args = 2, allow_hyphen_values = true)]
  pub worker: Option<Vec<String>>,
  #[arg(long, num_args = 2, allow_hyphen_values = true)]
  pub repairs: Option<Vec<String>>,
  #[arg(long, num_args = 2, allow_hyphen_values = true)]
  pub priority: Option<Vec<String>>,
  #[arg(long, allow_hyphen_values = true)]
  pub after: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub after_deploy: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub append: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub approve: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub brief: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub by: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub change: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub chat: Option<i64>,
  #[arg(long, allow_hyphen_values = true)]
  pub conclusion: Option<String>,
  #[arg(long = "do", allow_hyphen_values = true)]
  pub to_do: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub done: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub drop: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub edit: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub from_todo: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub hold: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub instruction: Vec<i64>,
  #[arg(long, allow_hyphen_values = true)]
  pub intent: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub into: Option<i64>,
  #[arg(long, allow_hyphen_values = true)]
  pub judgement: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub mark: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub merge: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub model: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub note: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub note_id: Option<i64>,
  #[arg(long, allow_hyphen_values = true)]
  pub order: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub raised_by: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub reopen: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub reply: Vec<i64>,
  #[arg(long, allow_hyphen_values = true)]
  pub reply_text: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub report: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub requester: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub resolve: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub set_model: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub status: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub stop: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub stop_here: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub text: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub title: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub unappend: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub unapprove: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub unfinished: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub unhold: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub unmerge: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub what: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub why: Option<String>,
  #[arg(long, allow_hyphen_values = true)]
  pub wind_down: Option<String>,
}

impl TodoArgs {
  fn holding(&self) -> Holding {
    Holding {
      why: self.why.clone(),
      after: self.after.clone(),
      after_deploy: self.after_deploy.clone(),
      report: self.report.clone(),
      by: self.by.clone(),
    }
  }

  fn filing(&self) -> Filing {
    Filing {
      title: self.title.clone(),
      chat: self.chat,
      model: self.model.clone(),
      status: self.status.clone(),
      cowork: self.cowork.is_some(),
      impact: self.impact.as_ref().and_then(|given| given.first().cloned()),
      what: self.what.clone(),
      to_do: self.to_do.clone(),
      conclusion: self.conclusion.clone(),
      instructions: self.instruction.clone(),
      replies: self.reply.clone(),
      reply_text: self.reply_text.clone(),
      requester: self.requester.clone(),
      judgement: self.judgement.clone(),
      raised_by: self.raised_by.clone(),
      from_todo: self.from_todo.clone(),
    }
  }
}

// The comma-separated numbers a flag names, smallest first; a part naming no number refuses the merge.
fn id_list(value: &str) -> Option<Vec<i64>> {
  let mut ids: Vec<i64> = value
    .split(',')
    .map(|part| part.trim().parse::<i64>().ok())
    .collect::<Option<Vec<i64>>>()?;

  ids.sort_unstable();
  Some(ids)
}

// The two words a flag of two takes: the item, and what it is set to.
fn pair(given: &[String]) -> (&str, &str) {
  (given.first().map_or("", String::as_str), given.get(1).map_or("", String::as_str))
}

/// One call of `todo`, what it came to in its codes. A middle step of a work order does not close its item.
pub async fn todo_verb<T: Testbed>(
  ground: &Ground<'_>,
  beside: &Beside<'_, T>,
  args: &TodoArgs,
  work_order_step: bool,
) -> Result<Vec<TodoAnswer>, Refusal> {
  let one = |answered: Result<TodoAnswer, Refusal>| answered.map(|answer| vec![answer]);

  if let Some(selector) = &args.append {
    let asked = Append {
      text: args.text.as_deref().unwrap_or_default(),
      by: args.by.as_deref(),
    };

    return one(filing::append(ground, selector, &asked).await);
  }

  if let Some(selector) = &args.unappend {
    return one(filing::unappend(ground, selector, args.note_id, args.by.as_deref()).await);
  }

  if let Some(selector) = &args.brief {
    let asked = Brief {
      brief: args.text.as_deref().unwrap_or_default(),
      change: args.change.as_deref().unwrap_or_default(),
    };

    return one(filing::brief(ground, selector, &asked).await);
  }

  if let Some(selector) = &args.edit {
    let asked = Edit {
      title: args.title.clone(),
      what: args.what.clone(),
      to_do: args.to_do.clone(),
      conclusion: args.conclusion.clone(),
      judgement: args.judgement.clone(),
      intent: args.intent.clone(),
      order: args.order.clone(),
    };

    return one(edits::edit(ground, selector, &asked).await);
  }

  if args.pause || args.resume {
    return holds::pause(ground, args.pause).await;
  }

  if args.release_deploy_hold {
    return one(holds::release_deploy_hold(ground).await);
  }

  if let Some((selector, approve)) = args
    .approve
    .as_ref()
    .map(|one| (one, true))
    .or_else(|| args.unapprove.as_ref().map(|one| (one, false)))
  {
    let asked = Approving {
      approve,
      note: args.note.as_deref(),
    };

    return one(filing::approve(ground, selector, &asked).await);
  }

  if let Some(selector) = &args.hold {
    return one(holds::hold(ground, beside, selector, &args.holding()).await);
  }

  if let Some(selector) = &args.stop {
    return one(holds::stop(ground, selector, args.why.as_deref()).await);
  }

  if let Some(selector) = &args.wind_down {
    return one(holds::wind_down(ground, beside, selector, &args.holding()).await);
  }

  if let Some(selector) = &args.stop_here {
    return one(holds::stop_here(ground, beside, selector, &args.holding()).await);
  }

  if let Some(selector) = &args.unhold {
    return one(holds::unhold(ground, selector).await);
  }

  if let Some(given) = &args.merge {
    let Some(ids) = id_list(given) else {
      return Err(refused(TodoRefusal::MergeNeedsItems));
    };

    return one(edits::merge(ground, &ids, args.into).await);
  }

  if let Some(selector) = &args.unmerge {
    return one(edits::unmerge(ground, selector).await);
  }

  if let Some(selector) = &args.resolve {
    return one(edits::resolve(ground, selector, args.why.as_deref().unwrap_or_default()).await);
  }

  if let Some(selector) = &args.reopen {
    return one(edits::reopen(ground, selector).await);
  }

  if let Some(selector) = &args.done {
    if work_order_step {
      return Err(refused(TodoRefusal::MiddleStepCloses));
    }

    let asked = Closing {
      report: args.report.clone(),
      unfinished: args.unfinished.clone(),
    };

    return edits::done(ground, selector, &asked).await;
  }

  if let Some(selector) = &args.mark {
    return one(edits::mark(ground, selector, args.status.as_deref()).await);
  }

  if let Some(given) = &args.worker {
    let (selector, asked) = pair(given);

    return one(edits::worker(ground, beside.source, selector, asked).await);
  }

  if let Some(given) = &args.repairs {
    if args.add {
      return Err(refused(TodoRefusal::RepairsOnFiling));
    }

    let (selector, asked) = pair(given);

    return one(edits::repairs(ground, selector, asked).await);
  }

  if let Some(given) = args.cowork.as_ref().filter(|_| !args.add) {
    let (selector, asked) = pair(given);

    return one(edits::cowork(ground, selector, asked).await);
  }

  if let Some(given) = args.impact.as_ref().filter(|_| !args.add) {
    let (selector, written) = pair(given);

    return one(impact::impact(ground, selector, written).await);
  }

  if let Some(given) = &args.impact_check {
    let (selector, written) = pair(given);

    return one(impact::impact_check(ground, selector, written).await);
  }

  if let Some(given) = &args.priority {
    let (selector, asked) = pair(given);

    return one(edits::priority(ground, selector, asked).await);
  }

  if let Some(selector) = &args.drop {
    return edits::drop(ground, selector).await;
  }

  if let Some(selector) = &args.set_model {
    return one(edits::set_model(ground, selector, args.model.as_deref()).await);
  }

  filing::add(ground, &args.filing()).await.map(|(_, answer)| vec![answer])
}

// A stop waits for the order's lock, which a launch of one of its steps may be holding.
const STOP_WAIT: Duration = Duration::from_secs(180);

// Telling the queue's standing waits no longer than a short while.
const TELL_WAIT: Duration = Duration::from_secs(30);

/// The hands of the verb run from a shell: the server told the queue moved and asked to tell the queue's
/// standing and to stop an item's runs, and the agent told of words added.
pub struct ShellHands<'a> {
  pub stand: &'a Stand,
}

fn server_refused(why: super::stand::ServerRefused) -> Refusal {
  JobError::from(UnitRefusal::Server { failure: why.error }).into()
}

impl Hands for ShellHands<'_> {
  fn moved(&self, _work_order: Option<i64>, _written: Option<Written>) -> BoxFuture<'_, ()> {
    Box::pin(async move { self.stand.moved("todo").await })
  }

  fn queue_standing<'a>(
    &'a self,
    moment: &'a str,
    repeat: bool,
  ) -> BoxFuture<'a, Result<QueueTold, Refusal>> {
    Box::pin(async move {
      let told = self
        .stand
        .ask_server("queue/tell", &json!({ "moment": moment, "repeat": repeat }), TELL_WAIT)
        .await
        .map_err(server_refused)?;

      Ok(
        serde_json::from_value(told.get("told").cloned().unwrap_or(Value::Null))
          .unwrap_or(QueueTold::Unreadable),
      )
    })
  }

  fn words_added<'a>(&'a self, added: &'a WordsAdded<'a>) -> BoxFuture<'a, Result<(), Refusal>> {
    Box::pin(async move {
      self.stand.tell("todo", self.stand.owner.chat, Fact::Todo(added.fact())).await;
      Ok(())
    })
  }

  fn stop<'a>(&'a self, id: i64, why: &'a str) -> BoxFuture<'a, Result<u32, Refusal>> {
    Box::pin(async move {
      let answer = self
        .stand
        .ask_server("todo", &json!({ "action": "stop", "id": id, "why": why }), STOP_WAIT)
        .await
        .map_err(server_refused)?;

      Ok(
        answer
          .get("stopped")
          .and_then(Value::as_u64)
          .and_then(|count| u32::try_from(count).ok())
          .unwrap_or(0),
      )
    })
  }
}

/// The rows' calls on other units, over the stand's clients.
pub fn rows_units(stand: &Stand) -> Ports<'_, AgencyClient, BrainClient, ReceiverClient> {
  Ports {
    agency: &stand.ground.agency,
    brain: &stand.ground.brain,
    receiver: &stand.ground.receiver,
    agent_key: &stand.ground.agent_key,
  }
}

/// The providers a job's model may name, as bmo-agency lists them.
pub async fn providers(stand: &Stand) -> Result<Vec<String>, Refusal> {
  Ok(
    stand
      .ground
      .agency
      .providers()
      .await
      .map_err(agency_failed)?
      .into_iter()
      .map(|provider| provider.name)
      .collect(),
  )
}

/// The rows' ground of a verb run from a shell: the process's job, the agent whose turn runs it, or the
/// person at it.
pub fn rows_ground<'a>(
  stand: &'a Stand,
  process: &'a JobProcess,
  filer: Filer<'a>,
  person: Option<&'a str>,
  units: &'a dyn queue_rows::Units,
  hands: &'a dyn Hands,
  providers: &'a [String],
) -> Ground<'a> {
  Ground {
    db: &stand.db,
    owner_sender: stand.owner.sender,
    owner_chat: stand.owner.chat,
    names: &stand.names,
    agent_key: &stand.ground.agent_key,
    policy: &stand.policy,
    reader: &stand.reader,
    banned: &stand.ground.banned,
    providers,
    in_job: process.in_job,
    run_name: process.run_name.as_deref(),
    filer,
    person,
    hands,
    units,
    now: crate::now_ms(),
  }
}
