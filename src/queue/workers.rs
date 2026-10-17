//! The job workers as the `workers` table holds them, the built-in standing whether its row has been
//! written yet or not, and a base's support workers following its settings (docs/queue.md, "Workers").
//! A resting worker is no worker of the list.

use std::path::Path;

use job_policy::workers::WorkerRules;
use queue_read::lines::BUILTIN_WORKER;
use sea_orm::{ConnectionTrait, QueryResult, TransactionTrait, Value};

use super::paths::resolve;
use super::units::Units;
use crate::{Refusal, all, one, run};

/// The icon a worker's row is written with.
const WORKER_ICON: &str = "i-material-symbols-construction";

/// One worker as the queue reads it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Worker {
  pub name: String,
  pub label: String,
  pub cwd: String,
  pub builtin: bool,
  pub preamble: Option<String>,
  pub dev_command: Option<String>,
  pub support_count: i64,
  pub support_keepalive: i64,
  pub support_of: Option<String>,
  pub support_slot: Option<i64>,
  /// The work node it stands on, none for this machine.
  pub node: Option<String>,
}

impl Worker {
  fn of(row: &QueryResult, source: &Path) -> Result<Self, sea_orm::DbErr> {
    let name: String = row.try_get("", "name")?;

    Ok(Self {
      label: row.try_get::<Option<String>>("", "label")?.unwrap_or_else(|| name.clone()),
      cwd: row
        .try_get::<Option<String>>("", "cwd")?
        .unwrap_or_else(|| source.to_string_lossy().into_owned()),
      builtin: row.try_get::<i64>("", "builtin")? != 0,
      preamble: row.try_get("", "preamble")?,
      dev_command: row.try_get("", "dev_command")?,
      support_count: row.try_get("", "support_count")?,
      support_keepalive: row.try_get("", "support_keepalive")?,
      support_of: row.try_get::<Option<String>>("", "support_of")?.filter(|base| !base.is_empty()),
      support_slot: row.try_get("", "support_slot")?,
      node: row.try_get::<Option<String>>("", "node")?.filter(|node| !node.is_empty()),
      name,
    })
  }

  /// The built-in worker as it stands before its row is written, in the source root.
  pub fn builtin(source: &Path) -> Self {
    Self {
      name: BUILTIN_WORKER.into(),
      label: BUILTIN_WORKER.into(),
      cwd: source.to_string_lossy().into_owned(),
      builtin: true,
      preamble: None,
      dev_command: None,
      support_count: 0,
      support_keepalive: 0,
      support_of: None,
      support_slot: None,
      node: None,
    }
  }

  /// Whether it is a support worker rather than a base.
  pub fn is_support(&self) -> bool {
    self.support_of.is_some()
  }

  /// Whether it is a base's checkout on a node rather than a numbered worktree here.
  pub fn is_node_checkout(&self) -> bool {
    self.node.is_some() && self.support_slot.is_none()
  }

  /// The name its work is filed under: its base's where it is a support worker, else its own.
  pub fn base_name(&self) -> &str {
    self.support_of.as_deref().unwrap_or(&self.name)
  }

  /// Whether, as a support worker, it is within its base's count, which is what makes it usable. A
  /// checkout on a node is counted by no number and stands until it is removed.
  pub fn in_use(&self, base: &Worker, rules: &WorkerRules) -> bool {
    if self.is_node_checkout() {
      return true;
    }

    let slot = self.support_slot.unwrap_or(0);

    slot >= rules.first_support_slot && slot < rules.first_support_slot + base.support_count
  }
}

/// Whether a worker of that name stands: the built-in always, any other while its row is not resting.
pub async fn is_worker(db: &impl ConnectionTrait, name: &str) -> Result<bool, Refusal> {
  if name == BUILTIN_WORKER {
    return Ok(true);
  }

  let row = one(
    db,
    r#"SELECT 1 AS "one" FROM "workers" WHERE "name" = $1 AND "resting_since" IS NULL"#,
    vec![name.into()],
  )
  .await?;

  Ok(row.is_some())
}

/// Every worker, each base followed by its support workers in their order; the built-in stands first
/// whether its row has been written or not, working in the source root where it has none.
pub async fn workers(db: &impl ConnectionTrait, source: &Path) -> Result<Vec<Worker>, Refusal> {
  let rows = all(
    db,
    r#"SELECT a.* FROM "workers" a WHERE a."resting_since" IS NULL
       ORDER BY COALESCE((SELECT b."builtin" FROM "workers" b WHERE b."name" = a."support_of"), a."builtin") DESC,
         COALESCE(a."support_of", a."name"), COALESCE(a."support_slot", 1), a."name""#,
    vec![],
  )
  .await?;
  let mut workers: Vec<Worker> =
    rows.iter().map(|row| Worker::of(row, source)).collect::<Result<_, _>>()?;

  if !workers.iter().any(|worker| worker.name == BUILTIN_WORKER) {
    workers.insert(0, Worker::builtin(source));
  }

  Ok(workers)
}

/// One worker's row as it stands, resting or not.
pub async fn worker_row(
  db: &impl ConnectionTrait,
  name: &str,
  source: &Path,
) -> Result<Option<Worker>, Refusal> {
  let row = one(db, r#"SELECT * FROM "workers" WHERE "name" = $1"#, vec![name.into()]).await?;

  Ok(row.as_ref().map(|row| Worker::of(row, source)).transpose()?)
}

/// Every name a worker's row stands under, a resting worker's included: the names a new worker may not
/// take.
pub async fn taken_names(db: &impl ConnectionTrait) -> Result<Vec<String>, Refusal> {
  let rows = all(db, r#"SELECT "name" FROM "workers" ORDER BY "name""#, vec![]).await?;

  Ok(rows.iter().map(|row| row.try_get("", "name")).collect::<Result<_, _>>()?)
}

async fn named(db: &impl ConnectionTrait, name: &str) -> Result<bool, Refusal> {
  Ok(
    one(db, r#"SELECT 1 AS "one" FROM "workers" WHERE "name" = $1"#, vec![name.into()])
      .await?
      .is_some(),
  )
}

/// A worker the owner added.
pub struct NewWorker<'a> {
  pub name: &'a str,
  pub label: &'a str,
  pub cwd: &'a str,
  pub preamble: Option<&'a str>,
  pub dev_command: Option<&'a str>,
}

/// The worker's row written, at `now`.
pub async fn add_worker(
  db: &impl ConnectionTrait,
  worker: &NewWorker<'_>,
  now: i64,
) -> Result<(), Refusal> {
  run(
    db,
    r#"INSERT INTO "workers" ("name", "label", "cwd", "icon", "builtin", "preamble", "dev_command", "created_at")
       VALUES ($1, $2, $3, $4, 0, $5, $6, $7)"#,
    vec![
      worker.name.into(),
      worker.label.into(),
      worker.cwd.into(),
      WORKER_ICON.into(),
      worker.preamble.into(),
      worker.dev_command.into(),
      now.into(),
    ],
  )
  .await?;

  Ok(())
}

/// What the owner asked to change of a worker's row; a field left none is left alone.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Changes {
  pub label: Option<String>,
  pub cwd: Option<String>,
  pub preamble: Option<Option<String>>,
  pub dev_command: Option<Option<String>>,
  pub support_count: Option<i64>,
  pub support_keepalive: Option<i64>,
  /// The work node it stands on; none stands it on this machine.
  pub node: Option<Option<String>>,
}

impl Changes {
  /// Whether it changes anything at all.
  pub fn any(&self) -> bool {
    *self != Self::default()
  }
}

/// Only what was named moves; the rest of the row is the record. Whether a worker's row changed.
pub async fn update_worker(
  db: &impl ConnectionTrait,
  name: &str,
  changes: &Changes,
) -> Result<bool, Refusal> {
  let mut sets: Vec<String> = vec![];
  let mut values: Vec<Value> = vec![];
  let mut set = |column: &str, value: Value| {
    values.push(value);
    sets.push(format!(r#""{column}" = ${}"#, values.len()));
  };

  if let Some(label) = &changes.label {
    set("label", label.clone().into());
  }

  if let Some(cwd) = &changes.cwd {
    set("cwd", cwd.clone().into());
  }

  if let Some(preamble) = &changes.preamble {
    set("preamble", preamble.clone().into());
  }

  if let Some(dev_command) = &changes.dev_command {
    set("dev_command", dev_command.clone().into());
  }

  if let Some(count) = changes.support_count {
    set("support_count", count.into());
  }

  if let Some(keepalive) = changes.support_keepalive {
    set("support_keepalive", keepalive.into());
  }

  if let Some(node) = &changes.node {
    set("node", node.clone().into());
  }

  if sets.is_empty() {
    return Ok(false);
  }

  values.push(name.into());

  let sql = format!(r#"UPDATE "workers" SET {} WHERE "name" = ${}"#, sets.join(", "), values.len());

  Ok(run(db, &sql, values).await? > 0)
}

/// A worker the owner added taken away; the built-in never is. Whether there was one.
pub async fn remove_worker(db: &impl ConnectionTrait, name: &str) -> Result<bool, Refusal> {
  let gone =
    run(db, r#"DELETE FROM "workers" WHERE "name" = $1 AND "builtin" = 0"#, vec![name.into()]).await?;

  Ok(gone > 0)
}

/// A support worker's name.
pub fn support_name(base: &str, slot: i64) -> String {
  format!("{base}-w{slot}")
}

/// A support worker's directory, beside the base's checkout.
fn support_path(base_cwd: &str, slot: i64) -> String {
  format!("{}-w{slot}", resolve(base_cwd).display())
}

/// What a support worker is called on a screen.
fn support_label(base_label: &str, slot: i64) -> String {
  format!("{base_label}#{slot}")
}

/// What a base's checkout on a node is called on a screen.
pub fn node_checkout_label(base_label: &str, node_label: &str) -> String {
  format!("{base_label}@{node_label}")
}

/// The support rows one settling wrote and took away, by name, and those whose names another worker
/// already holds, which the base goes without.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Settled {
  pub added: Vec<String>,
  pub removed: Vec<String>,
  pub name_taken: Vec<String>,
}

/// A base's support workers brought to its settings, at `now`: written where the count asks for more,
/// their labels and places following the base's, a checkout on a node called by the base and the node,
/// and removed where the count no longer reaches them once nothing holds them. A support worker the busy
/// test names is left standing.
pub async fn settle_support(
  db: &(impl ConnectionTrait + TransactionTrait),
  units: &dyn Units,
  rules: &WorkerRules,
  base: &str,
  source: &Path,
  busy: impl Fn(&str) -> bool,
  now: i64,
) -> Result<Settled, Refusal> {
  let mut settled = Settled::default();
  let Some(row) = worker_row(db, base, source).await? else {
    return Ok(settled);
  };

  if row.is_support() {
    return Ok(settled);
  }

  let held: Vec<Worker> = workers(db, source)
    .await?
    .into_iter()
    .filter(|worker| worker.support_of.as_deref() == Some(base))
    .collect();

  for slot in rules.first_support_slot..rules.first_support_slot + row.support_count {
    let name = support_name(base, slot);
    let wanted_label = support_label(&row.label, slot);
    let wanted_cwd = support_path(&row.cwd, slot);

    if let Some(existing) = held.iter().find(|worker| worker.support_slot == Some(slot)) {
      if existing.label != wanted_label || existing.cwd != wanted_cwd {
        let changes = Changes {
          label: Some(wanted_label),
          cwd: Some(wanted_cwd),
          ..Changes::default()
        };

        update_worker(db, &existing.name, &changes).await?;
      }

      continue;
    }

    if named(db, &name).await? {
      settled.name_taken.push(name);
      continue;
    }

    run(
      db,
      r#"INSERT INTO "workers" ("name", "label", "cwd", "icon", "builtin", "support_of", "support_slot", "created_at")
         VALUES ($1, $2, $3, $4, 0, $5, $6, $7)"#,
      vec![
        name.clone().into(),
        wanted_label.into(),
        wanted_cwd.into(),
        WORKER_ICON.into(),
        base.into(),
        slot.into(),
        now.into(),
      ],
    )
    .await?;
    settled.added.push(name);
  }

  let on_nodes: Vec<&Worker> = held.iter().filter(|worker| worker.is_node_checkout()).collect();

  if !on_nodes.is_empty() {
    let nodes = units.nodes().await?;

    for worker in on_nodes {
      let node = worker.node.as_deref().unwrap_or_default();
      let node_label = nodes.iter().find(|one| one.name == node).map_or(node, |one| one.label.as_str());
      let wanted = node_checkout_label(&row.label, node_label);

      if worker.label != wanted {
        let changes = Changes {
          label: Some(wanted),
          ..Changes::default()
        };

        update_worker(db, &worker.name, &changes).await?;
      }
    }
  }

  for worker in held.iter().filter(|worker| !worker.in_use(&row, rules) && !busy(&worker.name)) {
    if remove_support_worker(db, &worker.name).await? {
      settled.removed.push(worker.name.clone());
    }
  }

  Ok(settled)
}

/// A support worker taken away with the checkouts it lent; whether there was one.
pub async fn remove_support_worker(db: &impl TransactionTrait, name: &str) -> Result<bool, Refusal> {
  let unit = db.begin().await?;
  let gone = run(
    &unit,
    r#"DELETE FROM "workers" WHERE "name" = $1 AND "support_of" IS NOT NULL"#,
    vec![name.into()],
  )
  .await?
    > 0;

  if gone {
    run(&unit, r#"DELETE FROM "checkout_leases" WHERE "worker" = $1"#, vec![name.into()]).await?;
  }

  unit.commit().await?;

  Ok(gone)
}

/// A base's checkout on a node, a support worker of its own placed there and numbered by no slot.
pub struct NodeCheckout<'a> {
  pub name: &'a str,
  pub label: &'a str,
  pub cwd: &'a str,
  pub base: &'a str,
  pub node: &'a str,
}

/// The checkout's row written, at `now`.
pub async fn add_node_checkout(
  db: &impl ConnectionTrait,
  checkout: &NodeCheckout<'_>,
  now: i64,
) -> Result<(), Refusal> {
  run(
    db,
    r#"INSERT INTO "workers" ("name", "label", "cwd", "icon", "builtin", "support_of", "node", "created_at")
       VALUES ($1, $2, $3, $4, 0, $5, $6, $7)"#,
    vec![
      checkout.name.into(),
      checkout.label.into(),
      checkout.cwd.into(),
      WORKER_ICON.into(),
      checkout.base.into(),
      checkout.node.into(),
      now.into(),
    ],
  )
  .await?;

  Ok(())
}

/// A worker the owner added set to rest at `now`, or woken; whether there was such a worker. The
/// built-in and a support worker never rest, and a worker already resting keeps the moment it began.
pub async fn set_resting(
  db: &impl ConnectionTrait,
  name: &str,
  resting: bool,
  now: i64,
) -> Result<bool, Refusal> {
  let (sql, values): (&str, Vec<Value>) = if resting {
    (
      r#"UPDATE "workers" SET "resting_since" = COALESCE("resting_since", $1)
         WHERE "name" = $2 AND "builtin" = 0 AND "support_of" IS NULL"#,
      vec![now.into(), name.into()],
    )
  } else {
    (
      r#"UPDATE "workers" SET "resting_since" = NULL
         WHERE "name" = $1 AND "builtin" = 0 AND "support_of" IS NULL"#,
      vec![name.into()],
    )
  };

  Ok(run(db, sql, values).await? > 0)
}

/// One resting worker: what it is called and since when it rests.
#[derive(Debug, PartialEq, Eq)]
pub struct Resting {
  pub name: String,
  pub label: Option<String>,
  pub since: i64,
}

/// The resting workers, by name.
pub async fn resting_workers(db: &impl ConnectionTrait) -> Result<Vec<Resting>, Refusal> {
  let rows = all(
    db,
    r#"SELECT "name", "label", "resting_since" FROM "workers" WHERE "resting_since" IS NOT NULL ORDER BY "name""#,
    vec![],
  )
  .await?;

  Ok(
    rows
      .iter()
      .map(|row| {
        Ok(Resting {
          name: row.try_get("", "name")?,
          label: row.try_get("", "label")?,
          since: row.try_get("", "resting_since")?,
        })
      })
      .collect::<Result<_, sea_orm::DbErr>>()?,
  )
}
