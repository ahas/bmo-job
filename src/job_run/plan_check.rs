//! A work step's shaping turn and the check of its answer (docs/queue.md, "The work-order runner"): the
//! request the turn after a control turn is sent — the shape its answer is held to, a refused answer's
//! reason, and the answer being shaped — and the answer read as a plan or as the tasks a synthesis builds,
//! refused with the validator's words where it does not hold, and an answer that holds read into the stages
//! or the build tasks the order runs. The tiers and the roles a plan may name are bmo-agency's, read once as
//! a roster.

use job_agency::{PoolRole, Tier};
use job_codes::WorkOrderRefusal;
use job_codes::work_order_edits::PlanPart;
use job_policy::Policy;
use serde_json::{Map, Value};

use crate::Refusal;
use crate::queue::units::Units;
use crate::queue::work_orders::{self as orders, Tiers};

/// The structure a plan's control turn answers in.
pub const PLAN: &str = "plan";

// The shape each control turn's answer is asked in.
const PLAN_SHAPE: &str = r#"{"stages":[{"title":"Investigate","kind":"investigate","tasks":[{"title":"what this one task is called","role":"scout","model":null,"instruction":"what this one task investigates"}]}]}"#;
const SYNTHESIS_SHAPE: &str = r#"{"tasks":[{"title":"what this one piece of work is called","instruction":"one thing to implement and what finishes it"}]}"#;

// The kinds a plan names its stages by, the first standing for a stage given none the order knows.
const STAGE_KINDS: [&str; 2] = ["investigate", "review"];

/// The role that is a turn of the worker itself rather than a co-worker borrowed from the pool.
pub const BUILDER: &str = "builder";

// How many things a synthesis builds. A plan holds as many stages, and a stage as many tasks, as its design
// asks for: a stage wider than the pool waits its turn there rather than being refused.
const MAX_BUILD_TASKS: usize = 64;

// How long a text field may run, in UTF-16 units as the order's own reader counts them.
const INSTRUCTION_LIMIT: usize = 30_000;
const TITLE_LIMIT: usize = 300;
const MODEL_LIMIT: usize = 200;
// How much of a task's name is kept: it stands on one line.
const TITLE_CUT: usize = 200;

/// The tiers and the roles of the co-worker pool, as bmo-agency answers them, read once for a judgement.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Roster {
  /// The ladder, in its order.
  pub tiers: Vec<Tier>,
  pub roles: Vec<PoolRole>,
}

impl Roster {
  /// The roster as bmo-agency answers it now.
  pub async fn read(units: &dyn Units) -> Result<Self, Refusal> {
    Ok(Self {
      tiers: units.tiers().await?,
      roles: units.pool_roles().await?,
    })
  }

  /// The ladder's tiers, in order.
  pub fn ladder(&self) -> Vec<&str> {
    self.tiers.iter().map(|tier| tier.name.as_str()).collect()
  }

  /// Whether a value is a tier of the ladder.
  pub fn holds(&self, tier: &str) -> bool {
    self.tiers.iter().any(|one| one.name == tier)
  }

  /// Whether a co-worker is lent at a tier.
  pub fn lends(&self, tier: &str) -> bool {
    self.tiers.iter().any(|one| one.name == tier && one.coworker)
  }

  /// The tiers a co-worker is lent at, in the ladder's order.
  pub fn coworker_tiers(&self) -> Vec<&str> {
    self.tiers.iter().filter(|tier| tier.coworker).map(|tier| tier.name.as_str()).collect()
  }

  /// The tiers as the order's edit verbs take them.
  pub fn edit_tiers(&self) -> Tiers {
    Tiers {
      ladder: self.ladder().into_iter().map(str::to_owned).collect(),
      coworker: self.coworker_tiers().into_iter().map(str::to_owned).collect(),
    }
  }

  /// The tier a role runs at by default, which must stand on the ladder.
  pub fn role_tier(&self, role: &str) -> Result<String, String> {
    let named = self
      .roles
      .iter()
      .find(|one| one.name == role)
      .ok_or_else(|| format!("Missing tier assignment: role:{role}"))?;

    if self.holds(&named.tier) {
      Ok(named.tier.clone())
    } else {
      Err(format!("Unknown model tier: {}", named.tier))
    }
  }

  /// The roles a plan may name: every enabled role the pool lends, and the builder, which it never lends.
  pub fn work_roles(&self) -> Vec<String> {
    self
      .roles
      .iter()
      .filter(|role| role.enabled && (if role.name == BUILDER { !role.pooled } else { role.pooled }))
      .map(|role| role.name.clone())
      .collect()
  }

  // Whether a task of this role may run at this tier: the builder takes any, every other role a
  // co-worker's.
  fn tier_fits_role(&self, role: &str, tier: &str) -> bool {
    role == BUILDER || self.lends(tier)
  }
}

fn refused(refusal: WorkOrderRefusal) -> String {
  refusal.to_string()
}

/// What the turn after a control turn is sent: the shape its answer is held to, the reason the last answer
/// was refused where one was, and the answer being shaped, which goes with it so a run that does not
/// continue the session that wrote it still has something to shape.
pub fn structure_request(roster: &Roster, kind: &str, previous: &str, refused: Option<&str>) -> String {
  let shape = if kind == PLAN { PLAN_SHAPE } else { SYNTHESIS_SHAPE };
  let held = if kind == PLAN {
    format!(
      "Every stage holds a title, a kind — investigate for a stage that finds things out, review for one that checks a finding or the plan — and one or more tasks, and every task holds exactly title, role, model and instruction, each a string except model. Choose each role from {}. Model is null where the tier [Model tiers] names is the one you want, and otherwise exactly one of {} for builder and one of {} for every other role.",
      roster.work_roles().join(", "),
      roster.ladder().join(", "),
      roster.coworker_tiers().join(", ")
    )
  } else {
    "Every task holds exactly title and instruction, both strings; an empty list is a valid answer."
      .to_owned()
  };
  let order = [
    format!("[Shape the answer]\nGive what you have just decided as one JSON object in this shape and nothing else: {shape}."),
    "No explaining sentence and no preamble. Do not investigate anything new and do not edit any file — carry over what you just decided, word for word. Never translate material you did not write; anything you quote keeps its original language.".to_owned(),
    held,
  ]
  .join(" ");
  let correction = refused.map(|why| {
    format!(
      "[JSON correction]\nThe previous answer failed validation. The error is quoted here as it was raised, in its own language: {why}\nCheck your answer with JSON.stringify and JSON.parse against the shape above, then give it again."
    )
  });

  std::iter::once(order)
    .chain(correction)
    .chain([format!("[Previous answer]\n{previous}")])
    .collect::<Vec<_>>()
    .join("\n\n")
}

/// The answer as one JSON object, a fence around it taken off.
pub fn object(text: &str) -> Result<Map<String, Value>, String> {
  let trimmed = text.trim();
  let opened = trimmed
    .strip_prefix("```json")
    .or_else(|| trimmed.strip_prefix("```"))
    .map_or(trimmed, str::trim_start);
  let clean = opened.strip_suffix("```").map_or(opened, str::trim_end);

  match serde_json::from_str::<Value>(clean).map_err(|why| format!("JSON Parse error: {why}"))? {
    Value::Object(object) => Ok(object),
    _ => Err(refused(WorkOrderRefusal::PlanNotObject)),
  }
}

/// A text field that holds something and is no longer than its limit, trimmed.
pub fn nonempty(value: Option<&Value>, part: PlanPart, max: usize) -> Result<String, String> {
  match value.and_then(Value::as_str) {
    Some(text) if !text.trim().is_empty() && text.encode_utf16().count() <= max => {
      Ok(text.trim().to_owned())
    }
    _ => Err(refused(WorkOrderRefusal::PlanPartEmpty { part })),
  }
}

/// A stage as a plan wrote it: its title, its kind and its tasks.
pub type WrittenStage = (String, &'static str, Vec<Value>);

/// A builder task is a later turn of the worker's one session, so a stage holding one beside anything else
/// is split: the other roles first as one stage under its title, each builder task after in a stage of its
/// own, numbered on from it, every part of the kind the stage was.
pub fn split_stages(stages: Vec<WrittenStage>) -> Vec<WrittenStage> {
  let builds = |task: &Value| task.get("role").and_then(Value::as_str) == Some(BUILDER);

  stages
    .into_iter()
    .flat_map(|(title, kind, tasks)| {
      if tasks.len() == 1 || !tasks.iter().any(builds) {
        return vec![(title, kind, tasks)];
      }

      let (builders, others): (Vec<Value>, Vec<Value>) = tasks.into_iter().partition(builds);
      let parts: Vec<Vec<Value>> = (!others.is_empty())
        .then_some(others)
        .into_iter()
        .chain(builders.into_iter().map(|task| vec![task]))
        .collect();

      parts
        .into_iter()
        .enumerate()
        .map(|(at, part)| {
          (if at == 0 { title.clone() } else { format!("{title} ({})", at + 1) }, kind, part)
        })
        .collect()
    })
    .collect()
}

/// The name a plan gave one task, cut to a line; none where it gave none, which is no reason to refuse it.
pub fn plan_title(value: Option<&Value>) -> Option<String> {
  value
    .and_then(Value::as_str)
    .map(str::trim)
    .filter(|title| !title.is_empty())
    .map(|title| title.encode_utf16().take(TITLE_CUT).collect::<Vec<u16>>())
    .map(|title| String::from_utf16_lossy(&title))
}

// A task as a plan or a synthesis makes one, named where it was given a name.
fn planned_task(id: &str, role: &str, model: &str, instruction: &str, title: Option<String>) -> Value {
  let mut task = orders::task(id, role, model, instruction);

  if let Some(title) = title {
    task["title"] = Value::String(title);
  }

  task
}

/// What a stage's task is written at, out of the model its plan named and the selection the planning turn
/// ran on; none leaves it following its role's tier. The planner copying its own model onto a task, or
/// naming a tier the role is never borrowed at, is no choice about that task. A builder task given no
/// choice comes before the build, where a review stage checks what it decides with a cheaper role, so it
/// runs at the stage builders' tier rather than its role's.
pub fn chosen_model(
  roster: &Roster,
  policy: &Policy,
  role: &str,
  named: Option<String>,
  model: &str,
) -> Option<String> {
  let copied = named.as_deref().is_some_and(|named| {
    (named == model && !roster.holds(named)) || (roster.holds(named) && !roster.tier_fits_role(role, named))
  });

  match named {
    Some(named) if !copied => Some(named),
    _ if role == BUILDER => Some(policy.queue.stage_builder_tier.clone()),
    _ => None,
  }
}

/// A stage's kind as its plan named it: one the order does not know, or none, is no reason to refuse a
/// design over what only a reader is told, and reads as the first.
pub fn stage_kind(value: Option<&Value>) -> &'static str {
  let named = value.and_then(Value::as_str).map(str::trim);

  STAGE_KINDS.into_iter().find(|kind| Some(*kind) == named).unwrap_or(STAGE_KINDS[0])
}

/// The stages a plan wrote, each its title, its kind and its tasks: a list of stages, each a titled object
/// holding one task or more. How many stages and how many tasks a stage are the design's; only a stage with
/// nothing to run is refused.
pub fn written_stages(data: &Map<String, Value>) -> Result<Vec<WrittenStage>, String> {
  let Some(Value::Array(stages)) = data.get("stages") else {
    return Err(refused(WorkOrderRefusal::PlanNoStages));
  };

  stages
    .iter()
    .map(|stage| match stage.get("tasks") {
      Some(Value::Array(tasks)) if stage.is_object() && !tasks.is_empty() => Ok((
        nonempty(stage.get("title"), PlanPart::StageTitle, TITLE_LIMIT)?,
        stage_kind(stage.get("kind")),
        tasks.clone(),
      )),
      _ => Err(refused(WorkOrderRefusal::PlanStageEmpty)),
    })
    .collect()
}

/// A selection as a provider and its model: `<provider>::<model>`, with something on both sides.
fn is_selection(value: &str) -> bool {
  value
    .split_once("::")
    .is_some_and(|(provider, model)| !provider.is_empty() && !model.is_empty())
}

/// A plan read and checked as the order will run it: its stages each holding a task or more, each task's
/// role one the plan may name, its model a tier the role takes or a selection, and its instruction there;
/// the stages as the order holds them, or the validator's words where it does not hold. `model` is the
/// selection the planning turn ran on.
pub fn read_plan(roster: &Roster, policy: &Policy, text: &str, model: &str) -> Result<Vec<Value>, String> {
  let data = object(text)?;
  let roles = roster.work_roles();
  let written = written_stages(&data)?;
  let mut read = vec![];

  for (index, (title, kind, tasks)) in split_stages(written).into_iter().enumerate() {
    let mut planned = vec![];

    for (at, task) in tasks.iter().enumerate() {
      let role = match task.get("role").and_then(Value::as_str) {
        Some(role) if roles.iter().any(|one| one == role) => role,
        _ => return Err(refused(WorkOrderRefusal::PlanRoleWrong { roles })),
      };
      let named = match task.get("model") {
        None | Some(Value::Null) => None,
        given => Some(nonempty(given, PlanPart::Model, MODEL_LIMIT)?),
      };
      let chosen = chosen_model(roster, policy, role, named, model);
      let follows = chosen.is_none();
      let selected = match chosen {
        Some(chosen) => chosen,
        None => roster.role_tier(role)?,
      };

      if !roster.holds(&selected) && !is_selection(&selected) {
        return Err(refused(WorkOrderRefusal::PlanModelForm));
      }

      let instruction = nonempty(task.get("instruction"), PlanPart::TaskInstruction, INSTRUCTION_LIMIT)?;
      let mut one = planned_task(
        &format!("stage-{index}-{at}"),
        role,
        &selected,
        &instruction,
        plan_title(task.get("title")),
      );

      if follows {
        one["followRole"] = Value::Bool(true);
      } else if roster.holds(&selected) {
        one["tier"] = Value::String(selected.clone());
      }

      planned.push(one);
    }

    read.push(serde_json::json!({ "title": title, "kind": kind, "tasks": planned }));
  }

  Ok(read)
}

/// The tasks a synthesis builds, read and checked: within bounds, each one an instruction, written alone or
/// under its title, each a builder task at the builder's tier that follows its role; the validator's words
/// where they do not hold.
pub fn read_build_tasks(roster: &Roster, text: &str) -> Result<Vec<Value>, String> {
  let data = object(text)?;
  let tasks = match data.get("tasks") {
    Some(Value::Array(tasks)) if tasks.len() <= MAX_BUILD_TASKS => tasks,
    _ => {
      return Err(refused(WorkOrderRefusal::PlanTooManyBuilds {
        most: u32::try_from(MAX_BUILD_TASKS).unwrap_or(u32::MAX),
      }));
    }
  };
  let mut read = vec![];

  for (at, task) in tasks.iter().enumerate() {
    let (instruction, title) = if task.is_string() {
      (Some(task), None)
    } else {
      (task.get("instruction"), plan_title(task.get("title")))
    };

    read.push(build_task(roster, &format!("build-{at}"), instruction, title)?);
  }

  Ok(read)
}

// One build task at the builder's tier, following its role.
fn build_task(
  roster: &Roster,
  id: &str,
  instruction: Option<&Value>,
  title: Option<String>,
) -> Result<Value, String> {
  let tier = roster.role_tier(BUILDER)?;
  let mut one = planned_task(
    id,
    BUILDER,
    &tier,
    &nonempty(instruction, PlanPart::BuildInstruction, INSTRUCTION_LIMIT)?,
    title,
  );

  one["followRole"] = Value::Bool(true);

  Ok(one)
}

/// A step added to a running order, read and checked as a synthesis's task is, its title required: a build
/// task under `id` at the builder's tier that follows its role.
pub fn added_step(roster: &Roster, id: &str, title: &str, instruction: &str) -> Result<Value, String> {
  let title = Value::String(title.to_owned());

  nonempty(Some(&title), PlanPart::TaskTitle, TITLE_LIMIT)?;
  build_task(roster, id, Some(&Value::String(instruction.to_owned())), plan_title(Some(&title)))
}
