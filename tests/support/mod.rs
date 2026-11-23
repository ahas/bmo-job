//! What the root package's tests share: the root and the walk over the files the repository checks read, and
//! the service run in process on loopback over a scratch database, its clock moved by hand, taking the shared
//! token and the tokens of three callers, one of them an approver and one supervising the work.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bmo_job::{Access, App, Caller, Clock, DeliveryPolicy, Questions, Store, Unattended};
use bmo_job_client::{ApprovalGiven, Item, ItemChange, ItemState, JobClient, NewItem};
use tempfile::TempDir;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpStream};
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

/// The caller every test's service marks as supervising the work, and its token.
pub const SUPERVISOR: &str = "lead";
pub const SUPERVISOR_TOKEN: &str = "supervisor-token";

/// The tokens every test's service takes.
pub fn access() -> Access {
  Access::shared(TOKEN)
    .with(APPROVER_TOKEN, Caller::named(APPROVER, true))
    .and_then(|access| access.with(CALLER_TOKEN, Caller::named(CALLER, false)))
    .and_then(|access| access.with(SUPERVISOR_TOKEN, Caller::named(SUPERVISOR, false).supervising()))
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
    } else if kind.is_file() && !is_skipped(&entry.file_name().to_string_lossy()) {
      files.push(entry.path());
    }
  }
}

/// A path as it reads relative to the root, for a failure's message.
pub fn shown(path: &Path) -> String {
  path.strip_prefix(ROOT).unwrap_or(path).display().to_string()
}

/// Delivery that retries at once and moves to failing after three failures, so no test waits on its backoff; a
/// failing subscription rests an hour, so only a test that sets its pace sees it tried again.
pub fn quick_delivery() -> DeliveryPolicy {
  DeliveryPolicy {
    first_retry: Duration::from_millis(1),
    longest_retry: Duration::from_millis(4),
    failures_to_failing: 3,
    answer_within: Duration::from_secs(10),
    failing_retry: Duration::from_secs(3_600),
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
    Self::start_on(policy, Unattended::default(), Questions::default()).await
  }

  /// The service whose questions wait `wait_ms` on whoever holds them at the most.
  pub async fn start_with_question_wait(wait_ms: i64) -> Self {
    Self::start_on(quick_delivery(), Unattended::default(), Questions { wait_ms }).await
  }

  /// The service whose raised items open by themselves where they carry one of `labels`.
  pub async fn start_unattended(labels: &[&str]) -> Self {
    let unattended = Unattended {
      labels: labels.iter().map(|label| (*label).to_owned()).collect(),
    };
    Self::start_on(quick_delivery(), unattended, Questions::default()).await
  }

  async fn start_on(policy: DeliveryPolicy, unattended: Unattended, questions: Questions) -> Self {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let url = format!("sqlite://{}?mode=rwc", dir.path().join("job.db").display());
    let clock = Clock::manual(START);
    let store = Store::open(&url, clock.clone())
      .await
      .expect("the store opens")
      .with_question_wait(questions.wait_ms);
    let app = App::new(store.clone(), access()).unattended(unattended);
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
  /// A client with the token of the caller that supervises the work.
  pub fn supervisor(&self) -> JobClient {
    self.client_with(SUPERVISOR_TOKEN)
  }

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

/// A stand-in in front of a peer on loopback, standing for that peer's restarts: it passes each connection
/// through while the peer is up; going down cuts every connection it holds and every new one until it comes up
/// again; and it can let a request reach the peer and cut the connection before the answer comes back.
pub struct Relay {
  pub url: String,
  state: Arc<RelayState>,
  task: JoinHandle<()>,
}

#[derive(Default)]
struct RelayState {
  down: AtomicBool,
  answers_to_cut: AtomicUsize,
  cuts: AtomicUsize,
  live: Mutex<Vec<JoinHandle<()>>>,
}

impl Drop for Relay {
  fn drop(&mut self) {
    self.task.abort();
    self.state.live.lock().expect("the live connections").iter().for_each(JoinHandle::abort);
  }
}

impl Relay {
  /// A relay in front of the peer at `url`, answering at the same path on a port of its own.
  pub async fn before(url: &str) -> Self {
    let target = url::Url::parse(url).expect("the peer's address");
    let peer = format!(
      "{}:{}",
      target.host_str().expect("the peer's host"),
      target.port_or_known_default().expect("the peer's port")
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("a loopback port");
    let mut url = target.clone();
    url
      .set_port(Some(listener.local_addr().expect("the relay's address").port()))
      .expect("a port");
    let state = Arc::new(RelayState::default());
    let serving = Arc::clone(&state);
    let task = tokio::spawn(async move {
      while let Ok((inbound, _)) = listener.accept().await {
        if serving.down.load(Ordering::SeqCst) {
          serving.cuts.fetch_add(1, Ordering::SeqCst);
          drop(inbound);
          continue;
        }
        let one = tokio::spawn(relay_one(inbound, peer.clone(), Arc::clone(&serving)));
        let mut live = serving.live.lock().expect("the live connections");
        live.retain(|connection| !connection.is_finished());
        live.push(one);
      }
    });

    Self {
      url: url.as_str().trim_end_matches('/').to_owned(),
      state,
      task,
    }
  }

  /// The peer goes down: every connection standing is cut, and every new one is cut as it comes.
  pub fn go_down(&self) {
    self.state.down.store(true, Ordering::SeqCst);
    let live: Vec<_> = self.state.live.lock().expect("the live connections").drain(..).collect();
    for connection in live {
      if !connection.is_finished() {
        connection.abort();
        self.state.cuts.fetch_add(1, Ordering::SeqCst);
      }
    }
  }

  /// The peer is up again.
  pub fn come_up(&self) {
    self.state.down.store(false, Ordering::SeqCst);
  }

  /// The next `count` new connections reach the peer, and are cut once it begins to answer.
  pub fn cut_answers(&self, count: usize) {
    self.state.answers_to_cut.store(count, Ordering::SeqCst);
  }

  /// How many connections were cut so far.
  pub fn cuts(&self) -> usize {
    self.state.cuts.load(Ordering::SeqCst)
  }

  /// Waits, within a bound, until `count` connections were cut, so a caller is known to have met the cut.
  pub async fn until_cut(&self, count: usize) {
    for _ in 0..1_200 {
      if self.cuts() >= count {
        return;
      }
      tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("the relay cut {} connections, not {count}", self.cuts());
  }
}

async fn relay_one(mut inbound: TcpStream, peer: String, state: Arc<RelayState>) {
  let Ok(mut outbound) = TcpStream::connect(&peer).await else {
    return;
  };
  let cut_answer = state
    .answers_to_cut
    .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| left.checked_sub(1))
    .is_ok();
  if !cut_answer {
    let _ = tokio::io::copy_bidirectional(&mut inbound, &mut outbound).await;
    return;
  }
  let (mut asked, _) = inbound.split();
  let (mut answered, mut asking) = outbound.split();
  let mut first = [0_u8; 1];
  tokio::select! {
    _ = tokio::io::copy(&mut asked, &mut asking) => {}
    _ = answered.read(&mut first) => {}
  }
  state.cuts.fetch_add(1, Ordering::SeqCst);
}
