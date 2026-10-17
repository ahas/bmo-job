//! The TODO's verbs: refusals of a verb, what a verb answers, the fact a room is told when words are
//! added to an item, and the kinds their fields take.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::todo_reading::HeldWord;
use crate::{Refused, listed};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum TodoRefusal {
  #[error("a hold resumes after an item or after a deployment, not both")]
  ConditionBoth,
  #[error("item {id} is not another unfinished item to resume after ({why})")]
  AfterNotOpen { id: i64, why: AfterWrong },
  #[error("item {id} has no commit waiting for a deployment")]
  NoWaitingCommits { id: i64 },
  #[error("{selector} is not a commit of this repository")]
  NotACommit { selector: String },
  #[error("{selector} is not on main, so no deployment makes it live")]
  CommitNotOnMain { selector: String },
  #[error("{selector} is already live")]
  CommitAlreadyLive { selector: String },
  #[error("the edit names no part to change")]
  EditNothing,
  #[error("no deployment hold stands")]
  NoDeployHold,
  #[error("a plain hold names no item to resume after")]
  HoldTakesNoAfter,
  #[error("a stop needs its reason")]
  StopNeedsWhy,
  #[error("a wind-down needs its reason")]
  WindDownNeedsWhy,
  #[error("a stop-here needs its report")]
  StopHereNeedsReport,
  #[error("item {id} ({title}) is finished or merged")]
  StopHereClosed { id: i64, title: String },
  #[error("a job does not release a hold")]
  JobCannotUnhold,
  #[error("a merge names the items and the item to merge into")]
  MergeNeedsItems,
  #[error("a resolution needs its reason")]
  ResolveNeedsWhy,
  #[error("a mark needs its status line")]
  MarkNeedsStatus,
  #[error("the assignment names no worker and does not clear one")]
  WorkerNeedsName,
  #[error("{worker} is a support worker of {base}")]
  WorkerIsSupport { worker: String, base: String },
  #[error("a repair link is not given with a filing")]
  RepairsOnFiling,
  #[error("the repair link names no item and does not clear one")]
  RepairsNeedsTarget,
  #[error("co-work is set on or off")]
  CoworkNeedsSwitch,
  #[error("the impact grounds are not the one JSON object they are written as: {why}")]
  ImpactUnread { why: String },
  #[error("a priority is a number")]
  PriorityNeedsNumber,
  #[error("a model setting names auto or a model")]
  ModelNeeded,
  #[error("a row raised on judgement needs its title and its room")]
  JudgementNeedsTitleAndRoom,
  #[error("the filing lacks {}", listed(.missing))]
  FilingPartsMissing { missing: Vec<FilingPart> },
  #[error("the words use terms the owner has set aside: {}", listed(.terms))]
  BannedTerms { terms: Vec<String> },
  #[error("the words to add are empty")]
  AppendEmpty,
  #[error("only the chat agent rewrites a brief")]
  BriefAgentOnly,
  #[error("a brief comes with what changed")]
  BriefNeedsChange,
  #[error("the brief is empty")]
  BriefEmpty,
  #[error("a brief and its change are written in English")]
  BriefNotEnglish,
  #[error("no item answers to {selector}")]
  NoSuchItem { selector: String },
  #[error("item {id} is merged into item {into}, so its work goes there")]
  ItemMerged { id: i64, into: i64 },
  #[error("item {id} is finished, so what is written on it reaches nobody")]
  ItemClosedAlready { id: i64 },
  #[error("words are taken back by the number of their note")]
  NoteNeedsNumber,
  #[error("item {id} has no standing words numbered {note}")]
  NoSuchNote { id: i64, note: i64 },
  #[error("a row raised on judgement takes no part of a conversation: {}", listed(.parts))]
  JudgementWithConversation { parts: Vec<FilingPart> },
  #[error("the judgement is the row's reason for being and cannot be empty")]
  JudgementEmpty,
  #[error("a row raised on judgement names the item whose work raised it")]
  RaisedByNeeded,
  #[error("{given} is not an item")]
  RaisedByNotItem { given: String },
  #[error("the strands a row came out of are written as item:strand, comma-separated")]
  FromTodoUnread,
  #[error("{id} is not an item")]
  SourceNotItem { id: i64 },
  #[error("item {id} left no unfinished note to raise a row out of")]
  SourceLeftNothing { id: i64 },
  #[error("item {id} left {strands} strands in its unfinished note; name the one the row takes up")]
  StrandNotNamed { id: i64, strands: u32 },
  #[error("item {id} has no strand {strand} in its unfinished note; it has {strands}")]
  NoSuchStrand { id: i64, strand: u32, strands: u32 },
  #[error("message {message} is not kept in room {chat}")]
  MessageNotArchived { chat: i64, message: i64 },
  #[error("an edit writes a part again, so every part it names needs words")]
  EditPartEmpty,
  #[error("item {id} has job {job} at work on these words; add to it instead")]
  EditJobRunning { id: i64, job: i64 },
  #[error("item {id} has no {part} part to write again")]
  EditNoPart { id: i64, part: EditedPart },
  #[error("item {id}'s work order has started, and only a draft is written again")]
  EditOrderStarted { id: i64 },
  #[error("item {id} cannot be merged into itself")]
  MergeItself { id: i64 },
  #[error("item {id} is finished, and only unfinished items are merged, into an unfinished one")]
  MergeFinished { id: i64 },
  #[error("item {id} has a job at work; stop it or let it end before merging")]
  MergeRunning { id: i64 },
  #[error("item {id} is not merged")]
  NotMerged { id: i64 },
  #[error("item {id} was finished with item {into}; opening it again takes it out")]
  MergedFinished { id: i64, into: i64 },
  #[error("item {id} has no unfinished note to resolve")]
  NoUnfinished { id: i64 },
  #[error("item {id}'s unfinished note is resolved already: {resolution}")]
  UnfinishedResolvedAlready { id: i64, resolution: String },
  #[error(
    "item {id} is held ({held}), so it is not closed as done; leave where the run stopped with a stop-here"
  )]
  DoneWhileHeld { id: i64, held: HeldWord },
  #[error("the closing is refused: {why}")]
  ClosingRefused { why: String },
  #[error("no worker is called {name}; the workers are {}", listed(.workers))]
  NoSuchWorker { name: String, workers: Vec<String> },
  #[error("item {id} cannot repair itself")]
  RepairsItself { id: i64 },
  #[error("item {target} already repairs item {id}, so item {id} cannot repair it back")]
  RepairsCycle { id: i64, target: i64 },
  #[error(
    "item {root} ({title}) has had {rounds} repair rounds, where its chain stops: it is the owner's to look at"
  )]
  RepairsChainFull {
    root: i64,
    title: String,
    rounds: u32,
  },
  #[error("item {id} is finished, merged into another, or set aside already")]
  HoldNotTaken { id: i64 },
  #[error("item {id} is finished, merged into another, or stopped by the owner")]
  WindDownNotTaken { id: i64 },
  #[error("item {id} is held already, so a stop-here takes no reason and no resume condition")]
  StopHereHeldTakesNoReason { id: i64 },
  #[error("item {id} is neither held nor at work; where its work is finished, close it")]
  StopHereNotHeld { id: i64 },
  #[error("item {id} is not held; a run told in words to stop gives the reason it holds the item for")]
  StopHereNeedsReason { id: i64 },
  #[error("a run of item {id}'s work order does not hold its item here ({why})")]
  OrderStepHolds { id: i64, why: OrderHold },
  #[error("item {id} is not held")]
  NotHeld { id: i64 },
  #[error("a middle step of a work order does not close its item; the order's final turn closes it")]
  MiddleStepCloses,
  #[error("the order names other items than the open ones, as an order read before the queue moved does")]
  ReorderStale,
}

coded!(TodoRefusal);

impl Refused for TodoRefusal {
  fn status(&self) -> StatusCode {
    match self {
      Self::JobCannotUnhold | Self::BriefAgentOnly | Self::MiddleStepCloses => StatusCode::FORBIDDEN,
      Self::ReorderStale
      | Self::NoWaitingCommits { .. }
      | Self::CommitNotOnMain { .. }
      | Self::CommitAlreadyLive { .. }
      | Self::NoDeployHold
      | Self::StopHereClosed { .. }
      | Self::ItemMerged { .. }
      | Self::ItemClosedAlready { .. }
      | Self::SourceLeftNothing { .. }
      | Self::EditJobRunning { .. }
      | Self::EditOrderStarted { .. }
      | Self::MergeFinished { .. }
      | Self::MergeRunning { .. }
      | Self::NotMerged { .. }
      | Self::MergedFinished { .. }
      | Self::NoUnfinished { .. }
      | Self::UnfinishedResolvedAlready { .. }
      | Self::DoneWhileHeld { .. }
      | Self::RepairsCycle { .. }
      | Self::RepairsChainFull { .. }
      | Self::HoldNotTaken { .. }
      | Self::WindDownNotTaken { .. }
      | Self::StopHereNotHeld { .. }
      | Self::OrderStepHolds { .. }
      | Self::NotHeld { .. } => StatusCode::CONFLICT,
      Self::NoSuchItem { .. }
      | Self::NoSuchWorker { .. }
      | Self::NoSuchNote { .. }
      | Self::RaisedByNotItem { .. }
      | Self::SourceNotItem { .. }
      | Self::NoSuchStrand { .. }
      | Self::MessageNotArchived { .. } => StatusCode::NOT_FOUND,
      _ => StatusCode::BAD_REQUEST,
    }
  }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TodoAnswer {
  NoteRemoved {
    id: i64,
    title: String,
    note: i64,
  },
  NoteRemovedAfterDelivery {
    id: i64,
    title: String,
    note: i64,
  },
  ItemEdited {
    id: i64,
    title: String,
    changed: Vec<EditedPart>,
  },
  QueuePaused,
  QueueResumed,
  DeployHoldStands {
    hold: String,
  },
  DeployHoldReleased {
    hold: String,
  },
  ItemHeld {
    id: i64,
    title: String,
    reason: Option<String>,
  },
  ItemStopped {
    id: i64,
    title: String,
    runs: u32,
    reason: String,
  },
  ItemWoundDown {
    id: i64,
    title: String,
    reason: String,
    condition: Option<ResumeCondition>,
    job: Option<i64>,
  },
  StoppedHere {
    id: i64,
    title: String,
    note: i64,
    held: HeldWord,
    reason: Option<String>,
  },
  ItemUnheld {
    id: i64,
    title: String,
    priority: i64,
  },
  ItemsMerged {
    ids: Vec<i64>,
    into: i64,
    title: String,
  },
  ItemUnmerged {
    id: i64,
    title: String,
    priority: i64,
  },
  ItemClosed {
    id: i64,
    written: Vec<ClosingPart>,
    carried: Vec<i64>,
  },
  StrandHanded {
    memory: i64,
    head: String,
  },
  ResumeConditionMet {
    id: i64,
    title: String,
    after: i64,
  },
  ItemReopened {
    id: i64,
    unmerged_from: Option<i64>,
    merged_again: Vec<i64>,
    sources_reopened: Vec<i64>,
  },
  ItemApproved {
    id: i64,
    noted: bool,
  },
  ItemUnapproved {
    id: i64,
  },
  WorkerCleared {
    id: i64,
    title: String,
  },
  WorkerSet {
    id: i64,
    title: String,
    worker: String,
  },
  RepairsCleared {
    id: i64,
    title: String,
  },
  RepairsSet {
    id: i64,
    title: String,
    target: i64,
    root: i64,
    round: u32,
    rounds: u32,
  },
  CoworkSet {
    id: i64,
    title: String,
    on: bool,
  },
  ImpactSet {
    id: i64,
    title: String,
    contracts: u32,
    new_home: bool,
    verdict: Verdict,
    cowork: bool,
  },
  ImpactCheckKept {
    id: i64,
  },
  ImpactCheckTold {
    id: i64,
    difference: ImpactDifference,
  },
  ModelSet {
    id: i64,
    model: String,
  },
  ItemFiled {
    id: i64,
    title: String,
    cowork: bool,
    /// The first message the item was filed out of, on one line and cut where it runs long.
    clip: String,
  },
  ItemRaised {
    id: i64,
    title: String,
    item: i64,
    sources: Vec<StrandSource>,
    cowork: bool,
    reopened: Vec<i64>,
  },
  WordsAppendedTold {
    id: i64,
    title: String,
  },
  WordsAppendedKept {
    id: i64,
    title: String,
  },
  BriefRewritten {
    id: i64,
    title: String,
    job: Option<i64>,
  },
  UnfinishedResolved {
    id: i64,
    title: String,
    why: String,
  },
  SourceResolved {
    id: i64,
    resolution: String,
  },
  StatusSet {
    id: i64,
    title: String,
    status: String,
  },
  PrioritySet {
    id: i64,
    title: String,
    priority: i64,
  },
  ItemDropped {
    id: i64,
    title: String,
  },
}

coded!(TodoAnswer);

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, IntoStaticStr, VariantNames)]
#[serde(tag = "code", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum TodoFact {
  WordsAppended {
    id: i64,
    title: String,
    by: String,
    job: Option<i64>,
  },
}

coded!(TodoFact);

/// An item a raised row came out of, with the strand of its unfinished note.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StrandSource {
  pub id: i64,
  pub strand: String,
}

/// Why a run of a work order may not hold its own item.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum OrderHold {
  /// The run is a step of the order, which runs its steps through to the end.
  Step,
  /// Steps stand ahead of the final turn.
  StepsAhead,
  /// The order has already waited once on its final turn's hold.
  HeldOnce,
}

/// Why the item a hold resumes after is refused.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum AfterWrong {
  Itself,
  Finished,
}

/// A part of an item an edit rewrote.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum EditedPart {
  Title,
  What,
  Do,
  Conclusion,
  Judgement,
  Intent,
  Order,
}

/// A part a closing wrote on an item.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ClosingPart {
  Report,
  Unfinished,
}

/// What impact grounds come to.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum Verdict {
  SingleJob,
  Cowork,
}

/// How a job's own impact grounds differ from the item's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ImpactDifference {
  Differs { verdict: Verdict, cowork: bool },
  Unfiled,
}

/// A part a filing is made of.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum FilingPart {
  Title,
  What,
  Do,
  Conclusion,
  Instruction,
  Reply,
}

/// What a held item resumes after.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum ResumeCondition {
  ItemFinished { id: i64 },
  Deployed { what: String },
}

/// A record a verb writes into an item's words.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum WrittenRecord {
  /// `at` in epoch milliseconds.
  LiveClaude {
    at: i64,
    purpose: String,
    runs: u32,
  },
  StrandSettled {
    rows: Vec<i64>,
  },
  MergedReport {
    target: i64,
  },
  StoppedHere,
}
