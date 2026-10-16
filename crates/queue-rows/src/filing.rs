//! The verbs of filing (docs/rows.md, "The verbs of filing"): words added to an item and taken back, its
//! brief rewritten, an approval given or taken back, the words on an item read, and a row filed out of a
//! conversation or raised on the agent's own judgement out of the unfinished note an item left.

use job_agency::{TurnRecord, TurnRecordKind};
use job_base::Refusal;
use job_codes::todo_verbs::{FilingPart, StrandSource, TodoAnswer};
use job_codes::{JobError, ProviderRefusal, TodoRefusal};
use queue_read::worker_lines::job_author;

use crate::blocks::{
  Account, Entry, PickedStrand, clip_instruction, raised_by_item, raised_row_instruction,
  render_instruction, render_raised_instruction,
};
use crate::closing::reopen_settled_sources;
use crate::impact::impact_grounds;
use crate::instructions::{Order, QUEUE_CHANGED, queue_order};
use crate::order_lock::with_order_lock;
use crate::store::{self, Author, NewItem, Note};
use crate::todos::{
  Approval, Hand, Road, WAITING_STATUS, Who, approve as approve_item, at_work_for, running_job_for,
  set_brief, set_cowork, set_impact,
};
use crate::wording::banned_terms_in;
use crate::{Answered, Ground, WordsAdded, Written, find, js, refused, work_orders};

/// How long the clip of a filed item's first message runs, in UTF-16 units.
const CLIP: usize = 60;

// A run's turn record kept, where a run wrote what the record names.
async fn keep_record(
  ground: &Ground<'_>,
  kind: TurnRecordKind,
  text: &str,
  id: i64,
) -> Result<(), Refusal> {
  let Some(run) = ground.run_name else {
    return Ok(());
  };
  let record = TurnRecord {
    run: run.to_owned(),
    kind,
    text: text.to_owned(),
    record_id: Some(id),
    wakes: false,
    at: ground.now,
  };

  ground.units.keep_turn_record(&record).await
}

// Words written onto an item that is finished or merged reach nobody, so they are refused.
fn open_for_words(row: &store::Row) -> Result<(), Refusal> {
  if row.done {
    return Err(refused(TodoRefusal::ItemClosedAlready { id: row.id }));
  }

  match row.merged_into.filter(|_| row.merged()) {
    Some(into) => Err(refused(TodoRefusal::ItemMerged { id: row.id, into })),
    None => Ok(()),
  }
}

/// The words on an item: those standing, oldest first, and those taken back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notes {
  pub id: i64,
  pub title: String,
  pub standing: Vec<Note>,
  pub withdrawn: Vec<Note>,
}

/// The words added to an item, standing and taken back.
pub async fn notes(ground: &Ground<'_>, selector: &str) -> Result<Notes, Refusal> {
  let item = find(ground, selector).await?;
  let (withdrawn, standing): (Vec<Note>, Vec<Note>) = store::every_note(ground.db, item.id)
    .await?
    .into_iter()
    .partition(|note| note.removed_at.is_some());

  Ok(Notes {
    id: item.id,
    title: item.title,
    standing,
    withdrawn,
  })
}

/// Words to add to an item, and who added them where the verb's caller names somebody else than itself.
pub struct Append<'a> {
  pub text: &'a str,
  pub by: Option<&'a str>,
}

/// Words added to an item that still stands, with the job they landed in the middle of.
pub async fn append(ground: &Ground<'_>, selector: &str, asked: &Append<'_>) -> Answered {
  let item = find(ground, selector).await?;
  let said = js::trim(asked.text);

  if said.is_empty() {
    return Err(refused(TodoRefusal::AppendEmpty));
  }

  open_for_words(&item)?;

  let running = running_job_for(ground.db, item.id).await?;
  // A job's words are written under the job that wrote them, found by the run it works for. Only on that job's
  // own item are they the record a later run reads; on any other, like the words a person or a companion adds,
  // they reach no worker and are BMO's to fold in.
  let jobs_words = ground.in_job && asked.by.is_none();
  let writer = match ground.run_name.filter(|_| jobs_words) {
    Some(run) => at_work_for(ground.db, run).await?,
    None => None,
  };
  let items_record = writer.is_some_and(|(_, todo)| todo == Some(item.id));
  let job = if jobs_words { writer.map(|(job, _)| job) } else { running };
  let by = match asked.by {
    Some(by) => by.to_owned(),
    None if ground.in_job => job_author(job),
    None => ground.names.owner.clone(),
  };
  let author = Author {
    name: &by,
    sender: (by == ground.names.owner).then_some(ground.owner_sender),
  };
  let note = store::add_note(ground.db, item.id, author, said, job, ground.now).await?;

  keep_record(ground, TurnRecordKind::TodoAppend, said, item.id).await?;

  // A job's words on its own item are that item's record, and chat BMO's own turn folds what it adds into the
  // brief itself; anybody else's, a job's on an item not its own among them, are BMO's to fold in.
  let told = !items_record && !ground.chat_bmos();

  if told {
    let added = WordsAdded {
      todo: item.id,
      title: &item.title,
      note,
      by: &by,
      running,
    };

    ground.hands.words_added(&added).await?;
  }

  // Only the record a job wrote onto its own item is named for rendering; anybody else's words stay as said.
  let wrote = if items_record { ground.wrote(item.id, vec![], vec![note]) } else { None };

  ground.hands.moved(None, wrote).await;

  Ok(if told {
    TodoAnswer::WordsAppendedTold {
      id: item.id,
      title: item.title,
    }
  } else {
    TodoAnswer::WordsAppendedKept {
      id: item.id,
      title: item.title,
    }
  })
}

/// A brief rewritten whole, and what changed in it.
pub struct Brief<'a> {
  pub brief: &'a str,
  pub change: &'a str,
}

/// The item's brief rewritten by chat BMO to its final version, with the change named in English: the change
/// is kept as a note of its own kind with the job at work on the item, which is handed that change alone, and a
/// work order's instruction follows the brief.
pub async fn brief(ground: &Ground<'_>, selector: &str, asked: &Brief<'_>) -> Answered {
  if !ground.chat_bmos() {
    return Err(refused(TodoRefusal::BriefAgentOnly));
  }

  let item = find(ground, selector).await?;

  open_for_words(&item)?;

  let change = js::trim(asked.change);

  if change.is_empty() {
    return Err(refused(TodoRefusal::BriefNeedsChange));
  }

  let brief = js::trim(asked.brief);

  if brief.is_empty() {
    return Err(refused(TodoRefusal::BriefEmpty));
  }

  if !ground.reader.written_in_english(brief) || !ground.reader.written_in_english(change) {
    return Err(refused(TodoRefusal::BriefNotEnglish));
  }

  let running = running_job_for(ground.db, item.id).await?;
  let ordered = rewrite_order(ground, item.id, brief).await?;

  set_brief(ground.db, item.id, brief).await?;

  let note =
    store::add_change(ground.db, item.id, &ground.names.agent, change, running, ground.now).await?;
  // The agent's English is named for rendering whatever road ran the verb: the console reads it in the
  // owner's language, and the worker reads the English as it stands.
  let written = Written {
    todo: item.id,
    texts: vec!["brief"],
    notes: vec![note],
  };

  ground.hands.moved(ordered.then_some(item.id), Some(written)).await;

  Ok(TodoAnswer::BriefRewritten {
    id: item.id,
    title: item.title,
    job: running,
  })
}

// The item's work order's instruction rewritten to the brief, where the item has an order; whether it had one.
async fn rewrite_order(ground: &Ground<'_>, id: i64, brief: &str) -> Result<bool, Refusal> {
  with_order_lock(ground.db, id, async {
    let Some(mut order) = work_orders::read(ground.db, id).await? else {
      return Ok(false);
    };

    order["instruction"] = brief.into();
    work_orders::save(ground.db, &mut order).await?;

    Ok(true)
  })
  .await
}

/// Words added to an item taken back, by their note's number, marked so rather than deleted.
pub async fn unappend(
  ground: &Ground<'_>,
  selector: &str,
  note: Option<i64>,
  by: Option<&str>,
) -> Answered {
  let item = find(ground, selector).await?;
  let Some(note) = note else {
    return Err(refused(TodoRefusal::NoteNeedsNumber));
  };
  let by = by.unwrap_or(&ground.names.owner);
  let Some(carried) = store::remove_note(ground.db, item.id, note, by, ground.now).await? else {
    return Err(refused(TodoRefusal::NoSuchNote { id: item.id, note }));
  };

  ground.hands.moved(None, None).await;

  Ok(if carried {
    TodoAnswer::NoteRemovedAfterDelivery {
      id: item.id,
      title: item.title,
      note,
    }
  } else {
    TodoAnswer::NoteRemoved {
      id: item.id,
      title: item.title,
      note,
    }
  })
}

/// An approval to give or take back, and the words it carries: none keeps the words the item stands with,
/// and words with nothing in them clear them.
pub struct Approving<'a> {
  pub approve: bool,
  pub note: Option<&'a str>,
}

/// An approval given in the owner's word at a shell, or taken back; BMO told how the queue stands where it
/// was given.
pub async fn approve(ground: &Ground<'_>, selector: &str, asked: &Approving<'_>) -> Answered {
  let hand = Hand {
    who: shell_hand(ground),
    road: Road::Verb,
  };

  approve_as(ground, selector, asked, Approval::Owner, &hand).await
}

// Whose hand a shell's call is: the job it runs in, the agent whose turn runs it, or the person at it.
fn shell_hand<'a>(ground: &Ground<'a>) -> Who<'a> {
  if ground.in_job {
    return Who::Job(ground.run_name.unwrap_or("job"));
  }

  match ground.filer.agent {
    Some(agent) => Who::Agent(agent),
    None => Who::Person(ground.person.unwrap_or("shell")),
  }
}

/// An approval given the way it came, or taken back, kept in the item's record under the hand that made it.
pub async fn approve_as(
  ground: &Ground<'_>,
  selector: &str,
  asked: &Approving<'_>,
  way: Approval,
  hand: &Hand<'_>,
) -> Answered {
  let item = find(ground, selector).await?;
  let approval = asked.approve.then_some(way);
  let note = match asked.note {
    None => item.note.clone(),
    Some(said) => Some(js::trim(said).to_owned()).filter(|said| !said.is_empty()),
  };

  approve_item(ground.db, item.id, approval, note.as_deref(), hand, ground.now).await?;

  if asked.approve {
    ground.hands.queue_standing("approved", false).await?;
  }

  // The approval note is the owner's words as said, never rendered, so nothing of it is named.
  ground.hands.moved(None, None).await;

  Ok(if asked.approve {
    TodoAnswer::ItemApproved {
      id: item.id,
      noted: asked.note.is_some(),
    }
  } else {
    TodoAnswer::ItemUnapproved { id: item.id }
  })
}

/// A title or a part written around the conversation that uses a term the owner has set aside, refused.
pub fn wording_refusal(ground: &Ground<'_>, parts: &[Option<&str>]) -> Option<Refusal> {
  let text = parts.iter().flatten().copied().collect::<Vec<_>>().join("\n");
  let terms = banned_terms_in(&text, ground.banned);

  (!terms.is_empty()).then(|| {
    refused(TodoRefusal::BannedTerms {
      terms: terms.into_iter().map(|term| term.term).collect(),
    })
  })
}

/// Whether a job's model is one the queue can run: `auto`, a listed provider's own model, or a Claude model.
pub fn is_job_model(value: &str, providers: &[String]) -> bool {
  if value == "auto" {
    return true;
  }

  if let Some((provider, model)) = value.split_once("::") {
    let lead = model.chars().next().is_some_and(|first| first.is_ascii_alphanumeric());
    let tail = model
      .chars()
      .all(|character| character.is_ascii_alphanumeric() || "._:/-".contains(character));

    return providers.iter().any(|listed| listed == provider)
      && provider
        .chars()
        .all(|character| character.is_ascii_lowercase() || character.is_ascii_digit() || character == '-')
      && lead
      && tail
      && model.chars().count() <= 201;
  }

  value.strip_prefix("claude-").is_some_and(|rest| {
    !rest.is_empty()
      && rest
        .chars()
        .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
  })
}

/// A row to file: out of a conversation, or with a judgement out of the agent's own judgement.
#[derive(Clone, Debug, Default)]
pub struct Filing {
  pub title: Option<String>,
  /// The room, the owner's private chat where none.
  pub chat: Option<i64>,
  /// The job model, `auto` where none.
  pub model: Option<String>,
  pub status: Option<String>,
  pub cowork: bool,
  /// The impact grounds, as their JSON.
  pub impact: Option<String>,
  /// A conversation's parts.
  pub what: Option<String>,
  pub to_do: Option<String>,
  pub conclusion: Option<String>,
  pub instructions: Vec<i64>,
  pub replies: Vec<i64>,
  pub reply_text: Option<String>,
  pub requester: Option<String>,
  /// A judgement's parts: the judgement, the item whose work raised it as given, and the strands it came
  /// out of as `<item>:<strand>`, comma-separated.
  pub judgement: Option<String>,
  pub raised_by: Option<String>,
  pub from_todo: Option<String>,
}

/// A row filed: its number beside what the verb answers of it.
pub async fn add(ground: &Ground<'_>, filing: &Filing) -> Result<(i64, TodoAnswer), Refusal> {
  let model = filing.model.clone().unwrap_or_else(|| "auto".to_owned());

  if !is_job_model(&model, ground.providers) {
    return Err(JobError::from(ProviderRefusal::JobModelWrong { selection: model }).into());
  }

  let worded = [&filing.title, &filing.what, &filing.to_do, &filing.conclusion, &filing.judgement];

  if let Some(refusal) = wording_refusal(ground, &worded.map(|part| part.as_deref())) {
    return Err(refusal);
  }

  // Grounds that do not read refuse the filing before anything is written.
  let impact = filing.impact.as_deref().map(impact_grounds).transpose()?;
  let chat = filing.chat.unwrap_or(ground.owner_chat);
  let (id, answer) = match filing.judgement.as_deref() {
    Some(judgement) => raised(ground, filing, judgement, chat, &model).await?,
    None => filed(ground, filing, chat, &model).await?,
  };

  if let Some(grounds) = &impact {
    set_impact(ground.db, id, grounds).await?;
  }

  Ok((id, answer))
}

/// The strands a raised row names, as `<id>:<strand>` each; a bare id leaves the strand unnamed, and a part
/// that names no strand at all stands as none.
pub fn strand_picks(value: &str) -> Vec<Option<(i64, Option<usize>)>> {
  value
    .split(',')
    .map(str::trim)
    .filter(|part| !part.is_empty())
    .map(|part| {
      let (todo, strand) = match part.split_once(':') {
        None => (part, None),
        Some((todo, strand)) => (todo, Some(strand)),
      };
      let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());

      if !digits(todo)
        || strand.is_some_and(|strand| !digits(strand) || strand.parse::<usize>().ok() == Some(0))
      {
        return None;
      }

      Some((todo.parse().ok()?, strand.and_then(|strand| strand.parse().ok())))
    })
    .collect()
}

async fn save_draft(ground: &Ground<'_>, id: i64, intent: &str, instruction: &str) -> Result<(), Refusal> {
  let mut order = queue_read::work_order::draft(id, intent, instruction);

  work_orders::save(ground.db, &mut order).await
}

// The strands a raised row came out of, each read off the item that left it.
async fn picked(ground: &Ground<'_>, from: &str) -> Result<Vec<PickedStrand>, Refusal> {
  let picks = strand_picks(from);

  if picks.is_empty() || picks.iter().any(Option::is_none) {
    return Err(refused(TodoRefusal::FromTodoUnread));
  }

  let mut sources = vec![];

  for (todo_id, strand) in picks.into_iter().flatten() {
    let source = store::row(ground.db, todo_id)
      .await?
      .ok_or_else(|| refused(TodoRefusal::SourceNotItem { id: todo_id }))?;
    let note = source
      .unfinished
      .as_deref()
      .ok_or_else(|| refused(TodoRefusal::SourceLeftNothing { id: source.id }))?;

    sources.push(ground.reader.pick_strand(source.id, &source.title, note, strand)?);
  }

  Ok(sources)
}

async fn raised(
  ground: &Ground<'_>,
  filing: &Filing,
  judgement: &str,
  chat: i64,
  model: &str,
) -> Result<(i64, TodoAnswer), Refusal> {
  let spoken: Vec<FilingPart> = [
    (FilingPart::Instruction, !filing.instructions.is_empty()),
    (FilingPart::Reply, !filing.replies.is_empty() || filing.reply_text.is_some()),
    (FilingPart::What, filing.what.is_some()),
    (FilingPart::Do, filing.to_do.is_some()),
    (FilingPart::Conclusion, filing.conclusion.is_some()),
  ]
  .into_iter()
  .filter_map(|(part, given)| given.then_some(part))
  .collect();

  if !spoken.is_empty() {
    return Err(refused(TodoRefusal::JudgementWithConversation { parts: spoken }));
  }

  let said = js::trim(judgement);

  if said.is_empty() {
    return Err(refused(TodoRefusal::JudgementEmpty));
  }

  let Some(title) = filing.title.as_deref() else {
    return Err(refused(TodoRefusal::JudgementNeedsTitleAndRoom));
  };
  let given = filing.raised_by.as_deref().ok_or_else(|| refused(TodoRefusal::RaisedByNeeded))?;
  let raised_by = match js::int(given) {
    Some(id) => store::row(ground.db, id).await?,
    None => None,
  }
  .ok_or_else(|| {
    refused(TodoRefusal::RaisedByNotItem {
      given: given.to_owned(),
    })
  })?;
  let sources = match filing.from_todo.as_deref() {
    Some(from) => picked(ground, from).await?,
    None => vec![],
  };
  let mut source_ids: Vec<i64> = vec![];

  for source in &sources {
    if !source_ids.contains(&source.todo_id) {
      source_ids.push(source.todo_id);
    }
  }

  let instruction = render_raised_instruction(
    &ground.names.agent,
    said,
    ground.now,
    &raised_by_item(raised_by.id),
    &sources,
  );
  let id = store::add(
    ground.db,
    &NewItem {
      title,
      instruction: &instruction,
      requester: &ground.names.agent,
      requester_id: None,
      chat_id: Some(chat),
      status: filing.status.as_deref().unwrap_or(WAITING_STATUS),
      model,
      created_at: ground.now,
      sources: &source_ids,
      agent: ground.filer.agent,
    },
  )
  .await?;

  save_draft(ground, id, said, said).await?;

  if filing.cowork {
    set_cowork(ground.db, id, true).await?;
  }

  queue_order(
    ground.units,
    Order {
      chat: Some(ground.owner_chat),
      text: raised_row_instruction(
        id,
        title,
        &format!("The job of item #{}, from its own work,", raised_by.id),
        &source_ids,
        ground.reader.unfinished_word(),
        &ground.names.owner,
      ),
      sort: "raised_row",
      summary: Some(format!("#{id} {title}")),
      events: vec![QUEUE_CHANGED.to_owned()],
    },
  )
  .await?;

  let reopened = reopen_settled_sources(ground.db, &source_ids).await?;

  keep_record(ground, TurnRecordKind::TodoAdd, title, id).await?;
  ground
    .hands
    .moved(
      None,
      ground.in_job.then(|| Written {
        todo: id,
        texts: vec!["title", "judgement"],
        notes: vec![],
      }),
    )
    .await;

  let answer = TodoAnswer::ItemRaised {
    id,
    title: title.to_owned(),
    item: raised_by.id,
    sources: sources
      .iter()
      .map(|source| StrandSource {
        id: source.todo_id,
        strand: source.strand.to_string(),
      })
      .collect(),
    cowork: filing.cowork,
    reopened,
  };

  Ok((id, answer))
}

async fn filed(
  ground: &Ground<'_>,
  filing: &Filing,
  chat: i64,
  model: &str,
) -> Result<(i64, TodoAnswer), Refusal> {
  let blank = |part: &Option<String>| part.as_deref().is_none_or(|said| js::trim(said).is_empty());
  let missing: Vec<FilingPart> = [
    (FilingPart::Title, filing.title.is_none()),
    (FilingPart::What, blank(&filing.what)),
    (FilingPart::Do, blank(&filing.to_do)),
    (FilingPart::Conclusion, blank(&filing.conclusion)),
    (FilingPart::Instruction, filing.instructions.is_empty()),
    (FilingPart::Reply, filing.replies.is_empty() && filing.reply_text.is_none()),
  ]
  .into_iter()
  .filter_map(|(part, short)| short.then_some(part))
  .collect();
  let (Some(title), true) = (filing.title.as_deref(), missing.is_empty()) else {
    return Err(refused(TodoRefusal::FilingPartsMissing { missing }));
  };
  let what = js::trim(filing.what.as_deref().unwrap_or_default());
  let to_do = js::trim(filing.to_do.as_deref().unwrap_or_default());
  let conclusion = js::trim(filing.conclusion.as_deref().unwrap_or_default());
  let replier = ground.filer.label.unwrap_or(&ground.names.agent);
  let mut entries: Vec<Entry> = vec![];
  let mut first_sender: Option<i64> = None;
  let said = filing
    .instructions
    .iter()
    .map(|id| (*id, None))
    .chain(filing.replies.iter().map(|id| (*id, Some(replier))));

  for (message_id, label) in said {
    let message = ground.units.message(chat, message_id).await?.ok_or_else(|| {
      refused(TodoRefusal::MessageNotArchived {
        chat,
        message: message_id,
      })
    })?;

    if entries.is_empty() {
      first_sender = message.sender;
    }

    entries.push(Entry {
      sender: match label {
        Some(label) => label.to_owned(),
        None if message.sender == Some(ground.owner_sender) => ground.names.owner.clone(),
        None => message.sender_name.clone().unwrap_or_else(|| "unknown".to_owned()),
      },
      sent_at: message.sent_at,
      message_id: Some(message_id),
      text: message.text,
    });
  }

  let split = filing.instructions.len();
  let (asked, mut answered) = (entries[..split].to_vec(), entries[split..].to_vec());

  if let Some(text) = &filing.reply_text {
    answered.push(Entry {
      sender: replier.to_owned(),
      sent_at: ground.now,
      message_id: None,
      text: text.clone(),
    });
  }

  let first = &asked[0];
  let requester = filing.requester.clone().unwrap_or_else(|| first.sender.clone());
  let instruction = render_instruction(&Account {
    what,
    to_do,
    instructions: &asked,
    replies: &answered,
    conclusion,
    filer: ground.filer.label.unwrap_or(&ground.names.agent),
  });
  let id = store::add(
    ground.db,
    &NewItem {
      title,
      instruction: &instruction,
      requester: &requester,
      requester_id: first_sender,
      chat_id: Some(chat),
      status: filing.status.as_deref().unwrap_or(WAITING_STATUS),
      model,
      created_at: ground.now,
      sources: &[],
      agent: ground.filer.agent,
    },
  )
  .await?;

  save_draft(ground, id, conclusion, to_do).await?;

  if filing.cowork {
    set_cowork(ground.db, id, true).await?;
  }

  keep_record(ground, TurnRecordKind::TodoAdd, title, id).await?;
  ground.hands.moved(None, None).await;

  let answer = TodoAnswer::ItemFiled {
    id,
    title: title.to_owned(),
    cowork: filing.cowork,
    clip: clip_instruction(&first.text, CLIP),
  };

  Ok((id, answer))
}
