//! What the work-order runner's tests stand on: its ground over a scene's database and units, a host whose
//! folders stand in a scratch directory, the hold it keeps across its ticks with a repository check that
//! passes or fails as the test sets it, and one runner step carried as the step driver carries it.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use bmo_job::order_run::Ground;
use bmo_job::order_run::check::carry;
use bmo_job::order_run::hold::{Hold, RepoCheck};
use bmo_job::queue::job_start::Host;
use futures_util::future::BoxFuture;
use serde_json::Value;
use tempfile::TempDir;

use super::queue::{OWNER, Scene};

/// A repository check answering what the test set: passing unless it is told to fail.
#[derive(Clone, Default)]
pub struct Check {
  pub fails: Arc<AtomicBool>,
}

impl RepoCheck for Check {
  fn passes(&self, _: PathBuf, _: PathBuf) -> BoxFuture<'static, bool> {
    let passes = !self.fails.load(Ordering::SeqCst);

    Box::pin(async move { passes })
  }
}

/// The runner over a scene of its own.
pub struct Bench {
  pub scene: Scene,
  pub scratch: TempDir,
  pub host: Host,
  pub check: Check,
  pub hold: Hold,
}

impl Bench {
  pub async fn new() -> Self {
    let scene = Scene::new().await;
    let scratch = tempfile::tempdir().expect("a scratch directory");
    let folder = |name: &str| {
      let path = scratch.path().join(name);

      std::fs::create_dir_all(&path).expect("a scratch folder is made");
      path
    };
    let host = Host {
      program: PathBuf::from("/usr/bin/true"),
      cwd: folder("runner"),
      setenv: vec![],
      environment_file: None,
      locks: folder("locks"),
      prompts: folder("prompts"),
      env: BTreeMap::new(),
      systemd_run: PathBuf::from("/usr/bin/true"),
      lock_wait: Duration::from_secs(1),
    };
    let check = Check::default();
    let hold = Hold::new(Arc::new(check.clone()));

    folder("documents");
    folder("tree");
    folder("source");

    Self {
      scene,
      scratch,
      host,
      check,
      hold,
    }
  }

  /// The runner's ground: no systemd to ask, the owner's chat the scene's.
  pub fn ground(&self) -> Ground<'_> {
    Ground {
      db: &self.scene.db,
      units: &self.scene.units,
      policy: &self.scene.policy,
      host: &self.host,
      documents: self.scratch.path().join("documents"),
      tree: self.scratch.path().join("tree"),
      source: self.scratch.path().join("source"),
      systemctl: None,
      owner_chat: OWNER,
    }
  }

  /// One runner step, as the step driver carries it: what it came to, or its refusal.
  pub async fn carried(&self, asked: Value) -> Result<Value, Value> {
    let came = carry(&self.ground(), &self.hold, &asked).await;

    match came.get("ok") {
      Some(ok) => Ok(ok.clone()),
      None => Err(came["error"].clone()),
    }
  }

  /// One runner step that is to come to something.
  pub async fn step(&self, asked: Value) -> Value {
    let came = self.carried(asked.clone()).await;

    came.unwrap_or_else(|why| panic!("the step {asked} was refused: {why}"))
  }

  /// An item's work order as the database holds it.
  pub async fn order(&self, todo: i64) -> Value {
    queue_rows::work_orders::read(&self.scene.db, todo)
      .await
      .expect("the order reads")
      .expect("the item has an order")
  }

  /// An order written as the body given.
  pub async fn write_order(&self, mut order: Value) {
    queue_rows::work_orders::save(&self.scene.db, &mut order)
      .await
      .expect("the order is written");
  }
}
