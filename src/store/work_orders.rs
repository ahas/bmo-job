//! Work orders and the tasks their steps make (docs/api.md, "Work orders and tasks").

use std::collections::HashSet;

use bmo_job_client::{
  Code, EventKind, Item, ItemState, NewStep, Page, Task, TaskAfter, TaskChange, TaskQuery, TaskState,
  WorkOrder, WorkOrderBody, WorkOrderChange, WorkOrderState,
};
use sqlx::Row;
use sqlx::sqlite::SqliteRow;

use super::events::{EventKeys, emit, key_alone};
use super::items::{find_item, item_at, item_keys};
use super::{Store, Tx, filled, from_json, page_limit, paged, stored, to_json};
use crate::refusal::Refusal;

const TASK_COLUMNS: &str =
  "id, item_id, stage, title, state, assignee_name, after_item_id, after_repository, changed_at";

fn work_order_from(row: &SqliteRow) -> Result<WorkOrder, Refusal> {
  Ok(WorkOrder {
    item_id: row.try_get("item_id")?,
    purpose: row.try_get("purpose")?,
    directive: row.try_get("directive")?,
    steps: from_json(row.try_get::<&str, _>("steps")?)?,
    state: stored(row.try_get("state")?, WorkOrderState::parse)?,
    changed_at: row.try_get("changed_at")?,
  })
}

fn task_from(row: &SqliteRow) -> Result<Task, Refusal> {
  Ok(Task {
    id: row.try_get("id")?,
    item_id: row.try_get("item_id")?,
    stage: row.try_get("stage")?,
    title: row.try_get("title")?,
    state: stored(row.try_get("state")?, TaskState::parse)?,
    assignee_name: row.try_get("assignee_name")?,
    after: after_from(row)?,
    changed_at: row.try_get("changed_at")?,
  })
}

/// The work a task's step waits on, kept as its item and repository, both null where it waits on none.
fn after_from(row: &SqliteRow) -> Result<Option<TaskAfter>, Refusal> {
  let item_id: Option<i64> = row.try_get("after_item_id")?;
  let repository: Option<String> = row.try_get("after_repository")?;
  Ok(item_id.zip(repository).map(|(item_id, repository)| TaskAfter {
    item_id,
    repository,
  }))
}

/// The work a task of `item_id` is given to wait on: a repository named, and another item that stands
/// (docs/api.md, "Work orders and tasks").
async fn after_read(tx: &mut Tx, item_id: i64, after: &TaskAfter) -> Result<(), Refusal> {
  filled("repository", &after.repository)?;
  if after.item_id == item_id {
    return Err(Refusal::conflict(format!(
      "A step of item {item_id} cannot wait on item {item_id} itself."
    )));
  }
  if find_item(tx, after.item_id).await?.is_none() {
    return Err(Refusal::new(
      Code::UnknownItem,
      format!("There is no item {} for the step to wait on.", after.item_id),
    ));
  }
  Ok(())
}

async fn find_work_order(tx: &mut Tx, item_id: i64) -> Result<Option<WorkOrder>, Refusal> {
  let row = sqlx::query(
    "SELECT item_id, purpose, directive, steps, state, changed_at FROM work_orders WHERE item_id = $1",
  )
  .bind(item_id)
  .fetch_optional(&mut **tx)
  .await?;
  row.as_ref().map(work_order_from).transpose()
}

/// The item and its work order, or a refusal saying which is missing.
async fn work_order_at(tx: &mut Tx, item_id: i64) -> Result<(Item, WorkOrder), Refusal> {
  let item = item_at(tx, item_id).await?;
  let order = find_work_order(tx, item_id)
    .await?
    .ok_or_else(|| Refusal::not_found(format!("The item {item_id} has no work order.")))?;
  Ok((item, order))
}

async fn tasks_of(tx: &mut Tx, item_id: i64) -> Result<Vec<Task>, Refusal> {
  let rows =
    sqlx::query(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE item_id = $1 ORDER BY stage, id"))
      .bind(item_id)
      .fetch_all(&mut **tx)
      .await?;
  rows.iter().map(task_from).collect()
}

async fn task_at(tx: &mut Tx, item_id: i64, task_id: i64) -> Result<Task, Refusal> {
  let row = sqlx::query(&format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = $1 AND item_id = $2"))
    .bind(task_id)
    .bind(item_id)
    .fetch_optional(&mut **tx)
    .await?
    .ok_or_else(|| Refusal::not_found(format!("The item {item_id} has no task {task_id}.")))?;
  task_from(&row)
}

/// Steps as a work order takes them: none empty, and a title taken once at its first place, since a step's
/// title names its task.
fn steps_read(steps: Vec<String>) -> Result<Vec<String>, Refusal> {
  let mut seen = HashSet::new();
  let mut taken = Vec::with_capacity(steps.len());
  for step in steps {
    filled("step", &step)?;
    if seen.insert(step.clone()) {
      taken.push(step);
    }
  }
  Ok(taken)
}

/// Makes the tasks match the steps: a task whose title stands among them keeps its key, state and assignee
/// and takes the step's stage, a new step makes a waiting task, and a task whose title is gone goes.
async fn rebuild_tasks(tx: &mut Tx, item: &Item, steps: &[String], now: i64) -> Result<(), Refusal> {
  // The steps are the plan its job runs in, so they stand fixed while a job holds the item (docs/api.md, "Work
  // orders and tasks"): the same steps written again change nothing.
  if item.state == ItemState::Leased {
    let standing: Vec<String> = tasks_of(tx, item.id).await?.into_iter().map(|task| task.title).collect();

    if standing != steps {
      return Err(Refusal::conflict(format!(
        "Item {}'s steps stand fixed while it is leased: they are the plan its job runs in.",
        item.id
      )));
    }

    return Ok(());
  }

  let keys = item_keys(item);
  let standing = tasks_of(tx, item.id).await?;
  for (place, title) in steps.iter().enumerate() {
    let stage = place as i64 + 1;
    match standing.iter().find(|task| &task.title == title) {
      Some(task) if task.stage == stage => {}
      Some(task) => {
        sqlx::query("UPDATE tasks SET stage = $1, changed_at = $2 WHERE id = $3")
          .bind(stage)
          .bind(now)
          .bind(task.id)
          .execute(&mut **tx)
          .await?;
        let moved = Task {
          stage,
          changed_at: now,
          ..task.clone()
        };
        emit(tx, now, EventKind::TaskChanged.as_str(), &moved, &keys).await?;
      }
      None => {
        make_task(tx, item.id, stage, title, None, now, &keys).await?;
      }
    }
  }
  for task in standing.iter().filter(|task| !steps.contains(&task.title)) {
    sqlx::query("DELETE FROM tasks WHERE id = $1").bind(task.id).execute(&mut **tx).await?;
    emit(tx, now, EventKind::TaskChanged.as_str(), &key_alone(task.id, Some(item.id)), &keys).await?;
  }
  Ok(())
}

/// Makes a waiting task of a step and tells it.
async fn make_task(
  tx: &mut Tx,
  item_id: i64,
  stage: i64,
  title: &str,
  after: Option<TaskAfter>,
  now: i64,
  keys: &EventKeys,
) -> Result<Task, Refusal> {
  let id: i64 = sqlx::query_scalar(
    "INSERT INTO tasks (item_id, stage, title, state, after_item_id, after_repository, changed_at) \
     VALUES ($1, $2, $3, 'waiting', $4, $5, $6) RETURNING id",
  )
  .bind(item_id)
  .bind(stage)
  .bind(title)
  .bind(after.as_ref().map(|after| after.item_id))
  .bind(after.as_ref().map(|after| after.repository.as_str()))
  .bind(now)
  .fetch_one(&mut **tx)
  .await?;
  let made = Task {
    id,
    item_id,
    stage,
    title: title.to_owned(),
    state: TaskState::Waiting,
    assignee_name: None,
    after,
    changed_at: now,
  };
  emit(tx, now, EventKind::TaskChanged.as_str(), &made, keys).await?;
  Ok(made)
}

async fn save_work_order(tx: &mut Tx, order: &WorkOrder) -> Result<(), Refusal> {
  sqlx::query(
    "INSERT INTO work_orders (item_id, purpose, directive, steps, state, changed_at) VALUES ($1, $2, $3, $4, $5, $6) \
     ON CONFLICT (item_id) DO UPDATE SET purpose = excluded.purpose, directive = excluded.directive, \
     steps = excluded.steps, state = excluded.state, changed_at = excluded.changed_at",
  )
  .bind(order.item_id)
  .bind(&order.purpose)
  .bind(&order.directive)
  .bind(to_json(&order.steps))
  .bind(order.state.as_str())
  .bind(order.changed_at)
  .execute(&mut **tx)
  .await?;
  Ok(())
}

impl Store {
  pub async fn work_order(&self, item_id: i64) -> Result<WorkOrder, Refusal> {
    let mut tx = self.begin().await?;
    let (_, order) = work_order_at(&mut tx, item_id).await?;
    tx.commit().await?;
    Ok(order)
  }

  /// Saves the item's work order whole; answers it and whether it is new.
  pub async fn put_work_order(
    &self,
    item_id: i64,
    body: WorkOrderBody,
  ) -> Result<(WorkOrder, bool), Refusal> {
    let WorkOrderBody {
      purpose,
      directive,
      steps,
      state: asked,
    } = body;
    let steps = steps_read(steps)?;
    let now = self.now();
    let mut tx = self.begin().await?;
    let item = item_at(&mut tx, item_id).await?;
    let standing = find_work_order(&mut tx, item_id).await?;
    let made = standing.is_none();
    let state = asked.or(standing.map(|order| order.state)).unwrap_or(WorkOrderState::Draft);
    let order = WorkOrder {
      item_id,
      purpose,
      directive,
      steps,
      state,
      changed_at: now,
    };
    save_work_order(&mut tx, &order).await?;
    rebuild_tasks(&mut tx, &item, &order.steps, now).await?;
    emit(&mut tx, now, EventKind::WorkOrderChanged.as_str(), &order, &item_keys(&item)).await?;
    self.commit(tx).await?;
    Ok((order, made))
  }

  /// Changes the item's work order, its state by the moves allowed.
  pub async fn change_work_order(
    &self,
    item_id: i64,
    change: WorkOrderChange,
  ) -> Result<WorkOrder, Refusal> {
    let steps = change.steps.map(steps_read).transpose()?;
    let now = self.now();
    let mut tx = self.begin().await?;
    let (item, mut order) = work_order_at(&mut tx, item_id).await?;
    if let Some(state) = change.state
      && state != order.state
    {
      if !order.state.may_move_to(state) {
        return Err(Refusal::conflict(format!("A work order {} cannot move to {}.", order.state, state)));
      }
      order.state = state;
    }
    if let Some(purpose) = change.purpose {
      order.purpose = purpose;
    }
    if let Some(directive) = change.directive {
      order.directive = directive;
    }
    if let Some(steps) = steps {
      order.steps = steps;
      rebuild_tasks(&mut tx, &item, &order.steps, now).await?;
    }
    order.changed_at = now;
    save_work_order(&mut tx, &order).await?;
    emit(&mut tx, now, EventKind::WorkOrderChanged.as_str(), &order, &item_keys(&item)).await?;
    self.commit(tx).await?;
    Ok(order)
  }

  /// Adds one step behind the item's plan, whether or not the item is leased, and answers its task. Where the item
  /// has no work order, one is made of its title, its brief and the standing work as the first step.
  pub async fn add_step(&self, item_id: i64, step: NewStep) -> Result<Task, Refusal> {
    let NewStep {
      title,
      after,
      standing,
    } = step;
    filled("step", &title)?;
    let now = self.now();
    let mut tx = self.begin().await?;
    let item = item_at(&mut tx, item_id).await?;
    let keys = item_keys(&item);
    let (mut order, made) = match find_work_order(&mut tx, item_id).await? {
      Some(order) => (order, false),
      None => {
        let standing = standing.ok_or_else(|| {
          Refusal::new(
            Code::StandingRequired,
            format!("Item {item_id} has no work order; name the work already under way as standing."),
          )
        })?;
        filled("standing", &standing)?;
        let order = WorkOrder {
          item_id,
          purpose: item.title.clone(),
          directive: item.brief.clone(),
          steps: vec![standing],
          state: WorkOrderState::Draft,
          changed_at: now,
        };
        (order, true)
      }
    };
    if order.state == WorkOrderState::Done {
      return Err(Refusal::conflict("The work order is done; reopen it to add a step."));
    }
    if order.steps.contains(&title) {
      return Err(Refusal::conflict(format!("The step {title:?} stands among the steps already.")));
    }
    if let Some(after) = &after {
      after_read(&mut tx, item_id, after).await?;
    }
    if made {
      make_task(&mut tx, item_id, 1, &order.steps[0], None, now, &keys).await?;
    }
    let stage = order.steps.len() as i64 + 1;
    let task = make_task(&mut tx, item_id, stage, &title, after, now, &keys).await?;
    order.steps.push(title);
    order.changed_at = now;
    save_work_order(&mut tx, &order).await?;
    emit(&mut tx, now, EventKind::WorkOrderChanged.as_str(), &order, &keys).await?;
    self.commit(tx).await?;
    Ok(task)
  }

  /// The work order's tasks by stage and key, or only the stage under way.
  pub async fn tasks(&self, item_id: i64, query: &TaskQuery) -> Result<Page<Task>, Refusal> {
    let limit = page_limit(query.limit)?;
    let mut tx = self.begin().await?;
    work_order_at(&mut tx, item_id).await?;
    let mut tasks = tasks_of(&mut tx, item_id).await?;
    tx.commit().await?;
    if query.current == Some(true) {
      let under_way = tasks
        .iter()
        .filter(|task| matches!(task.state, TaskState::Waiting | TaskState::Current))
        .map(|task| task.stage)
        .min();
      tasks.retain(|task| Some(task.stage) == under_way);
    }
    if let Some(after) = query.after {
      let Some(place) = tasks.iter().position(|task| task.id == after) else {
        return Err(Refusal::bad_request(format!("The cursor names no task of this list: {after}.")));
      };
      tasks.drain(..=place);
    }
    tasks.truncate(limit as usize + 1);
    Ok(paged(tasks, limit))
  }

  pub async fn task(&self, item_id: i64, task_id: i64) -> Result<Task, Refusal> {
    let mut tx = self.begin().await?;
    let task = task_at(&mut tx, item_id, task_id).await?;
    tx.commit().await?;
    Ok(task)
  }

  /// Moves a task, unless its work order is done.
  pub async fn change_task(&self, item_id: i64, task_id: i64, change: TaskChange) -> Result<Task, Refusal> {
    if change.stage.is_some_and(|stage| stage < 1) {
      return Err(Refusal::bad_request("A stage is 1 or more."));
    }
    let now = self.now();
    let mut tx = self.begin().await?;
    let (item, order) = work_order_at(&mut tx, item_id).await?;
    let mut task = task_at(&mut tx, item_id, task_id).await?;
    if order.state == WorkOrderState::Done {
      return Err(Refusal::conflict("The work order is done; reopen it to move its tasks."));
    }
    if let Some(state) = change.state {
      task.state = state;
    }
    if let Some(stage) = change.stage {
      task.stage = stage;
    }
    if let Some(assignee) = change.assignee_name {
      task.assignee_name = assignee;
    }
    if let Some(after) = change.after {
      if let Some(given) = &after {
        after_read(&mut tx, item_id, given).await?;
      }
      task.after = after;
    }
    task.changed_at = now;
    sqlx::query(
      "UPDATE tasks SET state = $1, stage = $2, assignee_name = $3, after_item_id = $4, after_repository = $5, \
       changed_at = $6 WHERE id = $7",
    )
    .bind(task.state.as_str())
    .bind(task.stage)
    .bind(&task.assignee_name)
    .bind(task.after.as_ref().map(|after| after.item_id))
    .bind(task.after.as_ref().map(|after| after.repository.as_str()))
    .bind(now)
    .bind(task.id)
    .execute(&mut *tx)
    .await?;
    emit(&mut tx, now, EventKind::TaskChanged.as_str(), &task, &item_keys(&item)).await?;
    self.commit(tx).await?;
    Ok(task)
  }
}
