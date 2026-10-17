//! What one step of a work order is handed (docs/queue.md, "What a step is handed"): a branch its task, the
//! results before it and the rules it runs under; a builder turn the order, where it stands, every earlier
//! result wrapped for the runner to send whole only to a session that has not read it, and the order's own
//! words in English through bmo-gate. A step reads the item through the order's purpose and instruction
//! alone: the item's conversation, the words people append and the approval note are never carried into it.
//! The texts are the policy's work turns.

use std::path::Path;
use std::sync::LazyLock;

use job_policy::Policy;
use regex::Regex;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::pool::branch_task;
use super::{Fault, Ground, faulted};
use crate::job_run::plan_check::Roster;
use crate::one;
use crate::queue::work_orders::{self as orders, Place, places};
use crate::queue::work_turns::structural_fixes;

// The role whose turns are the worker's own session rather than a branch of it.
const CONTROL_ROLE: &str = "builder";

// How much of a finished task's result a later turn is handed inline, and of one an earlier round ran.
const EXCERPT: usize = 6000;
const EARLIER_EXCERPT: usize = 2000;

// How a wrapped piece opens and closes; the runner opens it into its text or a marker before the run.
const BLOCK_OPEN: &str = "WORK-RESULT ";
const BLOCK_CLOSE: &str = "END WORK-RESULT";

fn text<'v>(value: &'v Value, key: &str) -> Option<&'v str> {
  value.get(key).and_then(Value::as_str)
}

fn id(task: &Value) -> &str {
  text(task, "id").unwrap_or_default()
}

fn state(task: &Value) -> &str {
  text(task, "state").unwrap_or_default()
}

fn settled(task: &Value) -> bool {
  matches!(state(task), "done" | "skipped")
}

// A task carried over from an earlier round, renamed under it.
fn carried(task: &Value) -> bool {
  static CARRIED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^r\d+-").expect("the carried pattern builds"));

  CARRIED.is_match(id(task))
}

// The planning turn, of this round or an earlier one.
fn plan_turn(task: &Value) -> bool {
  static PLAN: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(r\d+-)?plan$").expect("the plan pattern builds"));

  PLAN.is_match(id(task))
}

// A text cut to so many UTF-16 units, as the order's own reader counts a string's length.
fn utf16_cut(text: &str, units: usize) -> Option<String> {
  let encoded: Vec<u16> = text.encode_utf16().collect();

  (encoded.len() > units).then(|| String::from_utf16_lossy(&encoded[..units]))
}

/// How much of a finished task's result a later turn is handed inline: less of an earlier round's, whose
/// document still holds the whole of it.
pub fn result_excerpt(task: &Value) -> String {
  let result = text(task, "result").unwrap_or_default();
  let limit = if task.get("round").is_none() { EXCERPT } else { EARLIER_EXCERPT };

  match utf16_cut(result, limit) {
    None => result.to_owned(),
    Some(cut) => format!("{cut}\n… (cut here; the whole result is in its result document)"),
  }
}

/// One earlier result or note, wrapped with the key it is kept under and the version of its text.
pub fn result_block(key: &str, document: Option<&str>, body: &str) -> String {
  let version: String = Sha256::digest(body.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();
  let head = format!(
    "{{\"key\":{},\"version\":{},\"document\":{}}}",
    Value::from(key),
    Value::from(version),
    document.map_or(Value::Null, Value::from)
  );

  format!("{BLOCK_OPEN}{head}\n{body}\n{BLOCK_CLOSE}")
}

/// The branch results of one round joined into a single document: the path it is written to, its text, and
/// the tasks it was joined from.
pub struct Merged {
  pub path: String,
  pub text: String,
  pub ids: Vec<String>,
}

/// The branches of this round's stages that finished and left a document, joined in stage order, each piece
/// under the title its plan gave it; none where the round has no branch result to join.
pub fn merged_results(order: &Value) -> Option<Merged> {
  let tasks: Vec<&Value> = places(order)
    .into_iter()
    .filter(|(place, _)| matches!(place, Place::Stage(..)))
    .map(|(_, task)| task)
    .filter(|task| {
      text(task, "role") != Some(CONTROL_ROLE)
        && state(task) == "done"
        && task.get("result").is_some_and(|result| !result.is_null())
        && task.get("artifact").is_some_and(|artifact| !artifact.is_null())
        && !carried(task)
    })
    .collect();
  let first = text(tasks.first()?, "artifact")?;
  let round = orders::number(order, "round").unwrap_or(1);
  let todo = orders::number(order, "todoId").unwrap_or_default();
  let name = format!("{}branch-results.md", if round > 1 { format!("r{round}-") } else { String::new() });
  let path = Path::new(first).parent().unwrap_or(Path::new("")).join(name);
  let head = format!(
    "# Branch results of TODO #{todo}{}",
    if round > 1 { format!(" · round {round}") } else { String::new() }
  );
  let pieces = tasks
    .iter()
    .map(|task| orders::result_document(task, text(task, "result").unwrap_or_default()));

  Some(Merged {
    path: path.to_string_lossy().into_owned(),
    text: [
      head,
      "Every branch this work order has finished, in stage order, each piece under the title its plan gave that task.".to_owned(),
    ]
    .into_iter()
    .chain(pieces)
    .collect::<Vec<_>>()
    .join("\n\n"),
    ids: tasks.iter().map(|task| id(task).to_owned()).collect(),
  })
}

/// What the synthesis turn is asked for.
pub fn synthesis_order(policy: &Policy) -> String {
  policy.fill(&policy.work_turns.synthesis, &[("structural_fixes", &structural_fixes(policy))])
}

/// The command a builder turn of the item's order adds a step with, run from the tree `tree`.
pub fn add_step(policy: &Policy, tree: &Path, todo: i64) -> String {
  policy.fill(
    &policy.work_turns.add_step,
    &[("tree", &tree.to_string_lossy()), ("todo", &todo.to_string())],
  )
}

/// What the final turn is asked for, `add_step` the command it adds a step with.
pub fn final_order(policy: &Policy, add_step: &str) -> String {
  policy.fill(
    &policy.work_turns.r#final,
    &[("structural_fixes", &structural_fixes(policy)), ("add_step", add_step)],
  )
}

/// Names in an English sentence: "A, B or C".
pub fn spoken(names: &[&str], last: &str) -> String {
  match names.split_last() {
    Some((tail, head)) if !head.is_empty() => format!("{} {last} {tail}", head.join(", ")),
    _ => names.concat(),
  }
}

// What the planning turn is told about picking a model: the ladder, which tiers a borrowed co-worker takes,
// and each role the plan may name with its default tier and its own definition's description.
fn model_standard(policy: &Policy, roster: &Roster) -> String {
  let allowed = roster.work_roles();
  let lent = roster.coworker_tiers();
  let ladder = roster.ladder();
  let own: Vec<&str> = ladder.iter().copied().filter(|tier| !lent.contains(tier)).collect();
  let mut lines = vec![
    "[Model tiers]".to_owned(),
    format!(
      "Roles differ by the size of the judgement each one needs, and the model tier follows that size. Never spend an expensive tier on work a cheap one can do: the wide search and pinning a location down are a cheap role's work first, and builder takes the confirmed files and edits them. The model field names {}. A builder task is a turn of the worker itself and may name any of them; every other role is a co-worker borrowed from the pool and names only {}, since {} {} never a borrowed co-worker's tier. Provider models and effort come from the pool's settings at execution time.",
      spoken(&ladder, "or"),
      lent.join(", "),
      spoken(&own, "and"),
      if own.len() == 1 { "is" } else { "are" }
    ),
    "The roles you may use, and the default that applies when model is left null. Each description is quoted from that role's own definition.".to_owned(),
  ];

  for role in roster.roles.iter().filter(|role| role.enabled && allowed.contains(&role.name)) {
    lines.push(format!("- {} ({}): {}", role.name, role.tier, role.description));
  }

  lines.extend([
    stage_builder_standard(policy),
    "Leaving model null is the normal answer. Never copy the model you are running on onto another role's task, nor the model and effort in a co-worker definition's front matter or a concrete model an earlier plan holds: those are seeds, and this list is the live one.".to_owned(),
    "Naming a model explicitly follows the same standard: do it only where that one task plainly needs a different tier, and say why in its instruction. A non-builder role given the same model as builder is read as no choice at all and falls back to the role's default.".to_owned(),
  ]);

  lines.join("\n")
}

// What the planning turn is told about a builder task it puts in a stage: it comes before the build, so it
// runs at the stage builders' tier rather than the builder's own.
fn stage_builder_standard(policy: &Policy) -> String {
  format!(
    "A builder task you put in a stage runs at {tier} where its model is null, not at the builder's tier above: every stage comes before the build, and a review stage after it checks what it decides with a cheaper role, so this session's own tier is kept for the build tasks. Leave such a task's model null or name {tier}; name a higher tier only where that one decision plainly needs it, and say why in its instruction. On another model it starts from a note of this session rather than the conversation, so its instruction carries what it needs.",
    tier = policy.queue.stage_builder_tier
  )
}

// The item's title, the one thing of the item itself a step reads.
async fn item_title(ground: &Ground<'_>, todo: i64) -> Result<String, Fault> {
  let item = one(ground.db, r#"SELECT "title" FROM "todos" WHERE "id" = $1"#, vec![todo.into()])
    .await
    .map_err(faulted)?;

  Ok(
    item
      .and_then(|row| row.try_get::<Option<String>>("", "title").ok().flatten())
      .unwrap_or_default(),
  )
}

/// The prompt one step of the order is launched on.
pub async fn work_context(
  ground: &Ground<'_>,
  roster: &Roster,
  order: &Value,
  task: &Value,
) -> Result<String, Fault> {
  let policy = ground.policy;
  let todo = orders::number(order, "todoId").unwrap_or_default();
  let tasks: Vec<&Value> = places(order).into_iter().map(|(_, task)| task).collect();
  let done: Vec<&Value> = tasks.iter().copied().filter(|task| state(task) == "done").collect();
  let skipped: Vec<&Value> = tasks.iter().copied().filter(|task| state(task) == "skipped").collect();
  let remaining: Vec<&Value> = tasks.iter().copied().filter(|task| !settled(task)).collect();
  // What was decided against travels beside what was done, never inside it: only its document is pointed at.
  let skipped_block: Vec<String> = if skipped.is_empty() {
    vec![]
  } else {
    let lines: Vec<String> = skipped
      .iter()
      .map(|task| {
        let why = task.get("skip").and_then(|skip| text(skip, "why")).unwrap_or("no reason recorded");
        let document = text(task, "artifact")
          .filter(|artifact| !artifact.is_empty())
          .map(|artifact| format!(" (result document: {artifact})"))
          .unwrap_or_default();

        format!("{}: {why}{document}", id(task))
      })
      .collect();

    vec![format!("[Skipped steps]\n{}", lines.join("\n"))]
  };
  let title = item_title(ground, todo).await?;
  let role = text(task, "role").unwrap_or_default();
  let current = text(task, "instruction").unwrap_or_default();
  let fixes = structural_fixes(policy);

  if branch_task(task) {
    let earlier: Vec<String> = done
      .iter()
      .filter(|task| !plan_turn(task))
      .map(|task| {
        format!("{}: {}\n{}", id(task), text(task, "artifact").unwrap_or_default(), result_excerpt(task))
      })
      .collect();

    return Ok(
      [
        format!("TODO #{todo} · {role} · {}\n{current}", id(task)),
        format!("[Earlier results]\n{}", earlier.join("\n\n")),
      ]
      .into_iter()
      .chain(skipped_block)
      .chain([
        policy.fill(&policy.work_turns.branch_rules, &[("structural_fixes", &fixes)]),
        policy.work_turns.branch_result_shape.clone(),
      ])
      .collect::<Vec<_>>()
      .join("\n\n"),
    );
  }

  // A builder turn continues the worker's session, so each result is wrapped for the runner to send whole
  // only to a session that has not read that version yet; the branches go as the one document they were
  // joined into, and what is not in it each as its own piece beside it.
  let merged = merged_results(order);
  let joined: &[String] = merged.as_ref().map_or(&[], |merged| merged.ids.as_slice());
  let inputs: Vec<String> = merged
    .iter()
    .map(|merged| result_block(&format!("#{todo} branch results"), Some(&merged.path), &merged.text))
    .chain(done.iter().filter(|task| !joined.iter().any(|one| one == id(task))).map(|task| {
      let artifact = text(task, "artifact");

      result_block(
        &format!("#{todo} {}", id(task)),
        artifact,
        &format!(
          "### {}\nResult document: {}\n{}",
          id(task),
          artifact.unwrap_or("none"),
          result_excerpt(task)
        ),
      )
    }))
    .collect();
  let rounds: Vec<&Value> = order
    .get("earlier")
    .and_then(Value::as_array)
    .map(|rounds| rounds.iter().collect())
    .unwrap_or_default();
  let mut asked = vec![
    title,
    orders::text(order, "intent").unwrap_or_default().to_owned(),
    orders::text(order, "instruction").unwrap_or_default().to_owned(),
  ];

  for round in &rounds {
    asked.push(text(round, "intent").unwrap_or_default().to_owned());
    asked.push(text(round, "instruction").unwrap_or_default().to_owned());
  }

  let carried_texts = ground.units.english(&asked).await.map_err(faulted)?;
  let at = |index: usize| carried_texts.get(index).cloned().unwrap_or_default();
  let (title, intent, purpose) = (at(0), at(1), at(2));
  let completed: Vec<&str> = done.iter().map(|task| id(task)).collect();
  // The current task stands whole under its own heading, so what is left after it is named without it.
  let left: Vec<String> = remaining
    .iter()
    .filter(|other| id(other) != id(task))
    .map(|task| format!("{}: {}", id(task), text(task, "instruction").unwrap_or_default()))
    .collect();
  let mut sections = vec![
    format!("[This turn]\nTODO #{todo} \"{title}\" · {}", id(task)),
    format!("[Work order]\nTODO #{todo}: {title}\nPurpose: {intent}\nInstruction: {purpose}"),
    format!(
      "[Completed steps]\n{}",
      if completed.is_empty() { "none".to_owned() } else { completed.join(", ") }
    ),
    format!("[Current task]\n{} · {role}\n{current}", id(task)),
    format!(
      "[Where this task sits]\n{} · stage {}",
      orders::text(order, "phase").unwrap_or_default(),
      orders::number(order, "stage").unwrap_or_default() + 1
    ),
    format!(
      "[Remaining tasks]\n{}",
      if left.is_empty() { "none".to_owned() } else { left.join("\n") }
    ),
    format!(
      "[Earlier results]\n{}",
      if inputs.is_empty() {
        "none".to_owned()
      } else {
        format!("{}\n\n{}", policy.work_turns.settled_results, inputs.join("\n\n"))
      }
    ),
  ];

  sections.extend(skipped_block);

  if !rounds.is_empty() {
    let round = orders::number(order, "round").unwrap_or(1).to_string();
    let quoted = rounds.iter().enumerate().map(|(index, one)| {
      let carried_or = |offset: usize, key: &str| {
        carried_texts
          .get(3 + index * 2 + offset)
          .cloned()
          .unwrap_or_else(|| text(one, key).unwrap_or_default().to_owned())
      };

      format!(
        "--- Round {}\nPurpose: {}\nInstruction: {}",
        one.get("round").map(Value::to_string).unwrap_or_default(),
        carried_or(0, "intent"),
        carried_or(1, "instruction")
      )
    });

    sections.push(
      std::iter::once(policy.fill(&policy.work_turns.earlier_rounds, &[("round", &round)]))
        .chain(quoted)
        .collect::<Vec<_>>()
        .join("\n"),
    );
  }

  if let Some(conflicts) =
    order.get("conflicts").and_then(Value::as_array).filter(|files| !files.is_empty())
  {
    let files: Vec<&str> = conflicts.iter().filter_map(Value::as_str).collect();

    sections.push(policy.fill(&policy.work_turns.merge_conflicts, &[("files", &files.join(", "))]));
  }

  // The plan is the one turn that gives tasks a role and a model, so the standard is the plan's alone.
  if plan_turn(task) {
    sections.push(model_standard(policy, roster));
  }

  sections.push(
    policy.fill(&policy.work_turns.step_rules, &[("add_step", &add_step(policy, &ground.tree, todo))]),
  );

  Ok(sections.join("\n\n"))
}
