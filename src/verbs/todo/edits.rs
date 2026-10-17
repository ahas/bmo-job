//! An item changed in place (docs/verbs.md, "The TODO's edits and holds"): its parts rewritten while it is a
//! draft, items merged into it and taken out again, its unfinished note resolved, the item closed with its
//! account and opened again, and its settings — the state line, the worker, the item it repairs, co-work,
//! priority and model — and a row thrown out.

use std::path::Path;

use job_base::Refusal;
use job_codes::TodoRefusal;
use job_codes::todo_reading::HeldWord;
use job_codes::todo_verbs::{ClosingPart, EditedPart, TodoAnswer};
use queue_rows::blocks::{OwnBlock, own_block_text, rewrite_own_block};
use queue_rows::closing::{
  hand_over, live_claude_openings, merged_report, outside_hosting_refusal, reopen_settled_sources,
  report_with_live_claude, resolve_settled_sources,
};
use queue_rows::filing::{is_job_model, wording_refusal};
use queue_rows::instructions::{Order, queue_order};
use queue_rows::store::{self, Column, MergeRefusal, Row};
use queue_rows::todos::{
  drop_item, resolve_unfinished, running_job_for, set_cowork, set_priority, set_worker, sources,
};
use queue_rows::{Answered, Ground, find, js, refused, work_orders};
use serde_json::Value;

use crate::queue::workers::workers;

/// The most repairs a chain runs before it is the owner's to look at.
pub const MAX_REPAIR_ROUNDS: usize = 3;

/// The parts an edit writes again, each where it is named.
#[derive(Clone, Debug, Default)]
pub struct Edit {
  pub title: Option<String>,
  pub what: Option<String>,
  pub to_do: Option<String>,
  pub conclusion: Option<String>,
  pub judgement: Option<String>,
  pub intent: Option<String>,
  pub order: Option<String>,
}

fn part_of(block: OwnBlock) -> EditedPart {
  match block {
    OwnBlock::What => EditedPart::What,
    OwnBlock::ToDo => EditedPart::Do,
    OwnBlock::Conclusion => EditedPart::Conclusion,
    OwnBlock::Judgement => EditedPart::Judgement,
  }
}

fn trimmed(value: &Option<String>) -> Option<String> {
  value.as_deref().map(|value| js::trim(value).to_owned())
}

/// The filer's own parts of an item written again while it is a draft, and its work order's draft with them
/// where the draft still says what the part said.
pub async fn edit(ground: &Ground<'_>, selector: &str, asked: &Edit) -> Answered {
  let item = find(ground, selector).await?;
  let given: Vec<(OwnBlock, String)> = [
    (OwnBlock::What, &asked.what),
    (OwnBlock::ToDo, &asked.to_do),
    (OwnBlock::Conclusion, &asked.conclusion),
    (OwnBlock::Judgement, &asked.judgement),
  ]
  .into_iter()
  .filter_map(|(part, value)| trimmed(value).map(|value| (part, value)))
  .collect();
  let new_title = trimmed(&asked.title);
  let intent = trimmed(&asked.intent);
  let order_text = trimmed(&asked.order);

  if given.is_empty() && new_title.is_none() && intent.is_none() && order_text.is_none() {
    return Err(refused(TodoRefusal::EditNothing));
  }

  let named = [&new_title, &intent, &order_text]
    .into_iter()
    .flatten()
    .chain(given.iter().map(|(_, value)| value));

  if named.clone().any(String::is_empty) {
    return Err(refused(TodoRefusal::EditPartEmpty));
  }

  let worded: Vec<Option<&str>> = std::iter::once(new_title.as_deref())
    .chain(given.iter().map(|(_, value)| Some(value.as_str())))
    .collect();

  if let Some(refusal) = wording_refusal(ground, &worded) {
    return Err(refusal);
  }

  if item.done {
    return Err(refused(TodoRefusal::ItemClosedAlready { id: item.id }));
  }

  let words_too = !given.is_empty() || intent.is_some() || order_text.is_some();

  if words_too && let Some(job) = running_job_for(ground.db, item.id).await? {
    return Err(refused(TodoRefusal::EditJobRunning { id: item.id, job }));
  }

  let mut instruction = item.instruction.clone();

  for (part, value) in &given {
    instruction = rewrite_own_block(&instruction, *part, value).ok_or_else(|| {
      refused(TodoRefusal::EditNoPart {
        id: item.id,
        part: part_of(*part),
      })
    })?;
  }

  let mut order = work_orders::read(ground.db, item.id).await?;
  let draft = order
    .as_ref()
    .is_some_and(|order| order.get("status").and_then(Value::as_str) == Some("draft"));

  if (intent.is_some() || order_text.is_some()) && order.is_some() && !draft {
    return Err(refused(TodoRefusal::EditOrderStarted { id: item.id }));
  }

  if let Some(order) = order.as_mut().filter(|_| draft) {
    let before = |part: OwnBlock| own_block_text(&item.instruction, part);
    let after = |part: OwnBlock| own_block_text(&instruction, part).map_or(Value::Null, Value::from);
    let judged = before(OwnBlock::Judgement);
    let field = |order: &Value, key: &str| order.get(key).and_then(Value::as_str).map(str::to_owned);

    for (key, written, part) in
      [("intent", &intent, OwnBlock::Conclusion), ("instruction", &order_text, OwnBlock::ToDo)]
    {
      let standing = field(order, key);
      let next = match written {
        Some(written) => Some(Value::from(written.as_str())),
        None if before(part).is_some() && standing == before(part) => Some(after(part)),
        None if judged.is_some() && standing == judged => Some(after(OwnBlock::Judgement)),
        None => None,
      };

      if let Some(next) = next {
        order[key] = next;
      }
    }

    work_orders::save(ground.db, order).await?;
  }

  let title = new_title.clone().unwrap_or_else(|| item.title.clone());

  store::rewrite(ground.db, item.id, &title, &instruction).await?;

  let changed: Vec<EditedPart> = new_title
    .iter()
    .map(|_| EditedPart::Title)
    .chain(given.iter().map(|(part, _)| part_of(*part)))
    .chain(intent.iter().map(|_| EditedPart::Intent))
    .chain(order_text.iter().map(|_| EditedPart::Order))
    .collect();

  ground
    .hands
    .moved(
      draft.then_some(item.id),
      ground.wrote(item.id, if new_title.is_some() { vec!["title"] } else { vec![] }, vec![]),
    )
    .await;

  Ok(TodoAnswer::ItemEdited {
    id: item.id,
    title,
    changed,
  })
}

/// Items merged into another, which carries their work and finishes them with itself.
pub async fn merge(ground: &Ground<'_>, ids: &[i64], into: Option<i64>) -> Answered {
  let (Some(into), false) = (into, ids.is_empty()) else {
    return Err(refused(TodoRefusal::MergeNeedsItems));
  };
  let mut unique: Vec<i64> = vec![];

  for id in ids {
    if !unique.contains(id) {
      unique.push(*id);
    }
  }

  unique.sort_unstable();

  if let Some(refusal) = store::merge(ground.db, &unique, into, ground.now).await? {
    return Err(refused(match refusal {
      MergeRefusal::Missing(id) => TodoRefusal::NoSuchItem {
        selector: id.to_string(),
      },
      MergeRefusal::Itself(id) => TodoRefusal::MergeItself { id },
      MergeRefusal::Done(id) => TodoRefusal::MergeFinished { id },
      MergeRefusal::Merged { id, into } => TodoRefusal::ItemMerged { id, into },
      MergeRefusal::Running(id) => TodoRefusal::MergeRunning { id },
    }));
  }

  let title = store::row(ground.db, into).await?.map(|target| target.title).unwrap_or_default();

  ground.hands.moved(None, None).await;

  Ok(TodoAnswer::ItemsMerged {
    ids: unique,
    into,
    title,
  })
}

/// An item merged into another taken back out as work of its own.
pub async fn unmerge(ground: &Ground<'_>, selector: &str) -> Answered {
  let item = find(ground, selector).await?;

  if !store::unmerge(ground.db, item.id).await? {
    return Err(refused(match item.merged_into {
      None => TodoRefusal::NotMerged { id: item.id },
      Some(into) => TodoRefusal::MergedFinished { id: item.id, into },
    }));
  }

  ground.hands.moved(None, None).await;

  Ok(TodoAnswer::ItemUnmerged {
    id: item.id,
    title: item.title,
    priority: item.priority,
  })
}

/// An unfinished note resolved with one line of why.
pub async fn resolve(ground: &Ground<'_>, selector: &str, why: &str) -> Answered {
  let item = find(ground, selector).await?;
  let why = js::trim(why).to_owned();

  if why.is_empty() {
    return Err(refused(TodoRefusal::ResolveNeedsWhy));
  }

  if !resolve_unfinished(ground.db, item.id, &why, ground.now).await? {
    return Err(refused(match &item.unfinished {
      None => TodoRefusal::NoUnfinished { id: item.id },
      Some(_) => TodoRefusal::UnfinishedResolvedAlready {
        id: item.id,
        resolution: item.unfinished_resolution.clone().unwrap_or_default(),
      },
    }));
  }

  ground
    .hands
    .moved(None, ground.wrote(item.id, vec!["unfinished_resolution"], vec![]))
    .await;

  Ok(TodoAnswer::UnfinishedResolved {
    id: item.id,
    title: item.title,
    why,
  })
}

async fn sources_of(ground: &Ground<'_>, ids: &[i64]) -> Result<Vec<i64>, Refusal> {
  let mut found: Vec<i64> = vec![];

  for &id in ids {
    for source in sources(ground.db, id).await? {
      if !found.contains(&source) {
        found.push(source);
      }
    }
  }

  Ok(found)
}

/// A finished item opened again, with the items merged into it and the notes its closing had settled.
pub async fn reopen(ground: &Ground<'_>, selector: &str) -> Answered {
  let item = find(ground, selector).await?;
  let carried = store::reopen(ground.db, item.id).await?;
  let every: Vec<i64> = std::iter::once(item.id).chain(carried.iter().copied()).collect();
  let outstanding = reopen_settled_sources(ground.db, &sources_of(ground, &every).await?).await?;

  ground.hands.moved(None, None).await;

  Ok(TodoAnswer::ItemReopened {
    id: item.id,
    unmerged_from: item.merged_into,
    merged_again: carried,
    sources_reopened: outstanding,
  })
}

/// Why an item the owner wound down or stopped is not closed as done, or none where it may be.
pub fn done_refusal(item: &Row) -> Option<TodoRefusal> {
  let held = match item.held_kind.as_deref() {
    Some("pause") => HeldWord::Pause,
    Some("stop") => HeldWord::Stop,
    _ => return None,
  };

  Some(TodoRefusal::DoneWhileHeld { id: item.id, held })
}

// The order the agent is told when the resume conditions of held items are met.
fn resume_order(met: &[(Row, String)], owner: &str) -> String {
  let mut lines = vec![if met.len() == 1 {
    "The resume condition of a held item is met:".to_owned()
  } else {
    "The resume conditions of these held items are met:".to_owned()
  }];

  lines.extend(met.iter().map(|(item, by)| {
    let held = item
      .held_reason
      .as_ref()
      .map_or_else(String::new, |reason| format!(" (held: {reason})"));

    format!("- #{} {}{held}: {by}.", item.id, item.title)
  }));
  lines.push(
    "Nothing was released, so each still stands held. Where it should go on now, take its hold off and launch its job the moment its worker is free."
      .to_owned(),
  );
  lines.push(format!("Where something only {owner} can give still stands in its way, leave it held and put that one thing to {owner}."));
  lines.push(
    "Do not announce this order itself. What you say in the chat is which items you released and launched, or why one stays held."
      .to_owned(),
  );
  lines.join("\n")
}

async fn tell_met(ground: &Ground<'_>, met: &[(Row, String)]) -> Result<(), Refusal> {
  if met.is_empty() {
    return Ok(());
  }

  let summary = met
    .iter()
    .map(|(item, _)| format!("#{} {}", item.id, item.title))
    .collect::<Vec<_>>()
    .join(", ");

  queue_order(
    ground.units,
    Order {
      chat: Some(ground.owner_chat),
      text: resume_order(met, &ground.names.owner),
      sort: "resume_condition",
      summary: Some(summary),
      events: vec![],
    },
  )
  .await
  .map(std::mem::drop)
}

/// What a deployment that went live made ready: every held item whose commits are now all live, told to the
/// agent in one quiet order, its condition taken off the row so the next deployment does not tell it again;
/// the items told. `live` answers whether a commit stands in what went live.
pub async fn tell_deploy_conditions_met(
  ground: &Ground<'_>,
  live: &dyn Fn(&str) -> bool,
) -> Result<Vec<i64>, Refusal> {
  let met: Vec<(Row, String)> = store::awaiting_deploy(ground.db)
    .await?
    .into_iter()
    .filter(|(_, commits)| !commits.is_empty() && commits.iter().all(|commit| live(commit)))
    .map(|(item, commits)| {
      let named = commits
        .iter()
        .map(|commit| commit.chars().take(7).collect::<String>())
        .collect::<Vec<_>>()
        .join(", ");

      (item, format!("{named} {} live now", if commits.len() == 1 { "is" } else { "are" }))
    })
    .collect();

  tell_met(ground, &met).await?;

  let ids: Vec<i64> = met.iter().map(|(item, _)| item.id).collect();

  store::clear_resume_after_deploy(ground.db, &ids).await?;

  Ok(ids)
}

/// The account an item is closed with: a report and an unfinished note, each kept as it stands where it is
/// not given and cleared where it is given empty.
#[derive(Clone, Debug, Default)]
pub struct Closing {
  pub report: Option<String>,
  pub unfinished: Option<String>,
}

// The closing's readings of what a closing writes: none where the account may stand.
fn closing_refusal(
  ground: &Ground<'_>,
  report: Option<&str>,
  unfinished: Option<&str>,
  written_unfinished: bool,
) -> Option<String> {
  let reader = ground.reader;

  if written_unfinished && unfinished.is_some() {
    let unmarked = reader.unmarked_strands(unfinished);

    if !unmarked.is_empty() {
      return Some(reader.strand_format_refusal(&unmarked));
    }

    let host = reader.owner_host_strands(unfinished);

    if !host.is_empty() {
      return Some(reader.owner_host_refusal(&host));
    }
  }

  reader
    .leftover_refusal(report, unfinished)
    .or_else(|| reader.unchecked_refusal(report, unfinished))
    .or_else(|| outside_hosting_refusal(&[report, unfinished], &ground.names.owner))
}

/// The item closed with its account, and everything the closing settles: the items merged into it, the notes
/// it was the last open strand of, the strands left to the agent handed to it, and the items wound down to
/// wait on it. Its first answer is the closing; the rest say what it settled.
pub async fn done(
  ground: &Ground<'_>,
  selector: &str,
  asked: &Closing,
) -> Result<Vec<TodoAnswer>, Refusal> {
  let item = find(ground, selector).await?;

  if let Some(refusal) = done_refusal(&item) {
    return Err(refused(refusal));
  }

  let said = |given: &Option<String>, standing: &Option<String>| match given {
    None => standing.clone(),
    Some(value) => Some(js::trim(value).to_owned()).filter(|value| !value.is_empty()),
  };
  let openings = live_claude_openings(ground.db, item.id).await?;
  let report = report_with_live_claude(said(&asked.report, &item.report).as_deref(), &openings);
  let unfinished = said(&asked.unfinished, &item.unfinished);
  let written_report = asked.report.as_ref().and(report.as_deref());
  let written_unfinished = asked.unfinished.as_ref().and(unfinished.as_deref());

  if let Some(why) = closing_refusal(ground, written_report, written_unfinished, asked.unfinished.is_some())
  {
    return Err(refused(TodoRefusal::ClosingRefused { why }));
  }

  let carried = store::finish(
    ground.db,
    item.id,
    ground.now,
    report.as_deref(),
    unfinished.as_deref(),
    &merged_report(item.id),
  )
  .await?;
  let every: Vec<i64> = std::iter::once(item.id).chain(carried.iter().copied()).collect();
  let settled =
    resolve_settled_sources(ground.db, ground.reader, &sources_of(ground, &every).await?, ground.now)
      .await?;
  let handed = hand_over(ground, item.id, &item.title, unfinished.as_deref()).await?;
  let mut met: Vec<(Row, String)> = vec![];

  for &id in &every {
    for waiting in store::resuming_after(ground.db, id).await? {
      met.push((waiting, format!("#{id} is finished")));
    }
  }

  tell_met(ground, &met).await?;

  let written: Vec<ClosingPart> =
    [(ClosingPart::Report, report.is_some()), (ClosingPart::Unfinished, unfinished.is_some())]
      .into_iter()
      .filter_map(|(part, stands)| stands.then_some(part))
      .collect();
  let mut answers = vec![TodoAnswer::ItemClosed {
    id: item.id,
    written,
    carried,
  }];

  answers.extend(
    settled
      .into_iter()
      .map(|(id, resolution)| TodoAnswer::SourceResolved { id, resolution }),
  );
  answers.extend(handed.into_iter().map(|(memory, text)| TodoAnswer::StrandHanded {
    memory,
    head: text.split('\n').next().unwrap_or_default().to_owned(),
  }));
  answers.extend(met.into_iter().map(|(waiting, _)| TodoAnswer::ResumeConditionMet {
    id: waiting.id,
    title: waiting.title,
    after: item.id,
  }));

  let texts: Vec<&'static str> =
    [("report", asked.report.is_some()), ("unfinished", asked.unfinished.is_some())]
      .into_iter()
      .filter_map(|(text, given)| given.then_some(text))
      .collect();

  ground.hands.moved(None, ground.wrote(item.id, texts, vec![])).await;

  Ok(answers)
}

/// The owner closing an item he judges finished, from the console: the closing `done` makes, with the item's
/// own report and unfinished note kept as they stand. Refused where the item is finished or merged, wound
/// down or stopped, or a job still runs for it.
pub async fn close_as_owner(ground: &Ground<'_>, id: i64) -> Result<Vec<TodoAnswer>, Refusal> {
  let item = store::row(ground.db, id).await?.ok_or_else(|| {
    refused(TodoRefusal::NoSuchItem {
      selector: id.to_string(),
    })
  })?;

  if item.done {
    return Err(refused(TodoRefusal::ItemClosedAlready { id }));
  }

  if let Some(into) = item.merged_into {
    return Err(refused(TodoRefusal::ItemMerged { id, into }));
  }

  if let Some(job) = running_job_for(ground.db, id).await? {
    return Err(refused(TodoRefusal::EditJobRunning { id, job }));
  }

  done(ground, &id.to_string(), &Closing::default()).await
}

/// The item's one line of state written.
pub async fn mark(ground: &Ground<'_>, selector: &str, status: Option<&str>) -> Answered {
  let Some(status) = status.map(js::trim).filter(|status| !status.is_empty()) else {
    return Err(refused(TodoRefusal::MarkNeedsStatus));
  };
  let item = find(ground, selector).await?;

  store::set(ground.db, item.id, Column::Status, status.into()).await?;
  ground.hands.moved(None, ground.wrote(item.id, vec!["status"], vec![])).await;

  Ok(TodoAnswer::StatusSet {
    id: item.id,
    title: item.title,
    status: status.to_owned(),
  })
}

/// Which worker is to do the item; a dash puts it back to the built-in worker.
pub async fn worker(ground: &Ground<'_>, source: &Path, selector: &str, asked: &str) -> Answered {
  let item = find(ground, selector).await?;

  if asked.is_empty() {
    return Err(refused(TodoRefusal::WorkerNeedsName));
  }

  let chosen = (asked != "-").then_some(asked);
  let known = workers(ground.db, source).await?;

  if let Some(name) = chosen {
    let Some(named) = known.iter().find(|one| one.name == name) else {
      return Err(refused(TodoRefusal::NoSuchWorker {
        name: name.to_owned(),
        workers: known.iter().map(|one| one.name.clone()).collect(),
      }));
    };

    if let Some(base) = named.support_of.as_deref() {
      return Err(refused(TodoRefusal::WorkerIsSupport {
        worker: name.to_owned(),
        base: base.to_owned(),
      }));
    }
  }

  set_worker(ground.db, item.id, chosen).await?;
  ground.hands.moved(None, None).await;

  Ok(match chosen {
    None => TodoAnswer::WorkerCleared {
      id: item.id,
      title: item.title,
    },
    Some(worker) => TodoAnswer::WorkerSet {
      id: item.id,
      title: item.title,
      worker: worker.to_owned(),
    },
  })
}

// The item a chain of repairs began at: following what each repairs, back to one that repairs nothing.
async fn chain_root(ground: &Ground<'_>, id: i64) -> Result<Option<Row>, Refusal> {
  let mut seen: Vec<i64> = vec![];
  let mut item = store::row(ground.db, id).await?;

  while let Some(standing) = &item {
    let Some(next_id) = store::repairs_target(ground.db, standing.id).await? else { break };

    if seen.contains(&standing.id) {
      break;
    }

    seen.push(standing.id);

    match store::row(ground.db, next_id).await? {
      None => break,
      next => item = next,
    }
  }

  Ok(item)
}

// Every item filed to repair a root, however far down the chain, by number.
async fn chain_repairs(ground: &Ground<'_>, root: i64) -> Result<Vec<Row>, Refusal> {
  let mut found: Vec<Row> = vec![];
  let mut waiting = std::collections::VecDeque::from([root]);

  while let Some(next) = waiting.pop_front() {
    for repair in store::repairs_of(ground.db, next).await? {
      if repair.id == root || found.iter().any(|one| one.id == repair.id) {
        continue;
      }

      waiting.push_back(repair.id);
      found.push(repair);
    }
  }

  found.sort_by_key(|repair| repair.id);

  Ok(found)
}

// Why an item may not be linked as the repair of another, with the link written where it may.
async fn link_repair(
  ground: &Ground<'_>,
  id: i64,
  target: Option<i64>,
) -> Result<Option<TodoRefusal>, Refusal> {
  let absent = |id: i64| TodoRefusal::NoSuchItem {
    selector: id.to_string(),
  };

  if store::row(ground.db, id).await?.is_none() {
    return Ok(Some(absent(id)));
  }

  let Some(target) = target else {
    store::set(ground.db, id, Column::Repairs, Option::<i64>::None.into()).await?;

    return Ok(None);
  };

  if target == id {
    return Ok(Some(TodoRefusal::RepairsItself { id }));
  }

  if store::row(ground.db, target).await?.is_none() {
    return Ok(Some(absent(target)));
  }

  let own = chain_repairs(ground, id).await?;

  if own.iter().any(|repair| repair.id == target) {
    return Ok(Some(TodoRefusal::RepairsCycle { id, target }));
  }

  let Some(root) = chain_root(ground, target).await? else {
    return Ok(Some(absent(target)));
  };
  let others: Vec<Row> = chain_repairs(ground, root.id)
    .await?
    .into_iter()
    .filter(|repair| repair.id != id && !own.iter().any(|mine| mine.id == repair.id))
    .collect();

  if others.len() + 1 + own.len() > MAX_REPAIR_ROUNDS {
    return Ok(Some(TodoRefusal::RepairsChainFull {
      root: root.id,
      title: root.title,
      rounds: u32::try_from(others.len()).unwrap_or(u32::MAX),
    }));
  }

  store::set(ground.db, id, Column::Repairs, target.into()).await?;

  Ok(None)
}

/// Which item this one was filed to repair; a dash takes the link off.
pub async fn repairs(ground: &Ground<'_>, selector: &str, asked: &str) -> Answered {
  let item = find(ground, selector).await?;
  let target = if asked == "-" { None } else { Some(asked.trim().parse::<i64>().ok()) };

  if asked.is_empty() || target == Some(None) {
    return Err(refused(TodoRefusal::RepairsNeedsTarget));
  }

  let target = target.flatten();

  if let Some(refusal) = link_repair(ground, item.id, target).await? {
    return Err(refused(refusal));
  }

  ground.hands.moved(None, None).await;

  let cleared = || TodoAnswer::RepairsCleared {
    id: item.id,
    title: item.title.clone(),
  };
  let Some(target) = target else { return Ok(cleared()) };
  let root = chain_root(ground, item.id).await?;
  let repairs = match &root {
    Some(root) => chain_repairs(ground, root.id).await?,
    None => vec![],
  };

  Ok(match root.filter(|_| !repairs.is_empty()) {
    None => cleared(),
    Some(root) => TodoAnswer::RepairsSet {
      id: item.id,
      title: item.title.clone(),
      target,
      root: root.id,
      round: u32::try_from(repairs.len()).unwrap_or(u32::MAX),
      rounds: MAX_REPAIR_ROUNDS as u32,
    },
  })
}

/// Whether the item's job fans out into branches.
pub async fn cowork(ground: &Ground<'_>, selector: &str, asked: &str) -> Answered {
  let item = find(ground, selector).await?;

  if asked != "on" && asked != "off" {
    return Err(refused(TodoRefusal::CoworkNeedsSwitch));
  }

  set_cowork(ground.db, item.id, asked == "on").await?;
  ground.hands.moved(None, None).await;

  Ok(TodoAnswer::CoworkSet {
    id: item.id,
    title: item.title,
    on: asked == "on",
  })
}

/// The item's place in the queue.
pub async fn priority(ground: &Ground<'_>, selector: &str, asked: &str) -> Answered {
  let item = find(ground, selector).await?;
  let Ok(priority) = asked.trim().parse::<i64>() else {
    return Err(refused(TodoRefusal::PriorityNeedsNumber));
  };

  set_priority(ground.db, item.id, priority).await?;
  ground.hands.moved(None, None).await;

  Ok(TodoAnswer::PrioritySet {
    id: item.id,
    title: item.title,
    priority,
  })
}

/// A row that should never have been filed, thrown out; a note it was the last open strand of is settled.
/// Its first answer is the drop; the rest are the notes it settled.
pub async fn drop(ground: &Ground<'_>, selector: &str) -> Result<Vec<TodoAnswer>, Refusal> {
  let item = find(ground, selector).await?;
  let orphaned = sources(ground.db, item.id).await?;

  drop_item(ground.db, item.id).await?;

  let settled = resolve_settled_sources(ground.db, ground.reader, &orphaned, ground.now).await?;

  ground.hands.moved(None, None).await;

  Ok(
    std::iter::once(TodoAnswer::ItemDropped {
      id: item.id,
      title: item.title,
    })
    .chain(
      settled
        .into_iter()
        .map(|(id, resolution)| TodoAnswer::SourceResolved { id, resolution }),
    )
    .collect(),
  )
}

/// The model the item's job runs on.
pub async fn set_model(ground: &Ground<'_>, selector: &str, model: Option<&str>) -> Answered {
  let item = find(ground, selector).await?;
  let Some(model) = model.filter(|model| is_job_model(model, ground.providers)) else {
    return Err(refused(TodoRefusal::ModelNeeded));
  };

  store::set(ground.db, item.id, Column::Model, model.into()).await?;

  Ok(TodoAnswer::ModelSet {
    id: item.id,
    model: model.to_owned(),
  })
}
