//! What the root package's tests share: the root and the walk over the files the repository checks read, and
//! the service run in process on loopback over a scratch database, its clock moved by hand, taking the shared
//! token and the tokens of two callers, one of them an approver.

// Each test file is its own crate and uses only part of this module.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use bmo_job::{Access, App, Caller, Clock, DeliveryPolicy, Store};
use bmo_job_client::{ApprovalGiven, Item, ItemChange, ItemState, JobClient, NewItem};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;

pub const ROOT: &str = env!("CARGO_MANIFEST_DIR");

/// The shared token every test's service takes, which names no caller.
pub const TOKEN: &str = "test-token";

/// The caller every test's service marks as an approver, and its token.
pub const APPROVER: &str = "qa-admin";
pub const APPROVER_TOKEN: &str = "approver-token";

/// A caller with a token of its own that is no approver, and its token.
pub const CALLER: &str = "tester";
pub const CALLER_TOKEN: &str = "caller-token";

/// The tokens every test's service takes.
pub fn access() -> Access {
  Access::shared(TOKEN)
    .with(APPROVER_TOKEN, Caller::named(APPROVER, true))
    .and_then(|access| access.with(CALLER_TOKEN, Caller::named(CALLER, false)))
    .expect("the tokens differ")
}

/// Where every test's clock starts.
pub const START: i64 = 1_700_000_000_000;

/// Directories no check reads: build output, git's own and installed packages.
const SKIPPED: [&str; 3] = ["target", ".git", "node_modules"];

pub fn is_skipped(name: &str) -> bool {
  SKIPPED.contains(&name)
}

/// Every file under `dir`, at any depth, skipping build output and git's directory. A directory that does not
/// exist holds no files.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
  let mut files = Vec::new();
  if dir.is_dir() {
    collect(dir, &mut files);
  }
  files.sort();
  files
}

fn collect(dir: &Path, files: &mut Vec<PathBuf>) {
  for entry in fs::read_dir(dir).expect("a directory is readable") {
    let entry = entry.expect("an entry is readable");
    let kind = entry.file_type().expect("an entry's type is readable");
    if kind.is_dir() {
      if !is_skipped(&entry.file_name().to_string_lossy()) {
        collect(&entry.path(), files);
      }
    } else if kind.is_file() {
      files.push(entry.path());
    }
  }
}

/// A path as it reads relative to the root, for a failure's message.
pub fn shown(path: &Path) -> String {
  path.strip_prefix(ROOT).unwrap_or(path).display().to_string()
}

/// Delivery that retries at once and gives up after three failures, so no test waits on its backoff.
pub fn quick_delivery() -> DeliveryPolicy {
  DeliveryPolicy {
    answer_within: Duration::from_secs(5),
    first_retry: Duration::from_millis(1),
    longest_retry: Duration::from_millis(4),
    failures_to_failing: 3,
  }
}

/// The service running in this process, taken down when it is dropped.
pub struct Service {
  pub url: String,
  pub clock: Clock,
  pub store: Store,
  pub app: App,
  _dir: TempDir,
  tasks: Vec<JoinHandle<()>>,
}

impl Drop for Service {
  fn drop(&mut self) {
    self.app.stop();
    self.tasks.iter().for_each(JoinHandle::abort);
  }
}

impl Service {
  pub async fn start() -> Self {
    Self::start_with(quick_delivery()).await
  }

  pub async fn start_with(policy: DeliveryPolicy) -> Self {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let url = format!("sqlite://{}?mode=rwc", dir.path().join("job.db").display());
    let clock = Clock::manual(START);
    let store = Store::open(&url, clock.clone()).await.expect("the store opens");
    let app = App::new(store.clone(), access());
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
    let address = listener.local_addr().expect("the port's address");
    let mut tasks = bmo_job::spawn_background(store.clone(), policy);
    let served = app.clone();
    tasks.push(tokio::spawn(async move {
      let _ = bmo_job::serve_on(listener, served, std::future::pending()).await;
    }));
    Self {
      url: format!("http://{address}"),
      clock,
      store,
      app,
      _dir: dir,
      tasks,
    }
  }

  /// A client with the shared token.
  pub fn client(&self) -> JobClient {
    self.client_with(TOKEN)
  }

  /// A client with the approver's token.
  pub fn approver(&self) -> JobClient {
    self.client_with(APPROVER_TOKEN)
  }

  /// A client with `token`.
  pub fn client_with(&self, token: &str) -> JobClient {
    JobClient::new(&self.url, token).expect("the client takes the address")
  }

  /// A request on the raw route, with the token.
  pub fn raw(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
    reqwest::Client::new().request(method, format!("{}{path}", self.url)).bearer_auth(TOKEN)
  }

  /// Moves the clock on and tells what has come due.
  pub async fn pass(&self, ms: i64) {
    self.clock.advance(ms);
    self.store.sweep().await.expect("the sweep runs");
  }

  /// Raises an item in `queue` and opens it with the approver's approval.
  pub async fn open_item(&self, queue: &str, title: &str) -> Item {
    let client = self.client();
    let item = client
      .create_item(&NewItem {
        title: title.to_owned(),
        brief: format!("the brief of {title}"),
        queue_name: queue.to_owned(),
        ..NewItem::default()
      })
      .await
      .expect("the item is raised");
    let opened = self
      .approver()
      .change_item(
        item.id,
        &ItemChange {
          approval: Some(ApprovalGiven::default()),
          ..ItemChange::default()
        },
      )
      .await
      .expect("the item is approved");
    assert_eq!(opened.state, ItemState::Open);
    opened
  }
}
