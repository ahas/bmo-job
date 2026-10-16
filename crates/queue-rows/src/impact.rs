//! An item's impact grounds as the verb writes them (docs/rows.md, "The verbs of filing"): the grounds BMO
//! weighed the co-work setting on, and the grounds a job of the item weighs again before it changes
//! anything, told to BMO where they come to another verdict than the setting the item runs at, or where BMO
//! filed none.

use job_base::Refusal;
use job_codes::TodoRefusal;
use job_codes::todo_verbs::{ImpactDifference, TodoAnswer, Verdict as Coded};
use queue_read::impact::{ImpactGrounds, Verdict};

use crate::instructions::{Order, queue_order};
use crate::todos::{Item, item, set_impact, set_job_impact};
use crate::{Answered, Ground, find, no_item, refused};

/// What the console calls the instruction a job's grounds raise.
const NOTICE_SORT: &str = "impact_mismatch";

/// The grounds written as JSON, held to their shape.
pub fn impact_grounds(written: &str) -> Result<ImpactGrounds, Refusal> {
  ImpactGrounds::read(written).map_err(|fault| {
    refused(TodoRefusal::ImpactUnread {
      why: fault.to_string(),
    })
  })
}

// The item a selector names, as the queue reads it, and the grounds the call gives.
async fn item_and_grounds(
  ground: &Ground<'_>,
  selector: &str,
  written: &str,
) -> Result<(Item, ImpactGrounds), Refusal> {
  let found = find(ground, selector).await?;
  let grounds = impact_grounds(written)?;
  let item = item(ground.db, found.id).await?.ok_or_else(|| no_item(selector))?;

  Ok((item, grounds))
}

// The setting an item runs at, as the verdict it stands for.
fn setting(item: &Item) -> Verdict {
  if item.cowork { Verdict::Cowork } else { Verdict::Single }
}

fn coded(verdict: Verdict) -> Coded {
  match verdict {
    Verdict::Single => Coded::SingleJob,
    Verdict::Cowork => Coded::Cowork,
  }
}

/// The grounds the item's co-work setting is weighed on, and the verdict they come to beside the setting it
/// stands at, which the grounds never move.
pub async fn impact(ground: &Ground<'_>, selector: &str, written: &str) -> Answered {
  let (item, grounds) = item_and_grounds(ground, selector, written).await?;

  set_impact(ground.db, item.id, &grounds).await?;
  ground.hands.moved(None, None).await;

  Ok(TodoAnswer::ImpactSet {
    id: item.id,
    title: item.title.clone(),
    contracts: u32::try_from(grounds.contract_changes()).unwrap_or(u32::MAX),
    new_home: grounds.new_home,
    verdict: coded(grounds.verdict()),
    cowork: item.cowork,
  })
}

/// The grounds a job of the item weighed again, kept beside BMO's; BMO is told where they come to another
/// verdict than the setting the item runs at, or where it filed the item with none, and nobody is told where
/// only the nodes differ. The setting stays BMO's to change.
pub async fn impact_check(ground: &Ground<'_>, selector: &str, written: &str) -> Answered {
  let (item, found) = item_and_grounds(ground, selector, written).await?;

  set_job_impact(ground.db, item.id, &found).await?;
  ground.hands.moved(None, None).await;

  let differs = found.verdict() != setting(&item);

  if !differs && item.impact.is_some() {
    return Ok(TodoAnswer::ImpactCheckKept { id: item.id });
  }

  queue_order(
    ground.units,
    Order {
      chat: Some(ground.owner_chat),
      text: impact_notice(&item, &found),
      sort: NOTICE_SORT,
      summary: Some(format!("#{} {}", item.id, item.title)),
      events: vec![],
    },
  )
  .await?;

  let difference = if differs {
    ImpactDifference::Differs {
      verdict: coded(found.verdict()),
      cowork: item.cowork,
    }
  } else {
    ImpactDifference::Unfiled
  };

  Ok(TodoAnswer::ImpactCheckTold {
    id: item.id,
    difference,
  })
}

// A verdict in the agent's English.
fn verdict_word(verdict: Verdict) -> &'static str {
  match verdict {
    Verdict::Single => "a single job",
    Verdict::Cowork => "a co-work",
  }
}

// How an item runs, by its setting.
fn running(item: &Item) -> &'static str {
  if item.cowork { "as a co-work" } else { "as a single job" }
}

// A set of grounds as lines under a head, with the verdict they come to.
fn grounds_lines(head: &str, grounds: &ImpactGrounds) -> Vec<String> {
  [head.to_owned()]
    .into_iter()
    .chain(grounds.lines())
    .chain([format!("They come to {}.", verdict_word(grounds.verdict()))])
    .collect()
}

/// What BMO is told when a job's grounds come to another verdict than the setting the item runs at, or the
/// item carries none of BMO's: both sets of grounds and both verdicts, whose the setting is, and when a
/// change to it reaches the work.
pub fn impact_notice(item: &Item, found: &ImpactGrounds) -> String {
  let id = item.id;
  let filed = match &item.impact {
    Some(grounds) => grounds_lines("The grounds you filed it with:", grounds),
    None => vec!["You filed it with no grounds of your own.".to_owned()],
  };

  [format!(
    "The job of item #{id} {} weighed the item's impact again before changing anything. The item runs {}.",
    item.title,
    running(item)
  )]
  .into_iter()
  .chain(filed)
  .chain(grounds_lines("The grounds the job found:", found))
  .chain([
    format!(
      "The co-work setting is yours alone to change. Weigh the two, then turn it with bmo todo --cowork {id} on|off or leave it as it stands, and write the grounds you settle on with bmo todo --impact {id} '<grounds JSON>'."
    ),
    "The running job carries on as it was launched: a changed setting reaches the item's next run, or this one at once if you stop the job and launch it again.".to_owned(),
    "Do not announce the instruction itself. What you say in the chat is what you changed, if anything.".to_owned(),
  ])
  .collect::<Vec<_>>()
  .join("\n")
}
