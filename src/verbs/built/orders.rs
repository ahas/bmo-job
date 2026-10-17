//! An order's body changed by the console, the command line or a launch (docs/api.md, "Work orders";
//! docs/queue.md, "A work order's verbs"): every change applied inside the order's lock to the body read
//! there, beside the runner waiting on the same lock, and saved. What a change sets off beyond the body —
//! runs stopped, the item held or reopened, its brief written, the agent told how the queue stands —
//! follows from here too.

use std::path::Path;

use job_codes::{JobError, WorkOrderRefusal};
use queue_rows::order_lock::with_order_lock;
use queue_rows::todos::{self, Item};
use queue_rows::{filing, store, work_orders as rows};
use serde_json::{Value, json};

use super::Built;
use crate::job_run::plan_check::{Roster, added_step};
use crate::queue::outcomes::JOB_DIED;
use crate::queue::units::Units;
use crate::queue::work_orders::{self as orders, RecoveredDocument, StoppedRun, Tiers};
use crate::queue::{jobs, work_turns};
use crate::verbs::{Done, OrderAction, WorkOrders};
use crate::{Refusal, now_ms};

/// The most characters an order's intent or instruction holds.
const TEXT_MOST: usize = 30_000;

fn refused(refusal: WorkOrderRefusal) -> Refusal {
  JobError::from(refusal).into()
}

/// Whether a directory is a git checkout, which a co-work stands on.
pub(super) fn is_checkout(path: &str) -> bool {
  Path::new(path).join(".git").exists()
}

/// The tiers bmo-agency lists: the ladder in its order, and those a co-worker may run at.
fn tiers(roster: &Roster) -> Tiers {
  Tiers {
    ladder: roster.tiers.iter().map(|tier| tier.name.clone()).collect(),
    coworker: roster.tiers.iter().filter(|tier| tier.coworker).map(|tier| tier.name.clone()).collect(),
  }
}

/// What one change came to beside the body: whether it freed the worker the order held, and whether the
/// instruction it leaves is the item's brief.
#[derive(Default)]
struct Changed {
  freed: bool,
  briefs: bool,
}

impl Built {
  /// One change applied to one order inside its lock, answering the order as it now stands.
  pub(super) async fn apply(&self, id: i64, action: OrderAction) -> Result<Value, Refusal> {
    let db = &self.db;
    let (order, changed) = with_order_lock(db, id, async {
      let item = todos::item(db, id).await?.ok_or_else(|| refused(WorkOrderRefusal::ItemAbsent))?;
      let stored = rows::read(db, id).await?;
      let mut order = stored
        .clone()
        .unwrap_or_else(|| queue_read::work_order::draft(id, &item.title, &item.instruction));
      let changed = self.change(&item, &mut order, stored.is_some(), action).await?;

      rows::save(db, &mut order).await?;

      if orders::changed(stored.as_ref(), &order) {
        self.events.order_changed(id, &order);
      }

      // The instruction a launch starts the order on, and the one a reopen is handed, is the item's brief.
      if changed.briefs
        && let Some(said) = orders::text(&order, "instruction")
      {
        todos::set_brief(db, id, said).await?;
      }

      Ok::<_, Refusal>((order, changed))
    })
    .await?;

    // The worker the closed order held is free, and the queue is told outside the lock.
    if changed.freed {
      self.tell_standing("work-order", false).await?;
    }

    Ok(order)
  }

  // One action on the body read inside the lock.
  async fn change(
    &self,
    item: &Item,
    order: &mut Value,
    stored: bool,
    action: OrderAction,
  ) -> Result<Changed, Refusal> {
    let db = &self.db;
    let units = self.units();
    let now = now_ms();

    match action {
      OrderAction::Resume => {
        if order.get("closing").is_some() {
          return Err(refused(WorkOrderRefusal::OrderClosing));
        }

        if orders::text(order, "status") != Some("paused")
          || !item.approved
          || item.held_at.is_some()
          || item.merged()
        {
          return Err(refused(WorkOrderRefusal::ResumeNotHeld));
        }

        if item.done {
          store::reopen(db, item.id).await?;
        }

        orders::resume(order, now)?;
      }
      OrderAction::Save {
        intent,
        instruction,
        model,
      } => self.rewrite(order, item, [intent, instruction, model], false).await?,
      OrderAction::Reopen {
        intent,
        instruction,
        model,
      } => {
        let briefs = instruction.is_some();

        self.rewrite(order, item, [intent, instruction, model], true).await?;

        return Ok(Changed {
          freed: false,
          briefs,
        });
      }
      OrderAction::Edit {
        task,
        instruction,
        model,
      } => {
        let request = orders::pool_request_of(order, &task);
        let tiers = tiers(&Roster::read(&units).await?);

        orders::edit(order, &tiers, &task, instruction.as_deref(), model.as_deref(), now)?;
        give_back(&units, request, "task edited before launch").await?;
      }
      OrderAction::Rerun {
        task,
        instruction,
        model,
      } => {
        let request = orders::pool_request_of(order, &task);
        let tiers = tiers(&Roster::read(&units).await?);

        orders::rerun(order, &tiers, &task, instruction.as_deref(), model.as_deref(), now)?;
        give_back(&units, request, "task rerun").await?;
      }
      OrderAction::Skip { task, why } => {
        if let Some(stopped) = orders::skip(order, &task, &why, now)? {
          self.stop_runs(&units, &[stopped], &why).await?;
        }
      }
      OrderAction::Restore { task } => orders::restore(order, &task, now)?,
      OrderAction::Move {
        task,
        anchor,
        after,
      } => orders::shift(order, &task, &anchor, after, now)?,
      OrderAction::Add {
        title,
        instruction,
        anchor,
      } => {
        let roster = Roster::read(&units).await?;
        let step = added_step(&roster, "", &title, &instruction)
          .map_err(|_| refused(WorkOrderRefusal::InstructionSize))?;
        let anchor = anchor.as_ref().map(|(task, after)| (task.as_str(), *after));

        orders::add(order, step, anchor, now)?;
      }
      OrderAction::Checkout { cwd } => {
        // The directory the order runs in, and nothing else about it: a checkout can move on the machine
        // while orders still hold it.
        if orders::text(order, "status") == Some("done") {
          return Err(refused(WorkOrderRefusal::CheckoutOfDone));
        }

        if !is_checkout(&cwd) {
          return Err(refused(WorkOrderRefusal::CheckoutNotGit { cwd }));
        }

        order["cwd"] = json!(cwd);
      }
      OrderAction::Recover { from, to } => {
        if from > to {
          return Err(refused(WorkOrderRefusal::JobRangeForm));
        }

        let records = jobs::item_records(db, item.id, from, to).await?;

        write_recovered(&orders::recover(order, &records, &self.ground.documents, now)?)?;
      }
      OrderAction::Close { why } => {
        let stopped = orders::close(order, &why, now)?;

        self.stop_runs(&units, &stopped, &why).await?;
        // The item is not finished, so it steps out of the queue with the reason: an approved item whose
        // order is done would stand in the queue as open and be refused by the launcher every time.
        todos::hold(db, item.id, Some(&why), now).await?;

        return Ok(Changed {
          freed: true,
          briefs: false,
        });
      }
      OrderAction::Start {
        chat,
        cwd,
        worker,
        instruction,
        model,
      } => {
        self.start(order, item, stored, [instruction, model], (chat, cwd, worker)).await?;

        return Ok(Changed {
          freed: false,
          briefs: true,
        });
      }
    }

    Ok(Changed::default())
  }

  // A draft's text saved, or a finished order reopened with the text it is asked to carry. A field the
  // request leaves out keeps what the order already says.
  async fn rewrite(
    &self,
    order: &mut Value,
    item: &Item,
    [intent, instruction, model]: [Option<String>; 3],
    reopen: bool,
  ) -> Result<(), Refusal> {
    let status = orders::text(order, "status");

    if reopen && status != Some("done") {
      return Err(refused(WorkOrderRefusal::ReopenNotDone));
    }

    if !reopen && status != Some("draft") {
      return Err(refused(WorkOrderRefusal::DraftOnly));
    }

    let kept = |key: &str, given: Option<String>| {
      given.or_else(|| orders::text(order, key).map(str::to_owned)).unwrap_or_default()
    };
    let (intent, instruction) = (kept("intent", intent), kept("instruction", instruction));
    let fits = |text: &str| !text.trim().is_empty() && text.chars().count() <= TEXT_MOST;
    let model_fits = match model.as_deref() {
      None | Some("") => true,
      Some(model) => filing::is_job_model(model, &self.provider_names().await?),
    };

    if !fits(&intent) || !fits(&instruction) || !model_fits {
      return Err(refused(WorkOrderRefusal::OrderAndModel));
    }

    if reopen {
      orders::reopen(order, now_ms())?;
    }

    order["intent"] = json!(intent.trim());
    order["instruction"] = json!(instruction.trim());

    if let Some(model) = model {
      order["model"] = json!(model);
    }

    if reopen && item.done {
      store::reopen(&self.db, item.id).await?;
    }

    Ok(())
  }

  // A launch that runs the item as a co-work puts its order under way here, beside the runner, so the body it
  // starts is the one the runner reads next.
  async fn start(
    &self,
    order: &mut Value,
    item: &Item,
    stored: bool,
    [instruction, model]: [Option<String>; 2],
    (chat, cwd, worker): (i64, String, String),
  ) -> Result<(), Refusal> {
    let id = item.id;

    if !item.approved || item.done || item.held_at.is_some() || item.merged() {
      return Err(refused(WorkOrderRefusal::ItemNotApproved));
    }

    match orders::text(order, "status") {
      Some("paused") => return Err(refused(WorkOrderRefusal::OrderPaused { item: id })),
      Some("active") => return Err(refused(WorkOrderRefusal::OrderActive { item: id })),
      Some("done") => return Err(refused(WorkOrderRefusal::OrderDone { item: id })),
      _ => {}
    }

    // The prompt the order is launched with is its instruction; an order already written may be started on
    // the instruction it carries.
    match instruction.filter(|said| !said.trim().is_empty()) {
      Some(said) => order["instruction"] = json!(said.trim()),
      None if !stored => {
        return Err(refused(WorkOrderRefusal::FieldMissing {
          name: "instruction".to_owned(),
        }));
      }
      None => {}
    }

    let stored_model = orders::text(order, "model").filter(|model| !model.is_empty()).map(str::to_owned);
    let model = model
      .or(stored_model)
      .or_else(|| item.model.clone())
      .unwrap_or_else(|| "auto".to_owned());

    if !filing::is_job_model(&model, &self.provider_names().await?) {
      return Err(refused(WorkOrderRefusal::OrderAndModel));
    }

    if chat == 0 {
      return Err(refused(WorkOrderRefusal::ChatWrong));
    }

    if !is_checkout(&cwd) {
      return Err(refused(WorkOrderRefusal::CheckoutNotGit { cwd }));
    }

    let units = self.units();
    let roles: Vec<String> = units
      .pool_roles()
      .await?
      .into_iter()
      .filter(|role| role.enabled && role.pooled)
      .map(|role| role.name)
      .collect();
    let plan = work_turns::plan_order(&self.policy, &roles);

    order["model"] = json!(model);
    order["worker"] = json!(worker);
    order["cwd"] = json!(cwd);
    order["chatId"] = json!(chat);
    order["status"] = json!("active");
    order["control"] = orders::task("plan", "builder", &model, &plan);

    Ok(())
  }

  // The runs a change to an order ended, confirmed gone before the rows they left close as died; a unit still
  // standing refuses the change.
  async fn stop_runs(&self, units: &dyn Units, runs: &[StoppedRun], why: &str) -> Result<(), Refusal> {
    self.stops(units).stop_runs(runs, why, JOB_DIED, now_ms()).await
  }

  /// The names of the providers bmo-agency lists.
  pub(super) async fn provider_names(&self) -> Result<Vec<String>, Refusal> {
    Ok(self.providers().await?.into_iter().map(|provider| provider.name).collect())
  }
}

// A place still waited for given back, where the task held one.
async fn give_back(units: &dyn Units, request: Option<String>, why: &str) -> Result<(), Refusal> {
  if let Some(request) = request {
    units.finish_request(&request, why, true).await?;
  }

  Ok(())
}

// The documents a recovered round points at. Every one is checked before any is written, so a refusal leaves
// nothing behind, and a document already standing is never written over — one holding exactly the same text
// is the same document and is simply kept.
fn write_recovered(documents: &[RecoveredDocument]) -> Result<(), Refusal> {
  for document in documents {
    if let Ok(standing) = std::fs::read_to_string(&document.path)
      && standing != document.text
    {
      return Err(refused(WorkOrderRefusal::DocumentExists {
        path: document.path.display().to_string(),
      }));
    }
  }

  for document in documents.iter().filter(|document| !document.path.exists()) {
    write_private(&document.path, &document.text).map_err(|why| {
      Refusal::from(sea_orm::DbErr::Custom(format!("{}: {why}", document.path.display())))
    })?;
  }

  Ok(())
}

// A document only the owner reads, written whole beside itself first so no reader meets half of it.
fn write_private(path: &Path, text: &str) -> std::io::Result<()> {
  use std::io::Write;
  use std::os::unix::fs::OpenOptionsExt;

  if let Some(parent) = path.parent() {
    std::fs::create_dir_all(parent)?;
  }

  let beside = path.with_extension("tmp");

  std::fs::OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .mode(0o600)
    .open(&beside)?
    .write_all(text.as_bytes())?;
  std::fs::rename(&beside, path)
}

impl WorkOrders for Built {
  async fn change_order(&self, id: i64, action: OrderAction) -> Done<Value> {
    self.apply(id, action).await
  }
}
