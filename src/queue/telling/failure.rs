//! A broken run handed to the agent as work (docs/queue.md, "A broken run"): a job whose runner died is
//! handed to the agent's session in the owner's chat as an order carrying what was read where it broke, and
//! the agent finds the cause and fixes it at the root. One cause is one order: the same cause again while
//! its order stands is counted into it. Where the agent cannot answer, the owner hears the fact the order
//! keeps for that.

use std::sync::LazyLock;

use job_codes::{Fact, FailureFact};
use job_receiver::{Instruction, InstructionState, Said};
use queue_read::dates::iso;
use queue_read::runs::work_label;
use regex::Regex;
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};

use super::{Teller, utf16_cut};
use crate::queue::launches;
use crate::queue::outcomes::{JOB_DIED, JOB_ERROR, JOB_FAILED};
use crate::queue::settings;
use crate::queue::units::Units;
use crate::{Refusal, one};

/// How long an order stands: a cause that comes back inside it is the same investigation.
pub const ORDER_WINDOW_MS: i64 = 6 * 60 * 60_000;

/// How long an order may sit untaken before the agent is judged unable to answer it.
pub const ORDER_GRACE_MS: i64 = 5 * 60_000;

/// The family a failure's order is listed under.
pub const FAILURE_SORT: &str = "run_failure";

// How far either side of the failure the journal window reaches.
const JOURNAL_MARGIN_MS: i64 = 2 * 60_000;

const ORDERS_KEY: &str = "failure_orders";

// How much of a job's own failure text the order carries.
const JOB_SAID_LIMIT: usize = 1_500;

// How much of a tool call's arguments the owner's fact carries.
const TOOL_DETAIL_LIMIT: usize = 120;

static HEX: LazyLock<Regex> =
  LazyLock::new(|| Regex::new("(?i)[0-9a-f]{8,}").expect("the hex pattern builds"));
static DIGITS: LazyLock<Regex> = LazyLock::new(|| Regex::new("[0-9]+").expect("the digits pattern builds"));
static SPACE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\s+").expect("the space pattern builds"));

/// A moment as the journal takes it: UTC to the second, named as UTC.
pub fn journal_time(at_ms: i64) -> String {
  chrono::DateTime::from_timestamp_millis(at_ms)
    .map(|at| at.format("%Y-%m-%d %H:%M:%S UTC").to_string())
    .unwrap_or_default()
}

/// The command that reads one unit's journal over the window around a failure.
pub fn journal_window(unit: &str, at_ms: i64) -> String {
  format!(
    "journalctl --user -u {unit} --since \"{}\" --until \"{}\" --no-pager",
    journal_time(at_ms - JOURNAL_MARGIN_MS),
    journal_time(at_ms + JOURNAL_MARGIN_MS)
  )
}

/// What tells one job failure from another when a repetition is counted: its ending and the first line of
/// what came back, with the numbers and ids that differ between two runs of the same fault taken out.
pub fn job_signature(outcome: &str, said: Option<&str>) -> String {
  let first = said
    .unwrap_or_default()
    .split('\n')
    .find(|line| !line.trim().is_empty())
    .unwrap_or_default();
  let plain = HEX.replace_all(first, "#");
  let plain = DIGITS.replace_all(&plain, "#");
  let plain = SPACE.replace_all(&plain, " ");

  format!("job:{outcome}:{}", utf16_cut(plain.trim(), 80))
}

/// The last tool call a run made, and what it was given.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastAction {
  pub tool: String,
  pub detail: Option<String>,
}

/// What was read where a run broke: what tells its cause from another, what broke, when, how it ended,
/// what came back, what it was last doing, where its record is, the units whose journal holds it, and the
/// fact the owner hears where the agent cannot answer.
pub struct BrokenRun {
  pub signature: String,
  pub what: String,
  pub at: i64,
  pub ending: String,
  pub said: Option<String>,
  pub last_action: Option<LastAction>,
  pub record: String,
  pub units: Vec<String>,
  pub last_resort: FailureFact,
}

// The order as the agent reads it: English, the facts whole.
fn order_text(broken: &BrokenRun, count: u32, before: u32) -> String {
  let action = broken.last_action.as_ref().map_or_else(
    || "none recorded".to_owned(),
    |action| {
      format!(
        "{}{}",
        action.tool,
        action.detail.as_ref().map_or_else(String::new, |detail| format!(" ({detail})"))
      )
    },
  );
  let mut lines = vec![
    "[A run broke: find the cause and fix it at the root]".to_owned(),
    format!("What broke: {}", broken.what),
    format!("When: {}", iso(broken.at)),
    format!("How it ended: {}", broken.ending),
    format!("What came back: {}", broken.said.as_deref().unwrap_or("nothing readable")),
    format!("Last tool call: {action}"),
    format!("Record: {}", broken.record),
    "Journal window, in UTC as written:".to_owned(),
  ];

  lines.extend(broken.units.iter().map(|unit| format!("  {}", journal_window(unit, broken.at))));

  if count > 1 {
    lines.push(format!(
      "This same cause has now broken a run {count} times since this order was first raised; it is one order, not {count}."
    ));
  }

  if before > 0 {
    lines.push(format!(
      "It had already come {before} time(s) before, under an earlier order that did not end it."
    ));
  }

  lines.extend(
    [
      "",
      "What to do:",
      "1. Tell the owner, in your own words and briefly, what happened and that you have started looking into it. Do not give a cause you have not read yet; if you do not know it, say so.",
      "2. Read the record and the journal window above, and find why it broke.",
      "3. Fix it at the root. A change to the system is filed as a TODO item, as every such change is; host work no repository's service owns you do yourself.",
      "4. When you know the cause, tell the owner what it was and what you did about it.",
      "Do not rerun the job, and do not mark any item done, on the strength of this order alone.",
    ]
    .map(str::to_owned),
  );

  lines.join("\n")
}

/// An order raised and not yet outlived its window: its cause, how many times it broke a run since it was
/// raised and before under lapsed orders, when it was raised and last came, the instruction carrying it,
/// when a turn of the agent's answered after it and when the owner was told it, and the fact the owner is
/// told.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FailureOrder {
  pub signature: String,
  pub count: u32,
  pub before: u32,
  pub raised_at: i64,
  pub last_at: i64,
  pub instruction_id: i64,
  pub answered_at: Option<i64>,
  pub told_at: Option<i64>,
  pub last_resort: Fact,
}

async fn read_orders(db: &DatabaseConnection) -> Result<Vec<FailureOrder>, Refusal> {
  let stored = settings::get(db, ORDERS_KEY).await?;

  Ok(stored.and_then(|stored| serde_json::from_str(&stored).ok()).unwrap_or_default())
}

async fn write_orders(db: &DatabaseConnection, orders: &[FailureOrder]) -> Result<(), Refusal> {
  if orders.is_empty() {
    settings::forget(db, ORDERS_KEY).await
  } else {
    settings::set(db, ORDERS_KEY, &serde_json::to_string(orders).unwrap_or_default()).await
  }
}

/// The orders standing at `now`: raised inside their window.
pub async fn standing_orders(db: &DatabaseConnection, now: i64) -> Result<Vec<FailureOrder>, Refusal> {
  Ok(
    read_orders(db)
      .await?
      .into_iter()
      .filter(|order| now - order.raised_at < ORDER_WINDOW_MS)
      .collect(),
  )
}

fn order_instruction(owner_chat: i64, broken: &BrokenRun, count: u32, before: u32) -> Instruction {
  Instruction {
    agent: None,
    chat: Some(owner_chat),
    said: Said::Order(order_text(broken, count, before)),
    kind: Some(format!("failure-order:{}", broken.signature)),
    sort: FAILURE_SORT.to_owned(),
    summary: None,
    expires_at: None,
    events: vec![],
    rides_until: None,
    via: None,
    run: None,
  }
}

fn with_count(fact: &FailureFact, count: u32) -> FailureFact {
  match fact {
    FailureFact::JobBroke {
      job_id,
      label,
      outcome,
      said,
      last_tool,
      tool_detail,
      unit,
      ..
    } => FailureFact::JobBroke {
      job_id: *job_id,
      label: label.clone(),
      outcome: outcome.clone(),
      said: said.clone(),
      last_tool: last_tool.clone(),
      tool_detail: tool_detail.clone(),
      unit: unit.clone(),
      count,
    },
    other => other.clone(),
  }
}

/// Hands a broken run to the agent in the owner's chat at `now`. A cause with an order already standing is
/// counted into it: an order not yet taken is replaced by one carrying the count, one already taken is left
/// alone.
pub async fn raise_failure_order(
  db: &DatabaseConnection,
  units: &dyn Units,
  owner_chat: i64,
  broken: &BrokenRun,
  now: i64,
) -> Result<(), Refusal> {
  let all = read_orders(db).await?;
  let raised = |order: &FailureOrder| now - order.raised_at < ORDER_WINDOW_MS;
  let mut standing: Vec<FailureOrder> = all.iter().filter(|order| raised(order)).cloned().collect();
  let before: u32 = all
    .iter()
    .filter(|order| !raised(order) && order.signature == broken.signature)
    .map(|order| order.before + order.count)
    .sum();

  if let Some(current) = standing.iter_mut().find(|order| order.signature == broken.signature) {
    current.count += 1;
    current.last_at = now;
    current.last_resort = Fact::Failure(with_count(&broken.last_resort, current.count));

    // Not yet taken: the instruction waiting is replaced, so what the session takes up carries the count.
    if units.instruction_state(current.instruction_id).await? == Some(InstructionState::Pending) {
      current.instruction_id = units
        .queue_instruction(&order_instruction(owner_chat, broken, current.count, current.before))
        .await?;
    }

    return write_orders(db, &standing).await;
  }

  let instruction_id = units.queue_instruction(&order_instruction(owner_chat, broken, 1, before)).await?;

  standing.push(FailureOrder {
    signature: broken.signature.clone(),
    count: 1,
    before,
    raised_at: now,
    last_at: now,
    instruction_id,
    answered_at: None,
    told_at: None,
    last_resort: Fact::Failure(with_count(&broken.last_resort, 1)),
  });
  write_orders(db, &standing).await
}

/// A turn of the agent's has answered at `now`: every order raised before it is being answered by a
/// session that can speak.
pub async fn note_agent_answered(db: &DatabaseConnection, now: i64) -> Result<(), Refusal> {
  let mut orders = standing_orders(db, now).await?;
  let open = |order: &FailureOrder| order.answered_at.is_none() && order.raised_at <= now;

  if !orders.iter().any(open) {
    return Ok(());
  }

  for order in orders.iter_mut().filter(|order| open(order)) {
    order.answered_at = Some(now);
  }

  write_orders(db, &orders).await
}

/// The orders the agent cannot answer, each marked told as it is handed back so its fact goes out once:
/// nothing of the agent's answered since it was raised, and either the owner's own chat had a run die after
/// it or its grace passed with it not in the middle of being answered.
pub async fn due_last_resorts(
  db: &DatabaseConnection,
  units: &dyn Units,
  now: i64,
  owner_died_at: Option<i64>,
) -> Result<Vec<FailureOrder>, Refusal> {
  let mut orders = standing_orders(db, now).await?;
  let mut due = vec![];

  for (at, order) in orders.iter().enumerate() {
    if order.answered_at.is_some() || order.told_at.is_some() {
      continue;
    }

    // Strictly after: the death that raised an order in the owner's own chat is not that session failing
    // to answer it.
    let owner_died = owner_died_at.is_some_and(|died| died > order.raised_at);
    let overdue = now - order.raised_at >= ORDER_GRACE_MS
      && units.instruction_state(order.instruction_id).await? != Some(InstructionState::Handed);

    if owner_died || overdue {
      due.push(at);
    }
  }

  if due.is_empty() {
    return Ok(vec![]);
  }

  for at in &due {
    orders[*at].told_at = Some(now);
  }

  write_orders(db, &orders).await?;

  Ok(due.into_iter().map(|at| orders[at].clone()).collect())
}

impl Teller<'_> {
  // The last tool call a job's run made, as bmo-agency's log of it reads.
  async fn last_action(&self, job_id: i64) -> Result<Option<LastAction>, Refusal> {
    let lines = self.units.job_lines(job_id).await?;

    Ok(lines.calls.into_iter().max_by_key(|call| call.id).map(|call| {
      LastAction {
        tool: call.name,
        detail: call
          .command
          .or(call.args)
          .map(|detail| utf16_cut(detail.trim(), TOOL_DETAIL_LIMIT).to_owned())
          .filter(|detail| !detail.is_empty()),
      }
    }))
  }

  /// Hands a job that broke while it worked to the agent at `now`, out of what its row and its run log
  /// kept. A job whose row is gone, or whose ending is not a breakage, raises nothing.
  pub(super) async fn order_job_failure(&self, job_id: i64, now: i64) -> Result<(), Refusal> {
    let Some(job) = one(
      self.db,
      r#"SELECT "unit", "worker", "todo_id", "finished_at", "outcome", "report" FROM "jobs" WHERE "id" = $1"#,
      vec![job_id.into()],
    )
    .await?
    else {
      return Ok(());
    };
    let outcome: Option<String> = job.try_get("", "outcome")?;
    let Some(outcome) =
      outcome.filter(|outcome| [JOB_ERROR, JOB_FAILED, JOB_DIED].contains(&outcome.as_str()))
    else {
      return Ok(());
    };
    let unit: String = job.try_get("", "unit")?;
    let worker = job
      .try_get::<Option<String>>("", "worker")?
      .unwrap_or_else(|| queue_read::lines::BUILTIN_WORKER.into());
    let todo_id: Option<i64> = job.try_get("", "todo_id")?;
    let at = job.try_get::<Option<i64>>("", "finished_at")?.unwrap_or(now);
    let attempt = match todo_id {
      Some(_) => launches::attempt(self.db, job_id).await?,
      None => 1,
    };
    let label = work_label(todo_id, attempt);
    let title = match todo_id {
      Some(todo) => queue_rows::todos::item(self.db, todo).await?.map(|item| item.title),
      None => None,
    };
    let said = job
      .try_get::<Option<String>>("", "report")?
      .map(|report| report.trim().to_owned())
      .filter(|report| !report.is_empty())
      .map(|report| utf16_cut(&report, JOB_SAID_LIMIT).to_owned());
    let last_action = self.last_action(job_id).await?;
    let named = match todo_id {
      Some(todo) => format!("item #{todo} attempt {attempt}"),
      None => "no item".to_owned(),
    };
    let broken = BrokenRun {
      signature: job_signature(&outcome, said.as_deref()),
      what: format!(
        "job {job_id} ({named}{}), worker {worker}, unit {unit}",
        title.map_or_else(String::new, |title| format!(": {title}"))
      ),
      at,
      ending: outcome.clone(),
      said: said.clone(),
      last_action: last_action.clone(),
      record: format!("job {job_id}'s row in bmo-job, and its run log under unit {unit} in bmo-agency"),
      units: vec![self.service_unit.clone(), unit.clone()],
      last_resort: FailureFact::JobBroke {
        job_id,
        label,
        outcome,
        said: said.as_deref().map(|said| utf16_cut(said, TOOL_DETAIL_LIMIT).to_owned()),
        last_tool: last_action.as_ref().map(|action| action.tool.clone()),
        tool_detail: last_action.and_then(|action| action.detail),
        unit,
        count: 1,
      },
    };

    raise_failure_order(self.db, self.units, self.owner_chat, &broken, now).await
  }
}
