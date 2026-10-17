//! The branch places a work order's branches run in (docs/queue.md, "Branch places"): the list's judgement of
//! which place still holds a provider's prefix, a place taken for a branch — warm first, then one with nothing
//! to lose, then a new one — laid out of the checkout's open change, its row kept once its result lands, what
//! stands in bmo-agency for the places an ended order worked in retired, and the places and pools nothing will
//! use again taken away. What is reused is a place, not a conversation: its path is part of every prefix
//! opened there. The places' root, a provider's cache lifetime and a place's prefix are bmo-agency's.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;

use job_agency::RouteAsk;
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::pool::{branch_task, role_key};
use super::{Fault, Ground, faulted};
use crate::queue::turn::orders_in;
use crate::queue::work_orders::{self as orders, places};
use crate::queue::workers::workers;
use crate::{all, now_ms, run};

/// How many places a pool keeps standing once free, for the prompt caches they hold. A stage as wide as its
/// plan still gets a place for every branch, and the ones past this are taken away once free.
pub const CAP: i64 = 8;

/// How long a place with nothing alive in it is left standing.
pub const KEEP_MS: i64 = 7 * 24 * 60 * 60 * 1000;

// The provider whose rows name the prefix they opened on: the one prefix a worker's place assembles itself.
const DIGESTED: &str = "claude-code";

// The provider whose rows keep the upstream that answered.
const ROUTED: &str = "openrouter";

/// One row of the branch places' list: what a place last opened on, for which provider and model, and when
/// it last answered, in milliseconds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Agent {
  pub slot: i64,
  pub mode: String,
  pub model: String,
  pub role: String,
  pub upstream: String,
  pub prefix_digest: String,
  pub last_reply_at: i64,
}

impl Agent {
  /// The upstream a routed model's cache belongs to, none for any other provider.
  pub fn routed_upstream(&self) -> Option<&str> {
    (self.mode == ROUTED && !self.upstream.is_empty()).then_some(self.upstream.as_str())
  }
}

/// The lifetimes a list's rows are judged on: a row's provider, model and upstream to how long a place's
/// prefix is worth counting on, none where the provider publishes none.
pub type Lifetimes = HashMap<(String, String, String), Option<i64>>;

fn lifetime(lifetimes: &Lifetimes, agent: &Agent) -> Option<i64> {
  lifetimes
    .get(&(
      agent.mode.clone(),
      agent.model.clone(),
      agent.routed_upstream().unwrap_or_default().to_owned(),
    ))
    .copied()
    .flatten()
}

/// Whether the clock still allows a row.
pub fn alive(agent: &Agent, now: i64, ttl: Option<i64>) -> bool {
  ttl.is_some_and(|ttl| now - agent.last_reply_at < ttl)
}

/// Whether a run opening in the place would hit what the row names: the clock, and for Claude Code the prefix
/// the place would open on now.
pub fn warm(agent: &Agent, now: i64, ttl: Option<i64>, digest_now: &str) -> bool {
  alive(agent, now, ttl) && (agent.prefix_digest.is_empty() || agent.prefix_digest == digest_now)
}

/// What a run leaves in the list, or nothing worth writing: no row for a provider with no published lifetime,
/// nor for a Claude Code run whose prefix could not be read.
pub fn record(
  slot: i64,
  (mode, model, role): (&str, &str, &str),
  upstream: Option<&str>,
  digest: Option<&str>,
  at: i64,
  ttl: Option<i64>,
) -> Option<Agent> {
  ttl?;

  let upstream = upstream.filter(|_| mode == ROUTED);
  let claude = mode == DIGESTED;
  let digest = digest.unwrap_or_default();

  if claude && digest.is_empty() {
    return None;
  }

  Some(Agent {
    slot,
    mode: mode.to_owned(),
    model: model.to_owned(),
    role: role.to_owned(),
    upstream: upstream.unwrap_or_default().to_owned(),
    prefix_digest: if claude { digest.to_owned() } else { String::new() },
    last_reply_at: at,
  })
}

/// The places a caller would use for so many tasks on one provider and model, and the rows to delete.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Choice {
  /// Places whose row for this provider and model still names a cache, so the prefix is a read.
  pub reuse: Vec<i64>,
  /// Places standing free whose prefix this provider no longer has, or never had for this pair.
  pub cold: Vec<i64>,
  /// Places that do not exist yet, by the numbers the pool would give them.
  pub opened: Vec<i64>,
  /// Rows the clock has passed, or whose prefix has moved.
  pub expired: Vec<Agent>,
}

/// The pool's answer for `want` tasks on one provider and model: the warm places, oldest answer first since
/// refreshing the one closest to going cold keeps more alive, then free ones with nothing to lose, then new
/// numbers no one stands on. A row is judged at `now` on the lifetime its provider publishes.
pub fn choose(
  agents: &[Agent],
  slots: &[i64],
  busy: &HashSet<i64>,
  want: usize,
  (mode, model): (&str, &str),
  (now, lifetimes): (i64, &Lifetimes),
  digest_for: impl Fn(i64) -> String,
) -> Choice {
  let mut expired: Vec<Agent> = agents
    .iter()
    .filter(|agent| !alive(agent, now, lifetime(lifetimes, agent)))
    .cloned()
    .collect();
  let mut free: Vec<i64> = slots.iter().copied().filter(|slot| !busy.contains(slot)).collect();

  free.sort_unstable();

  // The last row of a slot for this pair is the one read, as a map keyed by the slot keeps it.
  let mine = |slot: i64| {
    agents.iter().rfind(|agent| {
      agent.slot == slot
        && agent.mode == mode
        && agent.model == model
        && alive(agent, now, lifetime(lifetimes, agent))
    })
  };
  let mut warmed: Vec<Agent> = vec![];

  for slot in &free {
    let Some(agent) = mine(*slot) else { continue };

    if warm(agent, now, lifetime(lifetimes, agent), &digest_for(*slot)) {
      warmed.push(agent.clone());
    } else {
      expired.push(agent.clone());
    }
  }

  warmed.sort_by_key(|agent| agent.last_reply_at);

  let reuse: Vec<i64> = warmed.iter().take(want).map(|agent| agent.slot).collect();
  let warm_slots: HashSet<i64> = warmed.iter().map(|agent| agent.slot).collect();
  let cold: Vec<i64> = free
    .iter()
    .copied()
    .filter(|slot| !warm_slots.contains(slot))
    .take(want.saturating_sub(reuse.len()))
    .collect();
  let mut held: HashSet<i64> = slots.iter().chain(busy.iter()).copied().collect();
  let mut opened = vec![];
  let mut slot = 1;

  while opened.len() < want.saturating_sub(reuse.len() + cold.len()) {
    if held.insert(slot) {
      opened.push(slot);
    }

    slot += 1;
  }

  Choice {
    reuse,
    cold,
    opened,
    expired,
  }
}

/// The places worth taking away: free, with no row still naming a cache, and either past the number the
/// pool keeps or rested longer than it keeps one.
pub fn prunable(
  agents: &[Agent],
  slots: &[i64],
  busy: &HashSet<i64>,
  now: i64,
  lifetimes: &Lifetimes,
  (cap, keep_ms): (i64, i64),
) -> Vec<i64> {
  let mut going: Vec<i64> = slots
    .iter()
    .copied()
    .filter(|slot| !busy.contains(slot))
    .filter(|slot| {
      let rows: Vec<&Agent> = agents.iter().filter(|agent| agent.slot == *slot).collect();

      if rows.iter().any(|agent| alive(agent, now, lifetime(lifetimes, agent))) {
        return false;
      }

      if *slot > cap {
        return true;
      }

      let newest = rows.iter().map(|agent| agent.last_reply_at).max().unwrap_or(0);

      newest > 0 && now - newest >= keep_ms
    })
    .collect();

  going.sort_unstable();
  going
}

// The lifetimes the rows given are judged on, each asked of bmo-agency once.
async fn lifetimes_of(ground: &Ground<'_>, agents: &[Agent]) -> Result<Lifetimes, Fault> {
  let mut lifetimes = Lifetimes::new();

  for agent in agents {
    let key = (
      agent.mode.clone(),
      agent.model.clone(),
      agent.routed_upstream().unwrap_or_default().to_owned(),
    );

    if lifetimes.contains_key(&key) {
      continue;
    }

    let ttl = ground
      .units
      .place_cache(&agent.mode, &agent.model, agent.routed_upstream())
      .await
      .map_err(faulted)?;

    lifetimes.insert(key, ttl);
  }

  Ok(lifetimes)
}

/// The rows of one pool, oldest answer first.
pub async fn agents(ground: &Ground<'_>, pool: &str) -> Result<Vec<Agent>, Fault> {
  all(
    ground.db,
    r#"SELECT "slot", "mode", "model", "role", "upstream", "prefix_digest", "last_reply_at" FROM "branch_agents"
       WHERE "pool" = $1 ORDER BY "last_reply_at""#,
    vec![pool.into()],
  )
  .await
  .map_err(faulted)?
  .iter()
  .map(|row| {
    Ok(Agent {
      slot: row.try_get("", "slot")?,
      mode: row.try_get("", "mode")?,
      model: row.try_get("", "model")?,
      role: row.try_get("", "role")?,
      upstream: row.try_get("", "upstream")?,
      prefix_digest: row.try_get("", "prefix_digest")?,
      last_reply_at: row.try_get("", "last_reply_at")?,
    })
  })
  .collect::<Result<_, sea_orm::DbErr>>()
  .map_err(|why| why.to_string())
}

async fn set_agent(ground: &Ground<'_>, pool: &str, agent: &Agent) -> Result<(), Fault> {
  run(
    ground.db,
    r#"INSERT INTO "branch_agents" ("pool", "slot", "mode", "model", "role", "upstream", "prefix_digest", "last_reply_at")
       VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
       ON CONFLICT ("pool", "slot", "mode", "model", "upstream") DO UPDATE SET "role" = excluded."role",
         "prefix_digest" = excluded."prefix_digest", "last_reply_at" = excluded."last_reply_at""#,
    vec![
      pool.into(),
      agent.slot.into(),
      agent.mode.clone().into(),
      agent.model.clone().into(),
      agent.role.clone().into(),
      agent.upstream.clone().into(),
      agent.prefix_digest.clone().into(),
      agent.last_reply_at.into(),
    ],
  )
  .await
  .map(drop)
  .map_err(faulted)
}

async fn delete_agent(ground: &Ground<'_>, pool: &str, agent: &Agent) -> Result<(), Fault> {
  run(
    ground.db,
    r#"DELETE FROM "branch_agents" WHERE "pool" = $1 AND "slot" = $2 AND "mode" = $3 AND "model" = $4 AND "upstream" = $5"#,
    vec![
      pool.into(),
      agent.slot.into(),
      agent.mode.clone().into(),
      agent.model.clone().into(),
      agent.upstream.clone().into(),
    ],
  )
  .await
  .map(drop)
  .map_err(faulted)
}

async fn delete_agents(ground: &Ground<'_>, pool: &str, slot: i64) -> Result<(), Fault> {
  run(
    ground.db,
    r#"DELETE FROM "branch_agents" WHERE "pool" = $1 AND "slot" = $2"#,
    vec![pool.into(), slot.into()],
  )
  .await
  .map(drop)
  .map_err(faulted)
}

// A path with its `.` and `..` read away.
fn lexical(path: &Path) -> PathBuf {
  let mut kept = PathBuf::new();

  for part in path.components() {
    match part {
      Component::CurDir => {}
      Component::ParentDir => {
        kept.pop();
      }
      other => kept.push(other),
    }
  }

  kept
}

// The repository a checkout's commits live in, as an absolute path; none where it is no repository.
async fn common_dir(cwd: &Path) -> Option<String> {
  if !cwd.exists() {
    return None;
  }

  let output = tokio::process::Command::new("git")
    .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
    .current_dir(cwd)
    .stdin(Stdio::null())
    .stderr(Stdio::null())
    .output()
    .await
    .ok()?;

  if !output.status.success() {
    return None;
  }

  let path = String::from_utf8_lossy(&output.stdout).trim().to_owned();

  Some(std::fs::canonicalize(&path).map_or(path, |real| real.to_string_lossy().into_owned()))
}

/// Which pool a checkout's side work borrows places from: its repository, named for a reader by the
/// repository's own name and made unique by a digest of where its commits live; a directory that is no
/// repository is named by its own path the same way.
pub async fn pool_of(cwd: &str) -> String {
  let common = common_dir(Path::new(cwd)).await;
  let place = common.clone().unwrap_or_else(|| {
    let path = Path::new(cwd);

    lexical(&if path.is_absolute() {
      path.to_path_buf()
    } else {
      std::env::current_dir().unwrap_or_default().join(path)
    })
    .to_string_lossy()
    .into_owned()
  });
  let base = |path: &str| {
    Path::new(path)
      .file_name()
      .map(|name| name.to_string_lossy().into_owned())
      .unwrap_or_default()
  };
  let named = match &common {
    None => base(&place),
    Some(_) if base(&place) == ".git" => base(
      &Path::new(&place)
        .parent()
        .map(|parent| parent.to_string_lossy().into_owned())
        .unwrap_or_default(),
    ),
    Some(_) => {
      let name = base(&place);

      match name.strip_suffix(".git") {
        Some(stem) if !stem.is_empty() => stem.to_owned(),
        _ => name,
      }
    }
  };
  let named: String = named
    .chars()
    .map(|character| {
      if character.is_ascii_alphanumeric() || "._-".contains(character) {
        character
      } else {
        '_'
      }
    })
    .collect();
  let hash: String = Sha256::digest(place.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect();

  format!("{}-{}", if named.is_empty() { "repo" } else { &named }, &hash[..10])
}

/// A pool's places as bmo-agency stands them: the root every pool stands under, and each place's number.
pub async fn pool_places(ground: &Ground<'_>, pool: &str) -> Result<(PathBuf, Vec<i64>), Fault> {
  let slots = ground.units.branch_slots(pool).await.map_err(faulted)?;
  let mut numbers: Vec<i64> = slots.slots.iter().map(|slot| i64::from(slot.slot)).collect();

  numbers.sort_unstable();

  Ok((slots.root, numbers))
}

/// One place, by its pool and the number that names it, under the root every pool stands under.
pub fn slot_dir(root: &Path, pool: &str, slot: i64) -> PathBuf {
  root.join(pool).join(slot.to_string())
}

/// The places a task is sitting in, read off the job rows rather than a flag of their own, so a restart frees
/// nothing by hand: a place is taken exactly as long as the run holding it has not finished. The order in
/// hand counts with the stored ones, since a stage's launches hand out places one task after another; an
/// order of another repository holds numbers of another pool, and no pool counts every order's together.
pub async fn busy_slots(
  ground: &Ground<'_>,
  pool: Option<&str>,
  order: Option<&Value>,
) -> Result<HashSet<i64>, Fault> {
  let unfinished: HashSet<i64> =
    all(ground.db, r#"SELECT "id" FROM "jobs" WHERE "finished_at" IS NULL"#, vec![])
      .await
      .map_err(faulted)?
      .iter()
      .filter_map(|row| row.try_get::<i64>("", "id").ok())
      .collect();
  let todo = order.and_then(|order| orders::number(order, "todoId"));
  let mut held: Vec<Value> = orders_in(ground.db, &["active", "paused"])
    .await
    .map_err(faulted)?
    .into_iter()
    .filter(|stored| todo.is_none() || orders::number(stored, "todoId") != todo)
    .collect();

  held.extend(order.cloned());

  let mut busy = HashSet::new();

  for one in &held {
    let cwd = orders::text(one, "cwd").unwrap_or_default();

    if cwd.is_empty() {
      continue;
    }

    if let Some(pool) = pool
      && pool_of(cwd).await != pool
    {
      continue;
    }

    for (_, task) in places(one) {
      let Some(slot) = task.get("slot").and_then(Value::as_i64) else { continue };
      let running = orders::text(task, "state") == Some("running");

      if running || task.get("jobId").and_then(Value::as_i64).is_some_and(|job| unfinished.contains(&job)) {
        busy.insert(slot);
      }
    }
  }

  Ok(busy)
}

// Taking a place off the repository it is a worktree of, whichever that is, and off the disk; the
// registration is pruned rather than removed by name, since the directory is gone by then.
async fn detach(cwd: &Path) {
  let owner = if cwd.join(".git").exists() { common_dir(cwd).await } else { None };

  let _ = tokio::fs::remove_dir_all(cwd).await;

  if let Some(owner) = owner {
    let parent = cwd.parent().unwrap_or(cwd);
    let _ = git(&["--git-dir", &owner, "worktree", "prune"], parent, None).await;
  }
}

// A git command in a directory, failing with what git said.
async fn git(args: &[&str], cwd: &Path, input: Option<&[u8]>) -> Result<Vec<u8>, Fault> {
  let mut child = tokio::process::Command::new("git")
    .args(args)
    .current_dir(cwd)
    .stdin(if input.is_some() { Stdio::piped() } else { Stdio::null() })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .map_err(|why| format!("git: {why}"))?;

  if let (Some(input), Some(mut stdin)) = (input, child.stdin.take()) {
    use tokio::io::AsyncWriteExt;

    stdin.write_all(input).await.map_err(|why| format!("git: {why}"))?;
    drop(stdin);
  }

  let done = child.wait_with_output().await.map_err(|why| format!("git: {why}"))?;

  if done.status.success() {
    return Ok(done.stdout);
  }

  let err = String::from_utf8_lossy(&done.stderr).into_owned();
  let out = String::from_utf8_lossy(&done.stdout).into_owned();

  Err(format!("git: {}", if err.is_empty() { out } else { err }))
}

// The directories standing straight under a root, by name.
fn standing_names(root: &Path) -> Vec<String> {
  std::fs::read_dir(root)
    .map(|entries| {
      entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect()
    })
    .unwrap_or_default()
}

/// Places standing straight under the root by number, which belong to no pool and which nothing borrows:
/// each taken off its repository and removed with whatever rows name its number outside every pool.
pub async fn tidy_legacy(ground: &Ground<'_>, root: &Path) -> Result<Vec<i64>, Fault> {
  let numbered: Vec<i64> = standing_names(root)
    .into_iter()
    .filter(|name| !name.is_empty() && name.chars().all(|character| character.is_ascii_digit()))
    .filter_map(|name| name.parse().ok())
    .collect();

  if numbered.is_empty() {
    return Ok(vec![]);
  }

  let busy = busy_slots(ground, None, None).await?;
  let mut gone = vec![];

  for slot in numbered.into_iter().filter(|slot| !busy.contains(slot)) {
    detach(&root.join(slot.to_string())).await;
    delete_agents(ground, "", slot).await?;
    gone.push(slot);
  }

  Ok(gone)
}

/// What stands in bmo-agency for a place taken down; a place that refuses to go is left for the next pass,
/// since a stage must never fail over tidying.
pub async fn retire_place(ground: &Ground<'_>, pool: &str, slot: i64) {
  if let Err(failed) = ground.units.retire_place(pool, slot).await {
    eprintln!("bmo-job: place {pool}/{slot}: {}", failed.error);
  }
}

/// Pools nothing can reach any more — every pool but the ones the workers' directories name and the one the
/// caller is about to hand a place out of — taken away as a place of a live pool is. A place a task still
/// sits in keeps its pool standing until that task has ended.
pub async fn tidy_orphan_pools(
  ground: &Ground<'_>,
  root: &Path,
  keep: Option<&str>,
) -> Result<Vec<String>, Fault> {
  let mut live: HashSet<String> = HashSet::new();

  for worker in workers(ground.db, &ground.source).await.map_err(faulted)? {
    if !worker.cwd.is_empty() {
      live.insert(pool_of(&worker.cwd).await);
    }
  }

  live.extend(keep.map(str::to_owned));

  let mut pools = standing_names(root);

  pools.extend(
    all(ground.db, r#"SELECT DISTINCT "pool" FROM "branch_agents" ORDER BY "pool""#, vec![])
      .await
      .map_err(faulted)?
      .iter()
      .filter_map(|row| row.try_get::<String>("", "pool").ok()),
  );
  pools.sort();
  pools.dedup();

  // A name no pool is ever given — a number, or one that would climb out of the root — is not this sweep's.
  let orphans: Vec<String> = pools
    .into_iter()
    .filter(|pool| {
      !live.contains(pool)
        && !pool.is_empty()
        && pool
          .chars()
          .all(|character| character.is_ascii_alphanumeric() || "._-".contains(character))
        && !pool.chars().all(|character| character.is_ascii_digit())
        && pool != "."
        && pool != ".."
    })
    .collect();
  let mut gone = vec![];

  for pool in orphans {
    let busy = busy_slots(ground, Some(&pool), None).await?;
    let mut slots: Vec<i64> = standing_names(&root.join(&pool))
      .into_iter()
      .filter_map(|name| name.parse().ok())
      .collect();

    slots.extend(agents(ground, &pool).await?.iter().map(|agent| agent.slot));
    slots.sort_unstable();
    slots.dedup();

    let mut held = false;

    for slot in slots {
      if busy.contains(&slot) {
        held = true;
        continue;
      }

      detach(&slot_dir(root, &pool, slot)).await;
      delete_agents(ground, &pool, slot).await?;
      retire_place(ground, &pool, slot).await;
    }

    if held {
      continue;
    }

    let _ = tokio::fs::remove_dir_all(root.join(&pool)).await;
    gone.push(pool);
  }

  Ok(gone)
}

/// The places an order that has ended worked in and nobody sits in now; the places themselves stay, only
/// what stands in bmo-agency for them goes.
pub async fn places_to_retire(
  ground: &Ground<'_>,
  order: &Value,
) -> Result<Option<(String, Vec<i64>)>, Fault> {
  let cwd = orders::text(order, "cwd").unwrap_or_default();

  if cwd.is_empty() {
    return Ok(None);
  }

  let pool = pool_of(cwd).await;
  let busy = busy_slots(ground, Some(&pool), None).await?;
  let mut used: Vec<i64> = places(order)
    .iter()
    .filter_map(|(_, task)| task.get("slot").and_then(Value::as_i64))
    .filter(|slot| !busy.contains(slot))
    .collect();

  used.sort_unstable();
  used.dedup();

  Ok(Some((pool, used)))
}

/// What stands in bmo-agency for the places a done or stopped order worked in taken down.
pub async fn retire_places(ground: &Ground<'_>, order: &Value) -> Result<(), Fault> {
  if orders::text(order, "status") == Some("active") {
    return Ok(());
  }

  if let Some((pool, slots)) = places_to_retire(ground, order).await? {
    for slot in slots {
      retire_place(ground, &pool, slot).await;
    }
  }

  Ok(())
}

// Places nothing uses and nothing keeps, taken away; one that refuses to go is left for the next time.
async fn tidy_slots(
  ground: &Ground<'_>,
  order: &Value,
  (root, pool): (&Path, &str),
  slots: &[i64],
  busy: &HashSet<i64>,
  now: i64,
) -> Result<Vec<i64>, Fault> {
  let cwd = orders::text(order, "cwd").unwrap_or_default();
  let rows = agents(ground, pool).await?;
  let lifetimes = lifetimes_of(ground, &rows).await?;
  let going = prunable(&rows, slots, busy, now, &lifetimes, (CAP, KEEP_MS));

  for slot in &going {
    let place = slot_dir(root, pool, *slot);
    let place_text = place.to_string_lossy().into_owned();

    if git(&["worktree", "remove", "--force", &place_text], Path::new(cwd), None)
      .await
      .is_err()
    {
      detach(&place).await;
    }

    delete_agents(ground, pool, *slot).await?;
    retire_place(ground, pool, *slot).await;
  }

  Ok(going)
}

// The effort a role's run goes out at on a provider and tier, where its route names one.
async fn effort_of(ground: &Ground<'_>, role: &str, mode: &str, tier: Option<&str>) -> Option<String> {
  let asked = RouteAsk {
    work_key: role_key(role),
    provider: Some(mode.to_owned()),
    tier: tier.unwrap_or_default().to_owned(),
  };

  ground.units.resolve_route(&asked).await.ok().flatten().and_then(|found| found.effort)
}

// What a place would open on now for a model and effort, or nothing where bmo-agency cannot say.
async fn digest_of(ground: &Ground<'_>, cwd: &Path, model: &str, effort: Option<&str>) -> Option<String> {
  match ground.units.prefix_digest(&cwd.to_string_lossy(), model, effort).await {
    Ok(digest) => Some(digest),
    Err(failed) => {
      eprintln!("bmo-job: branch prefix digest: {}", failed.error);
      None
    }
  }
}

/// Which place a branch task works in: a free place whose row for its provider and model still names a cache,
/// then a free one with nothing to lose, then a new one; rows the clock or a moved prefix has finished with
/// deleted on the way past, and the pools and places nothing will use taken away first.
pub async fn take_slot(ground: &Ground<'_>, order: &Value, task: &Value, now: i64) -> Result<i64, Fault> {
  let selection = orders::text(task, "model").unwrap_or_default();
  let (mode, model) = selection.split_once("::").unwrap_or((DIGESTED, selection));
  let cwd = orders::text(order, "cwd").unwrap_or_default();
  let pool = pool_of(cwd).await;
  let (root, standing) = pool_places(ground, &pool).await?;

  tidy_legacy(ground, &root).await?;
  tidy_orphan_pools(ground, &root, Some(&pool)).await?;

  let busy = busy_slots(ground, Some(&pool), Some(order)).await?;
  let gone: HashSet<i64> = tidy_slots(ground, order, (&root, &pool), &standing, &busy, now)
    .await?
    .into_iter()
    .collect();
  let slots: Vec<i64> = standing.into_iter().filter(|slot| !gone.contains(slot)).collect();
  let rows = agents(ground, &pool).await?;
  let lifetimes = lifetimes_of(ground, &rows).await?;
  let role = orders::text(task, "role").unwrap_or_default();
  let mut digests: Vec<(i64, String)> = vec![];

  for agent in &rows {
    if agent.mode != mode
      || agent.model != model
      || busy.contains(&agent.slot)
      || !slots.contains(&agent.slot)
    {
      continue;
    }

    let effort = effort_of(ground, role, mode, orders::text(task, "tier")).await;
    let digest = digest_of(ground, &slot_dir(&root, &pool, agent.slot), model, effort.as_deref()).await;

    digests.retain(|(slot, _)| *slot != agent.slot);
    digests.push((agent.slot, digest.unwrap_or_default()));
  }

  let decided = choose(&rows, &slots, &busy, 1, (mode, model), (now, &lifetimes), |slot| {
    digests
      .iter()
      .find(|(held, _)| *held == slot)
      .map(|(_, digest)| digest.clone())
      .unwrap_or_default()
  });

  for dead in &decided.expired {
    delete_agent(ground, &pool, dead).await?;
  }

  decided
    .reuse
    .first()
    .or(decided.cold.first())
    .or(decided.opened.first())
    .copied()
    .ok_or_else(|| "no branch place could be taken for the co-worker".to_owned())
}

/// The place a branch works in, for the order it is a task of.
pub async fn slot_cwd(ground: &Ground<'_>, order: &Value, slot: i64) -> Result<PathBuf, Fault> {
  let pool = pool_of(orders::text(order, "cwd").unwrap_or_default()).await;
  let (root, _) = pool_places(ground, &pool).await?;

  Ok(slot_dir(&root, &pool, slot))
}

/// A place brought back to the shape a task starts in: rewound to the checkout's commit — or made again where
/// it is no worktree of that repository — with the checkout's open change laid on top, tracked and new files
/// alike, so the branch reads the tree the item is being done in. Ignored files stay with the place.
pub async fn prepare_slot(source: &str, cwd: &Path) -> Result<(), Fault> {
  let source = Path::new(source);
  let head = git(&["rev-parse", "HEAD"], source, None)
    .await
    .map_err(|_| "the checkout's commit could not be read".to_owned())?;
  let commit = String::from_utf8_lossy(&head).trim().to_owned();

  rewind(source, &commit, cwd).await?;

  let diff = git(&["diff", "HEAD", "--binary"], source, None)
    .await
    .map_err(|_| "the checkout's open change could not be read".to_owned())?;

  if !diff.is_empty() {
    git(&["apply", "--binary", "-"], cwd, Some(&diff)).await?;
  }

  let extra = git(&["ls-files", "--others", "--exclude-standard", "-z"], source, None)
    .await
    .map_err(|_| "the checkout's new files could not be listed".to_owned())?;

  for path in String::from_utf8_lossy(&extra).split('\0').filter(|path| !path.is_empty()) {
    let from = source.join(path);

    if !tokio::fs::symlink_metadata(&from).await.is_ok_and(|meta| meta.is_file()) {
      continue;
    }

    let to = cwd.join(path);

    if let Some(parent) = to.parent() {
      tokio::fs::create_dir_all(parent).await.map_err(|why| why.to_string())?;
    }

    tokio::fs::copy(&from, &to).await.map_err(|why| why.to_string())?;
  }

  Ok(())
}

// A place brought to one commit of the repository `source` is a checkout of: rewound where it already is a
// worktree of that repository, made again where it is not.
async fn rewind(source: &Path, commit: &str, cwd: &Path) -> Result<(), Fault> {
  let repository = common_dir(source).await;
  let standing = cwd.join(".git").exists();

  if standing && repository.is_some() && common_dir(cwd).await == repository {
    git(&["reset", "--hard", "-q"], cwd, None).await?;
    git(&["clean", "-fdq"], cwd, None).await?;
    git(&["checkout", "-q", "--detach", commit], cwd, None).await?;
    return bring_submodules(cwd).await;
  }

  if standing {
    detach(cwd).await;
  }

  // A place registered but gone, or one never made: whatever stands in the way of the checkout is what a
  // failed attempt left behind.
  if let Some(parent) = cwd.parent() {
    tokio::fs::create_dir_all(parent).await.map_err(|why| why.to_string())?;
  }

  git(&["worktree", "prune"], source, None).await?;

  let place = cwd.to_string_lossy().into_owned();

  if git(&["worktree", "add", "--detach", &place, commit], source, None).await.is_err() {
    let _ = tokio::fs::remove_dir_all(cwd).await;
    git(&["worktree", "add", "--detach", &place, commit], source, None).await?;
  }

  bring_submodules(cwd).await
}

// A place's submodules forced onto the commit's pins, as the place itself is.
async fn bring_submodules(cwd: &Path) -> Result<(), Fault> {
  let place = crate::job_run::checkout::Checkout::local(cwd.to_string_lossy());

  tokio::task::spawn_blocking(move || place.bring_submodules(true))
    .await
    .map_err(|why| why.to_string())?
}

/// What a finished branch leaves in the list: the place it worked in, the provider and model that answered
/// there with the upstream that took it, the role, and the prefix the place now opens on — or nothing where
/// no cache can be named.
pub async fn remember_slot(
  ground: &Ground<'_>,
  order: &Value,
  task: &Value,
  (mode, model, effort, upstream): (Option<&str>, Option<&str>, Option<&str>, Option<&str>),
) -> Result<(), Fault> {
  let Some(slot) = task.get("slot").and_then(Value::as_i64) else { return Ok(()) };

  if !branch_task(task) {
    return Ok(());
  }

  let (Some(mode), Some(model)) = (mode, model) else { return Ok(()) };

  if !ground
    .units
    .providers()
    .await
    .map_err(faulted)?
    .iter()
    .any(|provider| provider.name == mode)
  {
    return Ok(());
  }

  let cwd = slot_cwd(ground, order, slot).await?;
  let digest = if mode == DIGESTED { digest_of(ground, &cwd, model, effort).await } else { None };
  let routed = upstream.filter(|_| mode == ROUTED);
  let ttl = ground.units.place_cache(mode, model, routed).await.map_err(faulted)?;
  let role = orders::text(task, "role").unwrap_or_default();
  let Some(row) = record(slot, (mode, model, role), upstream, digest.as_deref(), now_ms(), ttl) else {
    return Ok(());
  };
  let pool = pool_of(orders::text(order, "cwd").unwrap_or_default()).await;

  set_agent(ground, &pool, &row).await
}
