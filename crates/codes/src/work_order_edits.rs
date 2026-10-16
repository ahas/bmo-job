//! A work order's plan, tasks, moves, recovery and state: the refusals of a change to it.

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

use crate::{Refused, listed};

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum WorkOrderRefusal {
  #[error("the plan is not a JSON object")]
  PlanNotObject,
  #[error("the plan's {part} is empty or too long")]
  PlanPartEmpty { part: PlanPart },
  #[error("the plan has no list of stages")]
  PlanNoStages,
  #[error("a stage has no task")]
  PlanStageEmpty,
  #[error("a task's role is none of {}", listed(.roles))]
  PlanRoleWrong { roles: Vec<String> },
  #[error("a model is not named as provider and model")]
  PlanModelForm,
  #[error("the synthesis builds more than {most} tasks")]
  PlanTooManyBuilds { most: u32 },
  #[error("the order has no task {task}")]
  TaskAbsent { task: String },
  #[error("the plan, synthesis and final turns are not steps to change")]
  ControlTurnUntouched,
  #[error("the order is finished")]
  OrderFinished,
  #[error("the order is a draft that has not run")]
  OrderDraft,
  #[error("the order is winding down and not yet finished")]
  OrderClosing,
  #[error("a task's instruction is 1 to 30000 characters")]
  InstructionSize,
  #[error("the model tier is none of {}", listed(.tiers))]
  ModelTierWrong { tiers: Vec<String> },
  #[error("the co-worker tier is none of {} ({} are for a worker only)", listed(.tiers), listed(.worker_only))]
  CoworkerTierWrong {
    tiers: Vec<String>,
    worker_only: Vec<String>,
  },
  #[error("a task of an earlier round is not edited")]
  HistoryEdit,
  #[error("a task of an earlier round is not run again")]
  HistoryRerun,
  #[error("a task of an earlier round is not restored")]
  HistoryRestore,
  #[error("a running task's stored instruction stays as it is; item {item} is running")]
  RunningTaskEdit { item: i64 },
  #[error("task {task} is finished; run it again instead")]
  DoneTaskEdit { task: String },
  #[error("the edit names no instruction and no model")]
  NothingToEdit,
  #[error("a skip reason is 1 to 2000 characters")]
  SkipReasonSize,
  #[error("task {task} is already skipped")]
  TaskAlreadySkipped { task: String },
  #[error("a builder turn of item {item} is running")]
  BuilderRunning { item: i64 },
  #[error("the stage of task {task} has not come yet")]
  StageNotYet { task: String },
  #[error("the present stage has a task running")]
  StageRunning,
  #[error("a build task is running")]
  BuildRunning,
  #[error("the research phase is over")]
  ResearchOver,
  #[error("the order's present phase does not run the task again")]
  PhaseWrong,
  #[error("task {task} is running")]
  TaskRunning { task: String },
  #[error("task {task} is already pending")]
  TaskPending { task: String },
  #[error("task {task} is not skipped")]
  TaskNotSkipped { task: String },
  #[error("a task is not moved relative to itself")]
  SameAnchor,
  #[error("a new task is placed beside a pending build task")]
  AddAnchor,
  #[error("a build task moves only while it and its anchor are pending")]
  BuildMove,
  #[error("a stage moves whole, not one task of it")]
  TaskMove,
  #[error("a stage at or before the present one does not move")]
  StageMovePast,
  #[error("a stage with a task not pending does not move")]
  StageMovePending,
  #[error("a task moves among tasks of its own sort")]
  MoveKinds,
  #[error("a stop reason is 1 to 2000 characters")]
  StopReasonSize,
  #[error("a close reason is 1 to 2000 characters")]
  CloseReasonSize,
  #[error("a builder turn of item {item} is running")]
  CloseBuilderRunning { item: i64 },
  #[error("only a finished order is recovered")]
  RecoverNotDone,
  #[error("the range holds no job of the item")]
  RecoverNoJobs,
  #[error("the order already records job {job}")]
  RecoverJobKnown { job: i64 },
  #[error("job {job} has not ended")]
  RecoverJobRunning { job: i64 },
  #[error("job {job} ({unit}) is no step of the item's work order")]
  RecoverNotStep { job: i64, unit: String },
  #[error("the prompt of job {job} on {base} does not give the role and instruction")]
  RecoverPromptUnread { job: i64, base: String },
  #[error("task {task} is already in the order")]
  RecoverTaskTwice { task: String },
  #[error("the range holds no step that succeeded")]
  RecoverNoSuccess,
  #[error("the range holds no builder turn to read the purpose and instruction from")]
  RecoverNoBuilder,
  #[error("the order of item {item} is stopped and waits for its turn in its worker's line")]
  OrderPaused { item: i64 },
  #[error("{name} is not a string")]
  FieldNotText { name: String },
  #[error("{name} is missing")]
  FieldMissing { name: String },
  #[error("runs are not yet confirmed ended: {}", listed(.alive))]
  RunsStillAlive { alive: Vec<String> },
  #[error("a result document already stands at {path}")]
  DocumentExists { path: String },
  #[error("only a finished order is reopened")]
  ReopenNotDone,
  #[error("only a draft is changed as a draft")]
  DraftOnly,
  #[error("the order or its model does not read")]
  OrderAndModel,
  #[error("the item is not approved and ready to run")]
  ItemNotApproved,
  #[error("the order of item {item} is already running its steps")]
  OrderActive { item: i64 },
  #[error("the order of item {item} is finished")]
  OrderDone { item: i64 },
  #[error("the chat id is not a room's")]
  ChatWrong,
  #[error("{cwd} is not a git repository")]
  CheckoutNotGit { cwd: String },
  #[error("the item number is not a whole number")]
  ItemNumberWrong,
  #[error("there is no such item")]
  ItemAbsent,
  #[error("the work is not approved, or is held or merged")]
  ResumeNotHeld,
  #[error("the work is not stopped")]
  ResumeNotStopped,
  #[error("a move names before or after, not both")]
  BeforeAndAfter,
  #[error("a finished order's checkout stays as it is")]
  CheckoutOfDone,
  #[error("a job range is a first and a last job")]
  JobRangeForm,
  #[error("a finished item is not stopped")]
  StopDoneItem,
}

coded!(WorkOrderRefusal);

impl Refused for WorkOrderRefusal {
  fn status(&self) -> StatusCode {
    match self {
      Self::PlanNotObject
      | Self::PlanPartEmpty { .. }
      | Self::PlanNoStages
      | Self::PlanStageEmpty
      | Self::PlanRoleWrong { .. }
      | Self::PlanModelForm
      | Self::PlanTooManyBuilds { .. }
      | Self::InstructionSize
      | Self::ModelTierWrong { .. }
      | Self::CoworkerTierWrong { .. }
      | Self::NothingToEdit
      | Self::SkipReasonSize
      | Self::SameAnchor
      | Self::AddAnchor
      | Self::TaskMove
      | Self::MoveKinds
      | Self::StopReasonSize
      | Self::CloseReasonSize
      | Self::RecoverNotStep { .. }
      | Self::FieldNotText { .. }
      | Self::FieldMissing { .. }
      | Self::OrderAndModel
      | Self::ChatWrong
      | Self::ItemNumberWrong
      | Self::BeforeAndAfter
      | Self::JobRangeForm => StatusCode::BAD_REQUEST,
      Self::TaskAbsent { .. }
      | Self::RecoverNoJobs
      | Self::RecoverNoSuccess
      | Self::RecoverNoBuilder
      | Self::ItemAbsent => StatusCode::NOT_FOUND,
      _ => StatusCode::CONFLICT,
    }
  }
}

/// A part of a plan.
#[derive(
  Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames,
)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum PlanPart {
  StageTitle,
  Model,
  TaskInstruction,
  TaskTitle,
  BuildInstruction,
}
