//! One work order in a worker's checkout at a time (docs/queue.md, "A checkout held"): the checkout taken
//! once what ran there before has left it, a worktree of the worker's own or a base beside support workers put
//! on the item's branch, and a parked change brought back; and let go as the order stops or ends — a stopped
//! order's own change, picked out by its turns' windows, committed onto the checkout's branch where the
//! repository's own check passes with it and onto its `wip/` branch where it does not, and a base checkout
//! put back on main. What the order waits on is kept on it as a release wait, and what became of its change
//! as checkout notes.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use futures_util::future::BoxFuture;
use job_codes::checkout_holds::{CheckoutNote, GitStep, ReleaseWait};
use job_testbed::{Check as TestbedCheck, Outcome, Testbed, TestbedClient};
use serde_json::{Value, json};

use super::pool::branch_task;
use super::{Fault, Ground, faulted};
use crate::job_run::checkout::{Checkout, is_work_branch, item_branch, jobs_work_on_branches, resolved};
use crate::job_run::worktree::{
  WorktreeState, new_work, nothing_open, read_worktree_state, worktree_state,
};
use crate::queue::job_start::lock_held;
use crate::queue::turn::orders_in;
use crate::queue::work_orders::{current_places, number, places, task_at};
use crate::{all, now_ms, one};

// How long a push is given.
const PUSH_WAIT: Duration = Duration::from_secs(120);

/// How long after a job of a worker ended its unit is still asked about.
pub const ENDED_UNIT_WINDOW_MS: i64 = 15 * 60 * 1000;

/// How many times a stopped order's check is started again because its change moved under it.
pub const CHECK_RESTART_LIMIT: u32 = 3;

/// The program a repository's own worktree setup script runs on.
pub const SETUP_RUNTIME: &str = "bun";

// The worker a job with no worker of its own is.
const BUILTIN_WORKER: &str = "worker";

fn text<'v>(value: &'v Value, key: &str) -> Option<&'v str> {
  value.get(key).and_then(Value::as_str)
}

fn locked<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
  mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn count(value: usize) -> u32 {
  u32::try_from(value).unwrap_or(u32::MAX)
}

/// Whether a checkout's own check passes with what is in it now, its output in `log`: the repository's check
/// as bmo-testbed runs it.
pub trait RepoCheck: Send + Sync {
  fn passes(&self, checkout: PathBuf, log: PathBuf) -> BoxFuture<'static, bool>;
}

/// The repository check run through bmo-testbed: a check that passed passes; one that failed, or whose turn
/// did not come, does not.
pub struct TestbedRepoCheck(pub TestbedClient);

impl RepoCheck for TestbedRepoCheck {
  fn passes(&self, checkout: PathBuf, log: PathBuf) -> BoxFuture<'static, bool> {
    let testbed = self.0.clone();

    Box::pin(async move {
      if let Some(parent) = log.parent() {
        let _ = std::fs::create_dir_all(parent);
      }

      let asked = TestbedCheck {
        checkout,
        since: None,
        log,
      };

      matches!(testbed.check(&asked).await, Ok(Outcome::Passed))
    })
  }
}

/// The checkout's branch put on its remote; what refused it, where something did.
pub async fn push(cwd: &str) -> Option<String> {
  let child = tokio::process::Command::new("git")
    .args(["push", "-q", "origin", "HEAD"])
    .current_dir(cwd)
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::piped())
    .kill_on_drop(true)
    .spawn();
  let Ok(child) = child else { return Some("git push could not start".to_owned()) };

  match tokio::time::timeout(PUSH_WAIT, child.wait_with_output()).await {
    Ok(Ok(done)) if done.status.success() => None,
    Ok(Ok(done)) => {
      let said = String::from_utf8_lossy(&done.stderr).trim().to_owned();

      Some(if said.is_empty() {
        format!("git push exited {}", done.status.code().unwrap_or(-1))
      } else {
        said
      })
    }
    _ => Some("git push did not end in time".to_owned()),
  }
}

// Git in a checkout, as the park helpers read it: whether it worked and what it said.
struct Git {
  ok: bool,
  out: String,
  err: String,
}

fn git(cwd: &str, args: &[&str], env: &[(&str, &str)]) -> Git {
  let ran = std::process::Command::new("git")
    .args(args)
    .current_dir(cwd)
    .envs(env.iter().copied())
    .stdin(Stdio::null())
    .output();

  match ran {
    Ok(ran) => Git {
      ok: ran.status.success(),
      out: String::from_utf8_lossy(&ran.stdout).into_owned(),
      err: String::from_utf8_lossy(&ran.stderr).into_owned(),
    },
    Err(why) => Git {
      ok: false,
      out: String::new(),
      err: why.to_string(),
    },
  }
}

// What a git step did, in the words a record keeps.
fn step_words(step: GitStep) -> &'static str {
  match step {
    GitStep::Add => "stage the change",
    GitStep::Commit => "commit the change",
    GitStep::ReadCommit => "read the commit",
    GitStep::ReadHead => "read HEAD",
    GitStep::WriteTree => "write the tree",
    GitStep::WriteCommit => "write the commit",
    GitStep::MoveBranch => "move the branch",
    GitStep::TakeBack => "take the change out of the tree",
    GitStep::ReadBranch => "read the branch",
  }
}

fn must(result: Git, step: GitStep) -> Result<String, Fault> {
  if !result.ok {
    let said = if result.err.is_empty() { result.out } else { result.err };

    return Err(format!("git could not {}: {}", step_words(step), said.trim()));
  }

  Ok(result.out.trim().to_owned())
}

/// When one of the order's own turns was at work in the checkout, in milliseconds.
#[derive(Clone, Copy, Debug)]
pub struct TurnWindow {
  pub from: i64,
  pub to: i64,
}

// A path's modification time in milliseconds, where it stands; a symlink's own, never its target's.
fn modified_ms(cwd: &str, path: &str) -> Option<f64> {
  let meta = std::fs::symlink_metadata(Path::new(cwd).join(path)).ok()?;
  let since = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;

  Some(since.as_secs_f64() * 1000.0)
}

/// Whether a path now open was written by one of the turns: a file by its modification time, a deleted
/// file — which has none — by any turn having run at all.
pub fn written_in(cwd: &str, path: &str, turns: &[TurnWindow]) -> bool {
  match modified_ms(cwd, path) {
    None => !turns.is_empty(),
    Some(at) => turns.iter().any(|turn| at >= turn.from as f64 && at <= turn.to as f64),
  }
}

/// The change opened since a baseline while one of the turns ran: what can be taken, and what was changed on
/// top of somebody else's open change or is no plain file and cannot be.
pub struct OwnChange {
  pub paths: Vec<String>,
  pub left: Vec<String>,
}

pub fn own_change(cwd: &str, baseline: &WorktreeState, turns: &[TurnWindow]) -> Result<OwnChange, Fault> {
  let now = worktree_state(&Checkout::local(cwd))?;
  let opened = new_work(baseline, &now);
  let tracked: Vec<String> =
    opened.tracked.into_iter().filter(|path| written_in(cwd, path, turns)).collect();
  let untracked: Vec<String> =
    opened.untracked.into_iter().filter(|path| written_in(cwd, path, turns)).collect();
  let mut left: Vec<String> =
    tracked.iter().filter(|path| baseline.tracked.contains_key(*path)).cloned().collect();
  let mut paths: Vec<String> =
    tracked.into_iter().filter(|path| !baseline.tracked.contains_key(path)).collect();

  for path in untracked {
    let plain = std::fs::symlink_metadata(Path::new(cwd).join(&path)).is_ok_and(|meta| meta.is_file());

    if plain { paths.push(path) } else { left.push(path) }
  }

  Ok(OwnChange { paths, left })
}

/// What a change's check was run against: its paths and when each was last written.
pub fn change_mark(cwd: &str, paths: &[String]) -> String {
  let marked: Vec<Value> = paths
    .iter()
    .map(|path| json!([path, modified_ms(cwd, path).map_or(Value::Null, |at| json!(at))]))
    .collect();

  Value::Array(marked).to_string()
}

/// Exactly these paths committed onto the branch the checkout is on, and nothing else in its index.
pub fn commit_on_checkout(cwd: &str, paths: &[String], message: &str) -> Result<String, Fault> {
  let listed: Vec<&str> = paths.iter().map(String::as_str).collect();
  let add: Vec<&str> = ["add", "-A", "--"].into_iter().chain(listed.iter().copied()).collect();
  let commit: Vec<&str> = ["commit", "-q", "-m", message, "--only", "--"]
    .into_iter()
    .chain(listed.iter().copied())
    .collect();

  must(git(cwd, &add, &[]), GitStep::Add)?;
  must(git(cwd, &commit, &[]), GitStep::Commit)?;
  must(git(cwd, &["rev-parse", "HEAD"], &[]), GitStep::ReadCommit)
}

/// These paths committed onto `branch`, on top of the checkout's HEAD, without moving the checkout off its own
/// branch, then taken out of the working tree; the commit is made from an index of its own, so nothing
/// somebody else staged goes with it.
pub fn commit_on_branch(
  cwd: &str,
  paths: &[String],
  branch: &str,
  message: &str,
  scratch: &Path,
) -> Result<String, Fault> {
  // The order's own folder stands only once one of its turns has launched; an order stopped before that
  // still parks there.
  let _ = std::fs::create_dir_all(scratch);

  let index = scratch.join(format!("park-{}.index", now_ms()));
  let index_text = index.to_string_lossy().into_owned();
  let env = [("GIT_INDEX_FILE", index_text.as_str())];
  let listed: Vec<&str> = paths.iter().map(String::as_str).collect();
  let add: Vec<&str> = ["add", "-A", "--"].into_iter().chain(listed.iter().copied()).collect();
  let committed = (|| {
    must(git(cwd, &["read-tree", "HEAD"], &env), GitStep::ReadHead)?;
    must(git(cwd, &add, &env), GitStep::Add)?;

    let tree = must(git(cwd, &["write-tree"], &env), GitStep::WriteTree)?;
    let commit = must(
      git(cwd, &["commit-tree", &tree, "-p", "HEAD", "-m", message], &[]),
      GitStep::WriteCommit,
    )?;

    must(
      git(cwd, &["update-ref", "-m", message, &format!("refs/heads/{branch}"), &commit], &[]),
      GitStep::MoveBranch,
    )?;
    take_out(cwd, paths)?;

    Ok(commit)
  })();

  let _ = std::fs::remove_file(&index);

  committed
}

// The paths back to what HEAD says: a file HEAD has is checked out again, one it has not is dropped from the
// index and from the disk.
fn take_out(cwd: &str, paths: &[String]) -> Result<(), Fault> {
  let in_head: Vec<&str> = paths
    .iter()
    .filter(|path| git(cwd, &["cat-file", "-e", &format!("HEAD:{path}")], &[]).ok)
    .map(String::as_str)
    .collect();

  if !in_head.is_empty() {
    let checkout: Vec<&str> =
      ["checkout", "HEAD", "--"].into_iter().chain(in_head.iter().copied()).collect();

    must(git(cwd, &checkout, &[]), GitStep::TakeBack)?;
  }

  for path in paths.iter().filter(|path| !in_head.contains(&path.as_str())) {
    let _ = git(cwd, &["rm", "-q", "--cached", "-f", "--ignore-unmatch", "--", path], &[]);
    let _ = std::fs::remove_file(Path::new(cwd).join(path));
  }

  Ok(())
}

/// A branch's change brought back into the working tree uncommitted, merged against the checkout's HEAD; the
/// files a conflict left marked are answered with, and a merge that cannot start at all refuses.
pub fn merge_back(cwd: &str, branch: &str) -> Result<Vec<String>, Fault> {
  if !git(cwd, &["rev-parse", "--verify", "-q", &format!("refs/heads/{branch}")], &[]).ok {
    return Err(format!("there is no branch {branch} to bring the change back from"));
  }

  let touched: Vec<String> = must(
    git(cwd, &["diff", "--name-only", &format!("{branch}^"), branch], &[]),
    GitStep::ReadBranch,
  )?
  .lines()
  .filter(|line| !line.is_empty())
  .map(str::to_owned)
  .collect();
  let merged = git(cwd, &["merge", "--squash", "--no-commit", branch], &[]);
  let conflicts: Vec<String> = git(cwd, &["diff", "--name-only", "--diff-filter=U"], &[])
    .out
    .lines()
    .filter(|line| !line.is_empty())
    .map(str::to_owned)
    .collect();

  if !merged.ok && conflicts.is_empty() {
    let said = if merged.err.is_empty() { merged.out } else { merged.err };

    return Err(format!("the change on {branch} could not be brought back: {}", said.trim()));
  }

  if !touched.is_empty() {
    let reset: Vec<&str> =
      ["reset", "-q", "--"].into_iter().chain(touched.iter().map(String::as_str)).collect();
    let _ = git(cwd, &reset, &[]);
  }

  let git_dir = git(cwd, &["rev-parse", "--absolute-git-dir"], &[]).out.trim().to_owned();
  let _ = std::fs::remove_file(Path::new(&git_dir).join("SQUASH_MSG"));

  Ok(conflicts)
}

/// Whether two orders work in the same checkout.
pub fn same_checkout(one: &Value, other: &Value) -> bool {
  let cwd = text(one, "cwd").unwrap_or_default();

  !cwd.is_empty() && resolved(cwd) == resolved(text(other, "cwd").unwrap_or_default())
}

// Where an order works on its item's branch: on a support worker, whose base is named; on a worker of its own
// standing in a linked worktree; and in a base checkout whose repository has support workers. None where it
// works on main.
struct BranchBase {
  cwd: String,
  own_worktree: bool,
}

// One check of a stopped order's change under way: the change it was started with, how many were started,
// and what it came to once it has.
struct Check {
  tree: String,
  starts: u32,
  passed: Arc<Mutex<Option<bool>>>,
  running: Option<tokio::task::JoinHandle<()>>,
}

/// The hold over the workers' checkouts, kept across the runner's ticks with the checks under way.
#[derive(Clone)]
pub struct Hold {
  checks: Arc<Mutex<HashMap<i64, Check>>>,
  checker: Arc<dyn RepoCheck>,
}

/// A change to the order's record of what became of its change, after what it already holds.
pub fn note(order: &mut Value, line: CheckoutNote) {
  let mut notes: Vec<Value> =
    order.get("checkoutNotes").and_then(Value::as_array).cloned().unwrap_or_default();

  notes.push(serde_json::to_value(line).unwrap_or(Value::Null));
  order["checkoutNotes"] = Value::Array(notes);
}

fn remove(order: &mut Value, keys: &[&str]) {
  if let Some(order) = order.as_object_mut() {
    for key in keys {
      order.remove(*key);
    }
  }
}

async fn save(ground: &Ground<'_>, order: &mut Value) -> Result<(), Fault> {
  queue_rows::work_orders::save(ground.db, order).await.map_err(faulted)
}

// What a stopped order holding its checkout waits on, written down once per change of it.
async fn wait_on(ground: &Ground<'_>, order: &mut Value, why: &ReleaseWait) -> Result<(), Fault> {
  let wait = serde_json::to_value(why).unwrap_or(Value::Null);

  if order.get("releaseWait") == Some(&wait) {
    return Ok(());
  }

  order["releaseWait"] = wait;
  save(ground, order).await
}

fn state_value(state: &WorktreeState) -> Value {
  serde_json::to_value(state).unwrap_or(Value::Null)
}

impl Hold {
  /// A hold whose stopped orders' changes are checked by `checker`.
  pub fn new(checker: Arc<dyn RepoCheck>) -> Self {
    Self {
      checks: Arc::default(),
      checker,
    }
  }

  /// Every check under way waited for: what a check that has to read their answers waits on.
  pub async fn settled(&self) {
    let running: Vec<tokio::task::JoinHandle<()>> =
      locked(&self.checks).values_mut().filter_map(|check| check.running.take()).collect();

    for check in running {
      let _ = check.await;
    }
  }

  /// What is still there of what ran in the order's checkout before — an open job of its worker, a unit of
  /// its ended jobs still alive, the checkout's job lock held — or none once the checkout is quiet.
  pub async fn checkout_busy(
    &self,
    ground: &Ground<'_>,
    order: &Value,
  ) -> Result<Option<ReleaseWait>, Fault> {
    let worker = text(order, "worker").unwrap_or_default();
    let open = one(
      ground.db,
      r#"SELECT "id" FROM "jobs" WHERE "finished_at" IS NULL AND COALESCE("worker", $1) = $2 ORDER BY "id" LIMIT 1"#,
      vec![BUILTIN_WORKER.into(), worker.into()],
    )
    .await
    .map_err(faulted)?;

    if let Some(open) = open {
      return Ok(Some(ReleaseWait::OpenJob {
        worker: worker.to_owned(),
        job: open.try_get("", "id").map_err(|why| why.to_string())?,
      }));
    }

    let since = now_ms() - ENDED_UNIT_WINDOW_MS;
    let ended: Vec<String> = all(
      ground.db,
      r#"SELECT "unit" FROM "jobs" WHERE "finished_at" >= $1 AND COALESCE("worker", $2) = $3 ORDER BY "id""#,
      vec![since.into(), BUILTIN_WORKER.into(), worker.into()],
    )
    .await
    .map_err(faulted)?
    .iter()
    .filter_map(|row| row.try_get::<String>("", "unit").ok())
    .collect();
    let live = if ended.is_empty() {
      HashSet::new()
    } else {
      ground.running().active(&ended).await.map_err(faulted)?
    };

    if !live.is_empty() {
      return Ok(Some(ended_unit_holder(ground, worker, &ended, &live).await?));
    }

    let cwd = text(order, "cwd").unwrap_or_default();

    Ok(lock_held(ground.host, cwd).then_some(ReleaseWait::Lock))
  }

  /// Whether the order may launch in its checkout now, taking the checkout where nobody holds it: an order
  /// that takes it brings back what it left on its own branch when it stopped, and from then on answers for
  /// what it opens against the tree as it stood just before.
  pub async fn take_checkout(&self, ground: &Ground<'_>, order: &mut Value) -> Result<bool, Fault> {
    let todo = number(order, "todoId").unwrap_or_default();

    if order.get("holdsCheckout").and_then(Value::as_bool) == Some(true) {
      locked(&self.checks).remove(&todo);

      if let Some(busy) = self.checkout_busy(ground, order).await? {
        wait_on(ground, order, &busy).await?;
        return Ok(false);
      }

      if order.get("releaseWait").is_some() {
        remove(order, &["releaseWait"]);
        save(ground, order).await?;
      }

      return Ok(true);
    }

    for other in orders_in(ground.db, &["active", "paused"]).await.map_err(faulted)? {
      if number(&other, "todoId") != Some(todo)
        && other.get("holdsCheckout").and_then(Value::as_bool) == Some(true)
        && same_checkout(&other, order)
      {
        return Ok(false);
      }
    }

    if self.checkout_busy(ground, order).await?.is_some() {
      return Ok(false);
    }

    let cwd = text(order, "cwd").unwrap_or_default().to_owned();
    let worker = text(order, "worker").unwrap_or_default().to_owned();
    let base = branch_base_of(ground, order).await?;

    if let Some(base) = &base {
      let main = Checkout::local(base.cwd.as_str()).main_branch();
      let message = ground
        .policy
        .fill(&ground.policy.hold_commits.taken, &[("item", &todo.to_string()), ("worker", &worker)]);
      let put = (|| {
        if base.own_worktree {
          Checkout::local(base.cwd.as_str()).ensure_support_checkout(&cwd, SETUP_RUNTIME)?;
        }

        Checkout::local(cwd.as_str())
          .prepare_branch(&item_branch(Some(todo), 0), &main, &message, SETUP_RUNTIME)
          .map(drop)
      })();

      if let Err(why) = put {
        let place = if base.own_worktree {
          format!("{worker}'s own worktree")
        } else {
          "the base checkout".to_owned()
        };

        return Err(format!("{place} could not be put on the item's branch: {why}"));
      }

      if base.own_worktree {
        remove(order, &["parked"]);
      }
    }

    let before = if base.as_ref().is_some_and(|base| base.own_worktree) {
      nothing_open()
    } else {
      worktree_state(&Checkout::local(cwd.as_str())).unwrap_or_else(|_| nothing_open())
    };

    if let Some(parked) = order.get("parked").cloned() {
      if text(&parked, "kind") == Some("branch") {
        let branch = text(&parked, "branch").unwrap_or_default();
        let conflicts = merge_back(&cwd, branch)
          .map_err(|why| format!("the parked change was not merged back: {why}"))?;

        if !conflicts.is_empty() {
          order["conflicts"] = json!(conflicts);
        }
      }

      remove(order, &["parked"]);
      order["worktreeBaseline"] = state_value(&before);
    }

    order["holdsCheckout"] = json!(true);
    order["checkoutBaseline"] = state_value(&before);
    order["checkoutTurns"] = json!([]);
    save(ground, order).await?;

    Ok(true)
  }

  /// A stopped or finished order lets go of the checkout it held: a stopped one first takes the change it
  /// opened there out of the working tree as a commit once nothing of it runs and the checkout is quiet.
  /// Answers whether the checkout was let go, which frees its worker for the queue.
  pub async fn release(&self, ground: &Ground<'_>, order: &mut Value) -> Result<bool, Fault> {
    if order.get("holdsCheckout").and_then(Value::as_bool) != Some(true) {
      return Ok(false);
    }

    let todo = number(order, "todoId").unwrap_or_default();
    let paused = text(order, "status") == Some("paused");

    if paused {
      // A stopped order is not advanced, so a turn of its own still going when it was stopped stays marked
      // running; whether it is still in the checkout is asked of its unit.
      let turns: Vec<String> = current_places(order)
        .into_iter()
        .map(|place| task_at(order, place))
        .filter(|task| text(task, "state") == Some("running") && !branch_task(task))
        .filter_map(|task| text(task, "unit").map(str::to_owned))
        .collect();
      let live: Vec<String> = if turns.is_empty() {
        vec![]
      } else {
        let alive = ground.running().active(&turns).await.map_err(faulted)?;

        turns.into_iter().filter(|unit| alive.contains(unit)).collect()
      };

      if !live.is_empty() {
        wait_on(ground, order, &ReleaseWait::OwnTurns { live }).await?;
        return Ok(false);
      }

      if let Some(busy) = self.checkout_busy(ground, order).await? {
        wait_on(ground, order, &busy).await?;
        return Ok(false);
      }

      if let Some(baseline) = order.get("checkoutBaseline").cloned()
        && order.get("parked").is_none()
        && !self.park(ground, order, &read_worktree_state(&baseline)).await?
      {
        return Ok(false);
      }
    }

    // A stopped order's parking has put a base checkout back on main already; any other ending does it here.
    if !paused {
      let baseline = order.get("checkoutBaseline").map_or_else(nothing_open, read_worktree_state);

      base_back_on_main(ground, order, &baseline).await?;
    }

    locked(&self.checks).remove(&todo);
    order["holdsCheckout"] = json!(false);
    remove(order, &["checkoutBaseline", "checkoutTurns", "releaseWait"]);

    // A close that waited for the checkout to be let go is finished now.
    if order.get("closing").is_some() {
      order["status"] = json!("done");
      remove(order, &["closing"]);
    }

    save(ground, order).await?;

    Ok(true)
  }

  // A stopped order's own change taken out of the working tree; whether it is done with — false while its
  // check is still running.
  async fn park(
    &self,
    ground: &Ground<'_>,
    order: &mut Value,
    baseline: &WorktreeState,
  ) -> Result<bool, Fault> {
    let todo = number(order, "todoId").unwrap_or_default();
    let cwd = text(order, "cwd").unwrap_or_default().to_owned();
    let base = branch_base_of(ground, order).await?;

    if base.as_ref().is_some_and(|base| base.own_worktree) {
      park_on_branch(ground, order, &cwd, todo);
      return Ok(true);
    }

    // Only an order the base checkout stands on the branch for; one that took it on main parks below.
    if base.is_some() && is_work_branch(Checkout::local(cwd.as_str()).current_branch().as_deref()) {
      base_back_on_main(ground, order, baseline).await?;
      return Ok(true);
    }

    let root = ground.order_root(todo);
    let windows = turn_windows(ground, order).await?;

    // No turn of its own ran in the checkout: nothing there is the order's, whoever wrote it.
    if windows.is_empty() {
      return Ok(true);
    }

    let change = own_change(&cwd, baseline, &windows)?;

    if !change.paths.is_empty() {
      let tree = change_mark(&cwd, &change.paths);
      // A check started by this very call is under way for it, whenever its task ends: the reading below would
      // otherwise race the task, and a check that fails at once would answer within the same call.
      let (starts, passed, unchecked, fresh) = {
        let mut checks = locked(&self.checks);
        let standing = checks
          .get(&todo)
          .filter(|check| check.tree == tree)
          .map(|check| (check.starts, check.passed.clone()));

        match standing {
          Some((starts, passed)) => (starts, passed, false, false),
          None => {
            let starts = checks.get(&todo).map_or(0, |check| check.starts) + 1;

            if starts > CHECK_RESTART_LIMIT + 1 {
              (starts, Arc::new(Mutex::new(Some(false))), true, false)
            } else {
              let passed = Arc::new(Mutex::new(None));
              let answer = passed.clone();
              let checking =
                self.checker.passes(PathBuf::from(&cwd), root.join(format!("check-{}.log", now_ms())));
              let running = tokio::spawn(async move {
                let came = checking.await;

                *locked(&answer) = Some(came);
              });

              checks.insert(
                todo,
                Check {
                  tree: tree.clone(),
                  starts,
                  passed: passed.clone(),
                  running: Some(running),
                },
              );
              (starts, passed, false, true)
            }
          }
        }
      };
      let passed = if fresh { None } else { *locked(&passed) };
      let Some(passed) = passed else {
        wait_on(
          ground,
          order,
          &ReleaseWait::Checking {
            count: count(change.paths.len()),
            starts,
          },
        )
        .await?;
        return Ok(false);
      };
      let at = now_ms();
      let commits = &ground.policy.hold_commits;
      let marks = [("item", todo.to_string()), ("count", change.paths.len().to_string())];
      let marks: Vec<(&str, &str)> = marks.iter().map(|(name, value)| (*name, value.as_str())).collect();

      if unchecked {
        note(
          order,
          CheckoutNote::Unsettled {
            tries: CHECK_RESTART_LIMIT + 1,
            item: todo,
          },
        );
      }

      let parked = if passed {
        commit_on_checkout(&cwd, &change.paths, &ground.policy.fill(&commits.passed, &marks)).map(
          |commit| {
            (
              json!({ "kind": "main", "at": at, "commit": commit, "paths": change.paths }),
              Some(commit),
            )
          },
        )
      } else {
        let branch = format!("wip/{todo}");
        let checked = if unchecked { &commits.unfinished } else { &commits.failed };
        let message = ground
          .policy
          .fill(&commits.kept_off, &[("item", &todo.to_string()), ("checked", checked)]);

        commit_on_branch(&cwd, &change.paths, &branch, &message, &root).map(|commit| {
          (
            json!({ "kind": "branch", "at": at, "branch": branch, "commit": commit, "paths": change.paths }),
            None,
          )
        })
      };

      match parked {
        Ok((parked, on_main)) => {
          order["parked"] = parked;

          if let Some(commit) = on_main
            && let Some(refused) = push(&cwd).await
          {
            note(
              order,
              CheckoutNote::Unpushed {
                commit: commit[..12.min(commit.len())].to_owned(),
                refused,
              },
            );
          }
        }
        Err(why) => note(order, CheckoutNote::Uncommitted { why: Some(why) }),
      }
    }

    if !change.left.is_empty() {
      note(order, CheckoutNote::Left { paths: change.left });
    }

    Ok(true)
  }
}

// Which of the worker's ended jobs still counts as running in its checkout, and what does the counting: a
// unit systemd still holds is named; a unit nothing holds but an open execution is that execution.
async fn ended_unit_holder(
  ground: &Ground<'_>,
  worker: &str,
  ended: &[String],
  live: &HashSet<String>,
) -> Result<ReleaseWait, Fault> {
  let mut held: Vec<(String, String, String)> = vec![];

  for execution in ground.units.open_executions().await.map_err(faulted)? {
    if execution.shared_pane {
      continue;
    }

    let owner = [execution.parent.as_deref(), execution.root.as_deref()]
      .into_iter()
      .flatten()
      .find(|owner| live.contains(*owner));

    if let Some(unit) = owner {
      let label = execution
        .details
        .get("work_key")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .or(execution.line.clone())
        .unwrap_or_default();

      held.push((unit.to_owned(), execution.id.clone(), label));
    }
  }

  let Some((unit, id, label)) = held.first().cloned() else {
    return Ok(ReleaseWait::EndedUnits {
      worker: worker.to_owned(),
      units: ended.iter().filter(|unit| live.contains(*unit)).cloned().collect(),
    });
  };

  Ok(ReleaseWait::EndedRecord {
    worker: worker.to_owned(),
    unit,
    id,
    label,
    more: count(held.len().saturating_sub(1)),
  })
}

// When the order's own turns were at work in its checkout since it took it, each widened by a second on both
// sides; a turn whose row is still open counts as at work until now. An order with no turns written down is
// read off its tasks: every turn in its checkout that has started.
async fn turn_windows(ground: &Ground<'_>, order: &Value) -> Result<Vec<TurnWindow>, Fault> {
  let turns: Vec<(String, i64)> = match order.get("checkoutTurns").and_then(Value::as_array) {
    Some(turns) => turns
      .iter()
      .filter_map(|turn| Some((text(turn, "unit")?.to_owned(), turn.get("from")?.as_i64()?)))
      .collect(),
    None => places(order)
      .into_iter()
      .map(|(_, task)| task)
      .filter(|task| !branch_task(task))
      .filter_map(|task| Some((text(task, "unit")?.to_owned(), task.get("startedAt")?.as_i64()?)))
      .collect(),
  };
  let mut windows = vec![];

  for (unit, from) in turns {
    let finished = one(
      ground.db,
      r#"SELECT "finished_at" FROM "jobs" WHERE "unit" = $1 ORDER BY "id" DESC LIMIT 1"#,
      vec![unit.into()],
    )
    .await
    .map_err(faulted)?
    .and_then(|row| row.try_get::<Option<i64>>("", "finished_at").ok().flatten());

    windows.push(TurnWindow {
      from: from - 1000,
      to: finished.map_or_else(now_ms, |at| at + 1000),
    });
  }

  Ok(windows)
}

async fn branch_base_of(ground: &Ground<'_>, order: &Value) -> Result<Option<BranchBase>, Fault> {
  let worker = text(order, "worker").unwrap_or_default();
  let cwd = text(order, "cwd").unwrap_or_default();
  let row = one(
    ground.db,
    r#"SELECT "name", "cwd", "support_of", "support_count" FROM "workers" WHERE "name" = $1"#,
    vec![worker.into()],
  )
  .await
  .map_err(faulted)?;
  let support_of: Option<String> = match &row {
    Some(row) => row.try_get("", "support_of").map_err(|why| why.to_string())?,
    None => None,
  };

  if let Some(base) = support_of.as_deref().filter(|base| !base.is_empty()) {
    let base_row = one(ground.db, r#"SELECT "cwd" FROM "workers" WHERE "name" = $1"#, vec![base.into()])
      .await
      .map_err(faulted)?;

    if let Some(base_row) = base_row {
      let base_cwd = base_row.try_get::<Option<String>>("", "cwd").ok().flatten();

      return Ok(Some(BranchBase {
        cwd: base_cwd.unwrap_or_else(|| cwd.to_owned()),
        own_worktree: true,
      }));
    }
  }

  let checkout = Checkout::local(cwd);

  if !checkout.is_checkout() {
    return Ok(None);
  }

  if let Some(main) = checkout.main_checkout() {
    return Ok(Some(BranchBase {
      cwd: main,
      own_worktree: true,
    }));
  }

  let support_count: i64 = match &row {
    Some(row) => row.try_get("", "support_count").map_err(|why| why.to_string())?,
    None => 0,
  };

  if !jobs_work_on_branches(support_of.as_deref(), support_count) {
    return Ok(None);
  }

  Ok(Some(BranchBase {
    cwd: cwd.to_owned(),
    own_worktree: false,
  }))
}

/// A base checkout the order put on its item's branch put back on main as the order lets it go: the change its
/// own turns wrote there committed onto the branch, what anybody else left open carried back as it stands, and
/// the installed units linked into it relinked through bmo-testbed once it stands on main.
pub async fn base_back_on_main(
  ground: &Ground<'_>,
  order: &mut Value,
  baseline: &WorktreeState,
) -> Result<(), Fault> {
  let Some(base) = branch_base_of(ground, order).await? else { return Ok(()) };
  let cwd = text(order, "cwd").unwrap_or_default().to_owned();
  let checkout = Checkout::local(cwd.as_str());

  if base.own_worktree || !is_work_branch(checkout.current_branch().as_deref()) {
    return Ok(());
  }

  let todo = number(order, "todoId").unwrap_or_default();
  let windows = turn_windows(ground, order).await?;
  let change = if windows.is_empty() {
    OwnChange {
      paths: vec![],
      left: vec![],
    }
  } else {
    own_change(&cwd, baseline, &windows)?
  };
  let main = checkout.main_branch();
  let message = ground.policy.fill(&ground.policy.hold_commits.released, &[("item", &todo.to_string())]);
  let back = checkout.restore_main(&main, &change.paths, &message);

  if back.on_main
    && let Err(failed) = ground.units.relink_units(Path::new(&cwd)).await
  {
    eprintln!("bmo-job: work order {todo}: the units were not relinked: {}", failed.error);
  }

  if let Some(parked) = &back.parked {
    note(
      order,
      CheckoutNote::Parked {
        count: count(parked.paths.len()),
        branch: parked.branch.clone(),
        commit: parked.commit[..12.min(parked.commit.len())].to_owned(),
        stopped: false,
      },
    );
  }

  if !back.on_main {
    note(
      order,
      CheckoutNote::NotOnMain {
        main,
        said: back.said.unwrap_or_default(),
      },
    );
  }

  if !change.left.is_empty() {
    note(order, CheckoutNote::Left { paths: change.left });
  }

  Ok(())
}

// A stopped order on a support worker parks by committing everything open in its worktree onto its item's
// branch: nothing else writes there, so all of it is the order's.
fn park_on_branch(ground: &Ground<'_>, order: &mut Value, cwd: &str, todo: i64) {
  let message = ground.policy.fill(&ground.policy.hold_commits.stopped, &[("item", &todo.to_string())]);

  match Checkout::local(cwd).park_open(&message) {
    Ok(Some(parked)) => note(
      order,
      CheckoutNote::Parked {
        count: count(parked.paths.len()),
        branch: parked.branch,
        commit: parked.commit[..12.min(parked.commit.len())].to_owned(),
        stopped: true,
      },
    ),
    Ok(None) => {}
    Err(why) => note(order, CheckoutNote::NotParked { why: Some(why) }),
  }
}
