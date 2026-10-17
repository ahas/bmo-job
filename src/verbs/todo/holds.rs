//! An item set aside and taken back (docs/verbs.md, "The TODO's edits and holds"): a plain hold, the owner's
//! stop, a wind-down that waits for an item to finish or commits to go live, the account a run leaves where it
//! stopped, a hold taken off; and the whole queue's own pause and a deployment's hold let go by hand.

use std::path::Path;

use job_base::Refusal;
use job_codes::TodoRefusal;
use job_codes::todo_reading::HeldWord;
use job_codes::todo_verbs::{AfterWrong, OrderHold, ResumeCondition, TodoAnswer};
use job_testbed::{PendingOf, Testbed};
use queue_read::worker_lines::job_author;
use queue_rows::closing::outside_hosting_refusal;
use queue_rows::order_lock::with_order_lock;
use queue_rows::store::{self, Row};
use queue_rows::todos::{WIND_DOWN_NOTE_MARK, hold_as, release, running_job_for};
use queue_rows::{Answered, Ground, find, js, refused, work_orders};
use serde_json::{Value, json};

use super::Beside;
use crate::one;
use crate::queue::holds::{
  deploy_held, deploy_hold, deploy_waits_on, release_deploy_hold as let_go, set_queue_paused,
};
use crate::queue::launches::WORK_ORDER_STARTED_BY;
use crate::queue::units::testbed_failed;
use crate::queue::work_orders::steps_ahead;
use crate::verbs::stand::span;

/// The mark the words a stop-here leaves on an item open with.
pub const STOPPED_HERE_MARK: &str = "[Stopped here]";

/// What a hold waits on before the agent is told it may go on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Condition {
  Finished {
    id: i64,
  },
  Deployed {
    commits: Vec<String>,
    item: Option<i64>,
  },
}

impl Condition {
  /// The condition as an answer names it.
  pub fn coded(&self) -> ResumeCondition {
    match self {
      Self::Finished { id } => ResumeCondition::ItemFinished { id: *id },
      Self::Deployed { commits, item } => ResumeCondition::Deployed {
        what: match item {
          Some(item) => format!("#{item}"),
          None => commits
            .iter()
            .map(|commit| commit.chars().take(7).collect::<String>())
            .collect::<Vec<_>>()
            .join(", "),
        },
      },
    }
  }

  fn words(&self) -> String {
    match self.coded() {
      ResumeCondition::ItemFinished { id } => format!("resumes once #{id} is finished"),
      ResumeCondition::Deployed { what } => format!("resumes once {what} goes live"),
    }
  }
}

/// What the words a hold asks for carry: its reason, and the resume condition it names, by an item that
/// finishes or by what goes live.
#[derive(Clone, Debug, Default)]
pub struct Holding {
  pub why: Option<String>,
  pub after: Option<String>,
  pub after_deploy: Option<String>,
  pub report: Option<String>,
  pub by: Option<String>,
}

/// The reason a held item carries: the reason given and, where one was named, its resume condition.
pub fn wind_down_reason(why: &str, condition: Option<&Condition>) -> String {
  match condition {
    None => why.to_owned(),
    Some(condition) if why.is_empty() => condition.words(),
    Some(condition) => format!("{why} ({})", condition.words()),
  }
}

/// The words a running job is handed when its item is wound down: why, and what it does now.
pub fn wind_down_note(why: &str, condition: Option<&Condition>) -> String {
  let resume = condition
    .map_or_else(|| "it resumes later".to_owned(), |condition| format!("it {}", condition.words()));

  format!(
    "{WIND_DOWN_NOTE_MARK} The owner asked this item to stop: {why}. Finish the step at hand, leave where the run stopped on the item with a stop-here and its report, and end the turn; {resume}."
  )
}

fn git(source: &Path, args: &[&str]) -> Option<String> {
  let done = std::process::Command::new("git")
    .args(args)
    .current_dir(source)
    .stdin(std::process::Stdio::null())
    .stderr(std::process::Stdio::null())
    .output()
    .ok()?;

  done.status.success().then(|| String::from_utf8_lossy(&done.stdout).trim().to_owned())
}

// The main a commit must stand in to go live: the branch the source checkout stands on, unless it is a job's.
fn main_head(source: &Path) -> Option<String> {
  let standing = git(source, &["rev-parse", "--abbrev-ref", "HEAD"]).filter(|branch| branch != "HEAD");
  let main = standing
    .filter(|branch| !branch.starts_with("work/"))
    .unwrap_or_else(|| "main".to_owned());

  git(source, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{main}^{{commit}}")])
    .filter(|head| head.len() == 40)
}

// A resume after a deployment settled to commits: an item's number is that item's commits waiting to go live,
// anything else one commit of the source checkout's repository on main and not live yet.
async fn deploy_condition<T: Testbed>(
  ground: &Ground<'_>,
  beside: &Beside<'_, T>,
  selector: &str,
) -> Result<Condition, Refusal> {
  let as_item = js::trim(selector).trim_start_matches('#');

  if !as_item.is_empty() && as_item.len() <= 6 && as_item.bytes().all(|byte| byte.is_ascii_digit()) {
    let named = find(ground, as_item).await?;
    let mut commits: Vec<String> = vec![];

    for pending in beside.testbed.pending(PendingOf::Item(named.id)).await.map_err(testbed_failed)? {
      if !commits.contains(&pending.commit) {
        commits.push(pending.commit);
      }
    }

    if commits.is_empty() {
      return Err(refused(TodoRefusal::NoWaitingCommits { id: named.id }));
    }

    return Ok(Condition::Deployed {
      commits,
      item: Some(named.id),
    });
  }

  let selector = js::trim(selector).to_owned();
  let source = beside.source;
  let Some(commit) = git(source, &["rev-parse", "--verify", "--quiet", &format!("{selector}^{{commit}}")])
    .filter(|commit| !commit.is_empty())
  else {
    return Err(refused(TodoRefusal::NotACommit { selector }));
  };
  let on_main = main_head(source)
    .is_some_and(|main| git(source, &["merge-base", "--is-ancestor", &commit, &main]).is_some());

  if !on_main {
    return Err(refused(TodoRefusal::CommitNotOnMain { selector }));
  }

  let live = beside.testbed.deployed(source).await.map_err(testbed_failed)?;

  if live.is_some_and(|live| git(source, &["merge-base", "--is-ancestor", &commit, &live.commit]).is_some())
  {
    return Err(refused(TodoRefusal::CommitAlreadyLive { selector }));
  }

  Ok(Condition::Deployed {
    commits: vec![commit],
    item: None,
  })
}

// The resume condition a hold is put with, where one is named.
async fn resume_condition<T: Testbed>(
  ground: &Ground<'_>,
  beside: &Beside<'_, T>,
  item: &Row,
  asked: &Holding,
) -> Result<Option<Condition>, Refusal> {
  if asked.after.is_some() && asked.after_deploy.is_some() {
    return Err(refused(TodoRefusal::ConditionBoth));
  }

  if let Some(selector) = &asked.after_deploy {
    return deploy_condition(ground, beside, selector).await.map(Some);
  }

  let Some(selector) = &asked.after else { return Ok(None) };
  let after = find(ground, selector).await?;

  if after.id == item.id || after.done {
    return Err(refused(TodoRefusal::AfterNotOpen {
      id: after.id,
      why: if after.id == item.id { AfterWrong::Itself } else { AfterWrong::Finished },
    }));
  }

  Ok(Some(Condition::Finished { id: after.id }))
}

async fn put_deploy_condition(
  ground: &Ground<'_>,
  id: i64,
  condition: Option<&Condition>,
) -> Result<(), Refusal> {
  if let Some(Condition::Deployed { commits, .. }) = condition {
    store::hold_until_deployed(ground.db, id, commits).await?;
  }

  Ok(())
}

fn after_item(condition: Option<&Condition>) -> Option<i64> {
  match condition {
    Some(Condition::Finished { id }) => Some(*id),
    _ => None,
  }
}

fn reason_of(why: Option<&str>) -> Option<String> {
  why.map(js::trim).filter(|why| !why.is_empty()).map(str::to_owned)
}

/// The item set aside: the queue no longer picks it up, a run going now finishing its turn.
pub async fn hold<T: Testbed>(
  ground: &Ground<'_>,
  beside: &Beside<'_, T>,
  selector: &str,
  asked: &Holding,
) -> Answered {
  let item = find(ground, selector).await?;

  if asked.after.is_some() {
    return Err(refused(TodoRefusal::HoldTakesNoAfter));
  }

  let condition = resume_condition(ground, beside, &item, asked).await?;
  let given = reason_of(asked.why.as_deref());
  let reason = match &condition {
    None => given,
    Some(condition) => Some(wind_down_reason(given.as_deref().unwrap_or_default(), Some(condition))),
  };

  if !hold_as(ground.db, item.id, reason.as_deref(), None, ground.now).await? {
    return Err(refused(TodoRefusal::HoldNotTaken { id: item.id }));
  }

  put_deploy_condition(ground, item.id, condition.as_ref()).await?;
  ground
    .hands
    .moved(
      None,
      ground.wrote(item.id, if reason.is_some() { vec!["held_reason"] } else { vec![] }, vec![]),
    )
    .await;

  Ok(TodoAnswer::ItemHeld {
    id: item.id,
    title: item.title,
    reason,
  })
}

/// The item's runs stopped now, and the item held as stopped.
pub async fn stop(ground: &Ground<'_>, selector: &str, why: Option<&str>) -> Answered {
  let item = find(ground, selector).await?;
  let Some(reason) = reason_of(why) else {
    return Err(refused(TodoRefusal::StopNeedsWhy));
  };
  let runs = ground.hands.stop(item.id, &reason).await?;

  Ok(TodoAnswer::ItemStopped {
    id: item.id,
    title: item.title,
    runs,
    reason,
  })
}

/// The owner's "finish this much and stop": held to resume later, a run going now told to stop at the end of
/// its step.
pub async fn wind_down<T: Testbed>(
  ground: &Ground<'_>,
  beside: &Beside<'_, T>,
  selector: &str,
  asked: &Holding,
) -> Answered {
  let item = find(ground, selector).await?;
  let Some(why) = reason_of(asked.why.as_deref()) else {
    return Err(refused(TodoRefusal::WindDownNeedsWhy));
  };
  let condition = resume_condition(ground, beside, &item, asked).await?;
  let reason = wind_down_reason(&why, condition.as_ref());

  if !store::wind_down(ground.db, item.id, &reason, after_item(condition.as_ref()), ground.now).await? {
    return Err(refused(TodoRefusal::WindDownNotTaken { id: item.id }));
  }

  put_deploy_condition(ground, item.id, condition.as_ref()).await?;

  let running = running_job_for(ground.db, item.id).await?;
  let by = asked.by.clone().unwrap_or_else(|| ground.names.owner.clone());

  if let Some(job) = running {
    let author = store::Author {
      name: &by,
      sender: (by == ground.names.owner).then_some(ground.owner_sender),
    };

    store::add_note(
      ground.db,
      item.id,
      author,
      &wind_down_note(&why, condition.as_ref()),
      Some(job),
      ground.now,
    )
    .await?;
  }

  ground.hands.moved(None, ground.wrote(item.id, vec!["held_reason"], vec![])).await;

  Ok(TodoAnswer::ItemWoundDown {
    id: item.id,
    title: item.title,
    reason,
    condition: condition.as_ref().map(Condition::coded),
    job: running,
  })
}

/// Why a stop-here is refused before anything moves, or none.
pub fn stop_here_refusal(item: &Row, running: bool, why: &str, after: bool) -> Option<TodoRefusal> {
  if item.done || item.merged_into.is_some() {
    return Some(TodoRefusal::StopHereClosed {
      id: item.id,
      title: item.title.clone(),
    });
  }

  if matches!(item.held_kind.as_deref(), Some("pause" | "stop")) {
    return (!why.is_empty() || after).then_some(TodoRefusal::StopHereHeldTakesNoReason { id: item.id });
  }

  if !running {
    return Some(TodoRefusal::StopHereNotHeld { id: item.id });
  }

  why.is_empty().then_some(TodoRefusal::StopHereNeedsReason { id: item.id })
}

/// Where a run of the item's work order may not hold its own item: anywhere but its final turn, and there
/// while a step stands ahead of it or once the order has already waited on a final turn's own hold.
pub fn order_hold_refusal(order: &Value, job: i64) -> Option<OrderHold> {
  let control = order.get("control").unwrap_or(&Value::Null);
  let final_turn = order.get("phase").and_then(Value::as_str) == Some("finalization")
    && control.get("jobId").and_then(Value::as_i64) == Some(job);

  if !final_turn {
    Some(OrderHold::Step)
  } else if steps_ahead(order) {
    Some(OrderHold::StepsAhead)
  } else if order.get("finalHolds").and_then(Value::as_i64).unwrap_or_default() > 0 {
    Some(OrderHold::HeldOnce)
  } else {
    None
  }
}

// The work order the running job is a run of, where it is one.
async fn running_order(ground: &Ground<'_>, item: i64, job: i64) -> Result<Option<Value>, Refusal> {
  let row = one(ground.db, r#"SELECT "started_by" FROM "jobs" WHERE "id" = $1"#, vec![job.into()]).await?;
  let started_by = row.map(|row| row.try_get::<Option<String>>("", "started_by")).transpose()?.flatten();

  if started_by.as_deref() != Some(WORK_ORDER_STARTED_BY) {
    return Ok(None);
  }

  work_orders::read(ground.db, item).await
}

// A final turn's own hold counted on its order, which waits on one only once.
async fn count_final_hold(ground: &Ground<'_>, item: i64) -> Result<(), Refusal> {
  with_order_lock(ground.db, item, async {
    let Some(mut order) = work_orders::read(ground.db, item).await? else { return Ok(()) };

    order["finalHolds"] = json!(order.get("finalHolds").and_then(Value::as_i64).unwrap_or_default() + 1);
    work_orders::save(ground.db, &mut order).await?;

    Ok(())
  })
  .await
}

fn held_word(kind: Option<&str>) -> HeldWord {
  match kind {
    Some("stop") => HeldWord::Stop,
    Some("pause") => HeldWord::Pause,
    Some("failure") => HeldWord::Failure,
    _ => HeldWord::Plain,
  }
}

/// Where a wound-down or stopped run stopped, left on the item as added words; the item stays open and held.
/// A run told in words to stop holds its own item first.
pub async fn stop_here<T: Testbed>(
  ground: &Ground<'_>,
  beside: &Beside<'_, T>,
  selector: &str,
  asked: &Holding,
) -> Answered {
  let item = find(ground, selector).await?;
  let why = reason_of(asked.why.as_deref()).unwrap_or_default();
  let running = running_job_for(ground.db, item.id).await?;

  if let Some(refusal) =
    stop_here_refusal(&item, running.is_some(), &why, asked.after.is_some() || asked.after_deploy.is_some())
  {
    return Err(refused(refusal));
  }

  let order = match running {
    Some(job) if !why.is_empty() => running_order(ground, item.id, job).await?,
    _ => None,
  };

  if let Some(why) = order.as_ref().zip(running).and_then(|(order, job)| order_hold_refusal(order, job)) {
    return Err(refused(TodoRefusal::OrderStepHolds { id: item.id, why }));
  }

  let report = reason_of(asked.report.as_deref()).unwrap_or_default();

  if report.is_empty() {
    return Err(refused(TodoRefusal::StopHereNeedsReport));
  }

  if let Some(why) = outside_hosting_refusal(&[Some(&report)], &ground.names.owner) {
    return Err(refused(TodoRefusal::ClosingRefused { why }));
  }

  if !why.is_empty() {
    let condition = resume_condition(ground, beside, &item, asked).await?;
    let reason = wind_down_reason(&why, condition.as_ref());

    if !store::wind_down(ground.db, item.id, &reason, after_item(condition.as_ref()), ground.now).await? {
      return Err(refused(TodoRefusal::StopHereClosed {
        id: item.id,
        title: item.title.clone(),
      }));
    }

    put_deploy_condition(ground, item.id, condition.as_ref()).await?;

    if order.is_some() {
      count_final_hold(ground, item.id).await?;
    }
  }

  let standing = store::row(ground.db, item.id).await?.unwrap_or(item);
  let author_name = job_author(running);
  let author = store::Author {
    name: &author_name,
    sender: None,
  };
  let note = store::add_note(
    ground.db,
    standing.id,
    author,
    &format!("{STOPPED_HERE_MARK} {report}"),
    running,
    ground.now,
  )
  .await?;

  store::mark_delivered(ground.db, &[note], ground.now).await?;
  ground
    .hands
    .moved(
      None,
      ground.wrote(standing.id, if why.is_empty() { vec![] } else { vec!["held_reason"] }, vec![note]),
    )
    .await;

  Ok(TodoAnswer::StoppedHere {
    id: standing.id,
    title: standing.title,
    note,
    held: held_word(standing.held_kind.as_deref()),
    reason: standing.held_reason,
  })
}

/// A hold taken off, which is the agent's and never a job's.
pub async fn unhold(ground: &Ground<'_>, selector: &str) -> Answered {
  let item = find(ground, selector).await?;

  if ground.in_job {
    return Err(refused(TodoRefusal::JobCannotUnhold));
  }

  if !release(ground.db, item.id, ground.now).await? {
    return Err(refused(TodoRefusal::NotHeld { id: item.id }));
  }

  ground.hands.moved(None, None).await;

  Ok(TodoAnswer::ItemUnheld {
    id: item.id,
    title: item.title,
    priority: item.priority,
  })
}

// The deployment's hold in one line: what it holds for and how long it has stood, read at `now`.
async fn deploy_hold_line(ground: &Ground<'_>) -> Result<Option<String>, Refusal> {
  let Some(hold) = deploy_hold(ground.db).await? else { return Ok(None) };
  let waits_on = deploy_waits_on(ground.db).await?;
  let number = |key: &str| hold.get(key).and_then(Value::as_i64);
  let commits: Vec<String> = hold
    .get("commits")
    .and_then(Value::as_array)
    .map(|commits| {
      commits
        .iter()
        .map(|one| {
          let commit: String =
            one.get("commit").and_then(Value::as_str).unwrap_or_default().chars().take(12).collect();

          match one.get("todoId").and_then(Value::as_i64) {
            Some(todo) => format!("{commit} (#{todo})"),
            None => commit,
          }
        })
        .collect()
    })
    .unwrap_or_default();
  let mut parts = vec![if commits.is_empty() {
    "commits unread".to_owned()
  } else {
    format!("commits {}", commits.join(", "))
  }];

  parts.push(if waits_on.is_empty() {
    "waiting on no job".to_owned()
  } else {
    format!("waiting on jobs {}", waits_on.iter().map(i64::to_string).collect::<Vec<_>>().join(", "))
  });

  if let (Some(since), Some(until)) = (number("since"), number("until")) {
    parts.push(format!(
      "held for {}, {} left",
      span(((ground.now - since).max(0) / 1000) as f64),
      span(((until - ground.now).max(0) / 1000) as f64)
    ));
  }

  Ok(Some(parts.join(" · ")))
}

/// The whole queue's automatic progress stopped or started again; a resume tells the agent how the queue
/// stands, and says a deployment's hold where one still holds it.
pub async fn pause(ground: &Ground<'_>, paused: bool) -> Result<Vec<TodoAnswer>, Refusal> {
  set_queue_paused(ground.db, paused).await?;

  let mut answers = vec![if paused { TodoAnswer::QueuePaused } else { TodoAnswer::QueueResumed }];

  if !paused {
    ground.hands.queue_standing("resumed", true).await?;

    if let Some(hold) = deploy_hold_line(ground).await? {
      answers.push(TodoAnswer::DeployHoldStands { hold });
    }
  }

  ground.hands.moved(None, None).await;

  Ok(answers)
}

/// The deployment's hold let go by hand; the deployment waiting behind it puts nothing live.
pub async fn release_deploy_hold(ground: &Ground<'_>) -> Answered {
  if !deploy_held(ground.db).await? {
    return Err(refused(TodoRefusal::NoDeployHold));
  }

  let hold = deploy_hold_line(ground).await?.unwrap_or_default();

  let_go(ground.db).await?;
  ground.hands.queue_standing("resumed", true).await?;
  ground.hands.moved(None, None).await;

  Ok(TodoAnswer::DeployHoldReleased { hold })
}
