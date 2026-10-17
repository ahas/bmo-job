//! Workers added, changed, removed and placed on nodes (docs/api.md, "Workers"; docs/queue.md, "Workers").
//! The rows are judged against the policy's worker rules before anything is written; a worker's pin and
//! keepalive and its bench are bmo-agency's, asked once the rows are written.

use std::path::PathBuf;

use job_agency::{Agency, Node, PinChoice};
use job_codes::{AgentRefusal, JobError};
use queue_read::lines::BUILTIN_WORKER;

use super::Built;
use crate::queue::paths::resolve;
use crate::queue::pins::pin_choice;
use crate::queue::units::agency_failed;
use crate::queue::worker_rules::{Adding, Beyond, Judge, remove_fault};
use crate::queue::workers::{
  self, Changes, NewWorker, NodeCheckout as CheckoutRow, Worker, node_checkout_label,
  remove_support_worker, settle_support, taken_names, worker_row,
};
use crate::verbs::{Done, NodeChange, NodeCheckout, WorkerAdd, WorkerAdded, WorkerUpdate, Workers};
use crate::{Refusal, now_ms};

/// The kind of node a job's checkout stands on: one SSH reaches.
const SSH_NODE: &str = "ssh";

/// The service a node's role names where it carries the jobs.
const JOBS_SERVICE: &str = "jobs";

fn refused(refusal: AgentRefusal) -> Refusal {
  JobError::from(refusal).into()
}

fn no_worker(name: &str) -> Refusal {
  refused(AgentRefusal::NoWorker {
    worker: name.to_owned(),
  })
}

/// Why a node takes no job, or none where it takes them: one SSH reaches, reserved for no service other
/// than the jobs.
fn node_refusal(node: &Node) -> Option<Refusal> {
  let why = if node.kind != SSH_NODE {
    Some(format!("it is reached as {}, with no SSH route to carry a job", node.kind))
  } else {
    node
      .role
      .as_deref()
      .filter(|role| !role.split('+').any(|service| service == JOBS_SERVICE))
      .map(|role| format!("it is reserved for {role}"))
  };

  why.map(|why| {
    refused(AgentRefusal::NodeTakesNoJobs {
      node: node.name.clone(),
      why,
    })
  })
}

/// Whether a path is one written on a node: from its root or from its home.
fn node_written(path: &str) -> bool {
  path.starts_with('/') || path.starts_with("~/")
}

/// Where a base's checkout on a node stands unless a path is given: the base's own folder name followed by
/// the node's, beside it.
fn node_checkout_path(base_cwd: &str, node: &str) -> String {
  let base = resolve(base_cwd);
  let folder = base.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();

  base
    .parent()
    .map_or_else(PathBuf::new, PathBuf::from)
    .join(format!("{folder}-{node}"))
    .display()
    .to_string()
}

impl Built {
  fn judge(&self) -> Result<Judge<'_>, Refusal> {
    Judge::new(&self.policy.workers)
      .map_err(|why| Refusal::from(sea_orm::DbErr::Custom(format!("the worker name pattern: {why}"))))
  }

  async fn held(&self, name: &str) -> Result<Option<Worker>, Refusal> {
    Ok(
      workers::workers(&self.db, &self.ground.source)
        .await?
        .into_iter()
        .find(|worker| worker.name == name),
    )
  }

  // The workers with something at work, or none where what is running could not be read.
  async fn busy_workers(&self) -> Result<Option<Vec<String>>, Refusal> {
    let units = self.units();

    self.ground.running(&self.db, &units).workers().await
  }

  // A worker's bench made ready by bmo-agency, its dev pane brought up where it has a dev command. A bench
  // that was not made is said and fails nothing: the row is written.
  async fn bench(&self, name: &str) {
    if let Err(failed) = self.ground.agency.make_bench(name).await {
      eprintln!("bmo-job: the bench of {name} was not made ready: {failed}");
    }
  }

  async fn node(&self, name: &str) -> Result<Node, Refusal> {
    self
      .ground
      .agency
      .nodes()
      .await
      .map_err(agency_failed)?
      .into_iter()
      .find(|node| node.name == name)
      .ok_or_else(|| {
        refused(AgentRefusal::NoNode {
          node: name.to_owned(),
        })
      })
  }

  // A base's checkout on a node, added as a support worker placed there.
  async fn add_checkout(
    &self,
    base: &str,
    node: &str,
    path: Option<String>,
  ) -> Result<NodeCheckout, Refusal> {
    let row = worker_row(&self.db, base, &self.ground.source).await?;
    let Some(row) = row.filter(|row| !row.is_support()) else {
      return Err(refused(AgentRefusal::NotABase {
        base: base.to_owned(),
      }));
    };

    if let Some(on) = &row.node {
      return Err(refused(AgentRefusal::BaseOnNode {
        base: base.to_owned(),
        node: on.clone(),
      }));
    }

    let target = self.node(node).await?;

    if let Some(refusal) = node_refusal(&target) {
      return Err(refusal);
    }

    let held = workers::workers(&self.db, &self.ground.source).await?;

    if held
      .iter()
      .any(|worker| worker.support_of.as_deref() == Some(base) && worker.node.as_deref() == Some(node))
    {
      return Err(refused(AgentRefusal::CheckoutAlreadyOnNode {
        base: base.to_owned(),
        node: node.to_owned(),
      }));
    }

    let name = format!("{base}-{node}");
    let judge = self.judge()?;

    if judge.name_fault(&name, &[]).is_err() {
      return Err(refused(AgentRefusal::CheckoutNameLong {
        base: base.to_owned(),
        name,
      }));
    }

    if judge.name_fault(&name, &taken_names(&self.db).await?).is_err() {
      return Err(refused(AgentRefusal::AgentNameTaken { name }));
    }

    let cwd = path.unwrap_or_else(|| node_checkout_path(&row.cwd, node));

    if !node_written(&cwd) {
      return Err(refused(AgentRefusal::NodePathForm));
    }

    let checkout = CheckoutRow {
      name: &name,
      label: &node_checkout_label(&row.label, &target.label),
      cwd: &cwd,
      base,
      node,
    };

    workers::add_node_checkout(&self.db, &checkout, now_ms()).await?;

    Ok(NodeCheckout { worker: name, cwd })
  }

  // A base's checkout on a node taken away, refused while a job holds it; the checkout on the node stays.
  async fn remove_checkout(&self, base: &str, node: &str) -> Result<NodeCheckout, Refusal> {
    let held = workers::workers(&self.db, &self.ground.source).await?;
    let Some(worker) = held.into_iter().find(|worker| {
      worker.support_of.as_deref() == Some(base)
        && worker.node.as_deref() == Some(node)
        && worker.support_slot.is_none()
    }) else {
      return Err(refused(AgentRefusal::CheckoutNotOnNode {
        base: base.to_owned(),
        node: node.to_owned(),
      }));
    };

    // Where what is running cannot be read, the checkout is taken for busy.
    if self.busy_workers().await?.is_none_or(|busy| busy.contains(&worker.name)) {
      return Err(refused(AgentRefusal::CheckoutBusy {
        worker: worker.name,
      }));
    }

    if !remove_support_worker(&self.db, &worker.name).await? {
      return Err(refused(AgentRefusal::CheckoutNotRemoved {
        worker: worker.name,
      }));
    }

    Ok(NodeCheckout {
      worker: worker.name,
      cwd: worker.cwd,
    })
  }

  // A worker of its own stood on a node at a path written there, or brought back to this machine at a path
  // here.
  async fn place(&self, name: &str, node: Option<String>, path: Option<String>) -> Result<(), Refusal> {
    let row = worker_row(&self.db, name, &self.ground.source).await?.ok_or_else(|| no_worker(name))?;
    let mut changes = Changes {
      node: Some(node.clone()),
      ..Changes::default()
    };

    match (node, path) {
      (None, None) => return Err(refused(AgentRefusal::BackHereNeedsPath)),
      (None, Some(path)) => {
        changes.cwd = Some(resolve(&path).display().to_string());
        self.judge()?.change_fault(&row, &changes, Beyond::default())?;
      }
      (Some(node), path) => {
        if row.builtin {
          return Err(refused(AgentRefusal::BuiltinStaysHere));
        }

        if let Some(base) = &row.support_of {
          return Err(refused(AgentRefusal::SupportFollowsBase {
            worker: name.to_owned(),
            base: base.clone(),
          }));
        }

        let held = workers::workers(&self.db, &self.ground.source).await?;

        if row.support_count > 0 || held.iter().any(|worker| worker.support_of.as_deref() == Some(name)) {
          return Err(refused(AgentRefusal::HasSupport {
            worker: name.to_owned(),
          }));
        }

        if let Some(refusal) = node_refusal(&self.node(&node).await?) {
          return Err(refusal);
        }

        let Some(path) = path.filter(|path| node_written(path)) else {
          return Err(refused(AgentRefusal::NodePathForm));
        };

        changes.cwd = Some(path);
      }
    }

    if !workers::update_worker(&self.db, name, &changes).await? {
      return Err(refused(AgentRefusal::NothingToChange));
    }

    Ok(())
  }
}

impl Workers for Built {
  async fn add_worker(&self, add: WorkerAdd) -> Done<WorkerAdded> {
    let label = add.label.clone().unwrap_or_else(|| add.name.clone());
    // No path given is a directory of the worker's own, made once the rest is accepted.
    let (cwd, made) = match (&add.path, &self.ground.worker_homes) {
      (Some(path), _) => (resolve(path), false),
      (None, Some(homes)) => (homes.join(&add.name), true),
      (None, None) => return Err(refused(AgentRefusal::PathEmpty)),
    };
    let cwd = cwd.display().to_string();
    let adding = Adding {
      name: &add.name,
      label: &label,
      cwd: &cwd,
      made,
      preamble: add.preamble.as_deref(),
      dev_command: add.dev_command.as_deref(),
    };

    self.judge()?.new_worker_fault(&adding, &taken_names(&self.db).await?)?;

    if made {
      tokio::fs::create_dir_all(&cwd)
        .await
        .map_err(|_| refused(AgentRefusal::MadePathBlocked { path: cwd.clone() }))?;
    }

    let row = NewWorker {
      name: &add.name,
      label: &label,
      cwd: &cwd,
      preamble: add.preamble.as_deref(),
      dev_command: add.dev_command.as_deref(),
    };

    workers::add_worker(&self.db, &row, now_ms()).await?;
    self.bench(&add.name).await;
    self.events.queue_moved(None);

    Ok(WorkerAdded {
      name: add.name,
      label,
      cwd,
    })
  }

  async fn update_worker(&self, name: String, update: WorkerUpdate) -> Done<()> {
    let held = self.held(&name).await?.ok_or_else(|| no_worker(&name))?;
    let mut changes = Changes {
      label: update.label,
      cwd: update.path.map(|path| resolve(&path).display().to_string()),
      preamble: update.preamble,
      dev_command: update.dev_command,
      ..Changes::default()
    };
    let units = self.units();
    // The one provider the worker is pinned to, or `auto` for none, as bmo-agency reads it.
    let pin = match &update.pinned_provider {
      Some(value) => Some(pin_choice(&units, value).await?),
      None => None,
    };
    let beyond = Beyond {
      runtime: pin.is_some() || update.keepalive_stopped.is_some(),
    };
    let judge = self.judge()?;

    // The base's two support settings, judged as the pair they make once written: either may come alone, and
    // the other is read off the row.
    let support_asked = update.support_count.is_some() || update.support_keepalive.is_some();

    if support_asked {
      let count = update.support_count.unwrap_or(held.support_count);
      let keepalive = update.support_keepalive.unwrap_or(held.support_keepalive);

      judge.support_fault(&held, count as f64, keepalive as f64)?;
      changes.support_count = Some(count);
      changes.support_keepalive = Some(keepalive);
    }

    judge.change_fault(&held, &changes, beyond)?;

    if !changes.any() && !beyond.runtime {
      return Err(refused(AgentRefusal::NothingToChange));
    }

    if changes.any() {
      workers::update_worker(&self.db, &name, &changes).await?;
    }

    let agency = &self.ground.agency;

    match pin {
      Some(PinChoice::Auto) => agency.unpin_worker(&name).await.map_err(agency_failed)?,
      Some(PinChoice::Pinned(pin)) => agency.pin_worker(&name, &pin).await.map_err(agency_failed)?,
      None => {}
    }

    match update.keepalive_stopped {
      Some(true) => agency.pause_keepalive(&name, now_ms()).await.map_err(agency_failed)?,
      Some(false) => agency.resume_keepalive(&name).await.map_err(agency_failed)?,
      None => {}
    }

    // The support rows follow the base. Where what is running cannot be read, nothing is removed.
    if support_asked || changes.label.is_some() || changes.cwd.is_some() {
      let busy = self.busy_workers().await?;

      settle_support(
        &self.db,
        &units,
        &self.policy.workers,
        &name,
        &self.ground.source,
        |worker| busy.as_ref().is_none_or(|busy| busy.iter().any(|one| one == worker)),
        now_ms(),
      )
      .await?;
    }

    if changes.dev_command.is_some() {
      self.bench(&name).await;
    }

    self.events.queue_moved(None);

    Ok(())
  }

  async fn remove_worker(&self, name: String) -> Done<()> {
    let held = self.held(&name).await?;

    remove_fault(held.as_ref(), &name)?;

    if !workers::remove_worker(&self.db, &name).await? {
      return Err(no_worker(&name));
    }

    self.events.queue_moved(None);

    Ok(())
  }

  async fn change_node(&self, change: NodeChange) -> Done<Option<NodeCheckout>> {
    let changed = match change {
      NodeChange::AddCheckout { name, node, path } => Some(self.add_checkout(&name, &node, path).await?),
      NodeChange::RemoveCheckout { name, node } => Some(self.remove_checkout(&name, &node).await?),
      NodeChange::Place { name, node, path } => {
        if name == BUILTIN_WORKER && node.is_some() {
          return Err(refused(AgentRefusal::BuiltinStaysHere));
        }

        self.place(&name, node, path).await?;
        None
      }
    };

    self.events.queue_moved(None);

    Ok(changed)
  }
}
