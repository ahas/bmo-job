//! What a checkout holds open, and the baseline a job is measured against (docs/queue.md, "A checkout
//! held"). A checkout is shared — several jobs and a person work in the same one, and any of them may have
//! left work open — so what is a job's own is what is open now that was not open as it began. The baseline
//! is left where anything else running in the checkout reads it, the order runner carrying a work order's own
//! among them, so its file keeps the one shape every reader takes.

use std::collections::HashMap;
use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::checkout::{Checkout, keyed_path, resolved, status_entries};

/// What is open in a checkout: each tracked path with a digest of its change, the untracked files by
/// name, and each tracked path's changed lines as short digests where it has lines to read.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorktreeState {
  pub tracked: IndexMap<String, String>,
  pub untracked: Vec<String>,
  #[serde(default, skip_serializing_if = "Option::is_none")]
  pub lines: Option<IndexMap<String, Vec<String>>>,
}

/// Open paths of two kinds.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Work {
  pub tracked: Vec<String>,
  pub untracked: Vec<String>,
}

impl Work {
  /// Every path in it, tracked and untracked alike, as a person reads a list.
  pub fn all(&self) -> Vec<String> {
    self.tracked.iter().chain(&self.untracked).cloned().collect()
  }
}

/// A checkout with nothing open in it, and what an unreadable one answers.
pub fn nothing_open() -> WorktreeState {
  WorktreeState {
    tracked: IndexMap::new(),
    untracked: Vec::new(),
    lines: Some(IndexMap::new()),
  }
}

fn hex(bytes: &[u8]) -> String {
  bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

// What a patch says, apart from where in the file it happens to say it. A hunk header counts lines from
// the top of the file and the index line names the blob the patch is against, so both move when
// anything else in that file is committed; the added, removed and context lines move with nothing.
fn patch_words(diff: &str) -> String {
  diff
    .split('\n')
    .filter(|line| !line.starts_with("@@") && !line.starts_with("index "))
    .collect::<Vec<_>>()
    .join("\n")
}

// The lines a patch adds and removes, each as a short digest of where it is open, its sign and its
// text. The file and hunk headers are not the change, and neither are the context lines.
fn changed_lines(place: &str, diff: &str) -> Vec<String> {
  diff
    .split('\n')
    .filter(|line| {
      (line.starts_with('+') || line.starts_with('-'))
        && !line.starts_with("+++ ")
        && !line.starts_with("--- ")
    })
    .map(|line| {
      let mut digest = Sha256::new();

      digest.update(place.as_bytes());
      digest.update(line.as_bytes());
      hex(&digest.finalize())[..16].to_owned()
    })
    .collect()
}

/// What is open in a checkout now. A directory that is no checkout answers nothing; a change git would
/// not show is a fault.
pub fn worktree_state(checkout: &Checkout) -> Result<WorktreeState, String> {
  // Every untracked file by name rather than the directory holding them, so a file added inside
  // somebody else's untracked directory is still told apart.
  let status = checkout.run(&["git", "status", "--porcelain=v1", "-z", "--untracked-files=all"], &[], None);

  if status.code != Some(0) {
    return Ok(nothing_open());
  }

  let mut state = WorktreeState {
    lines: Some(IndexMap::new()),
    ..WorktreeState::default()
  };

  for (code, path) in status_entries(&String::from_utf8_lossy(&status.stdout)) {
    if code == "??" {
      state.untracked.push(path);
      continue;
    }

    let unstaged = checkout.run(&["git", "diff", "--binary", "--", &path], &[], None);
    let staged = checkout.run(&["git", "diff", "--cached", "--binary", "--", &path], &[], None);

    if unstaged.code != Some(0) || staged.code != Some(0) {
      return Err("the open change in the checkout could not be read".to_owned());
    }

    let unstaged = String::from_utf8_lossy(&unstaged.stdout).into_owned();
    let staged = String::from_utf8_lossy(&staged.stdout).into_owned();
    let mut digest = Sha256::new();

    digest.update(code.as_bytes());
    digest.update(patch_words(&unstaged).as_bytes());
    digest.update(patch_words(&staged).as_bytes());
    state.tracked.insert(path.clone(), hex(&digest.finalize()));

    // Where a line is open counts as much as what it says. A binary patch has no lines to read, so a
    // binary file is compared by digest alone.
    if !staged.contains("GIT binary patch") && !unstaged.contains("GIT binary patch") {
      let lines = changed_lines("i", &staged).into_iter().chain(changed_lines("w", &unstaged)).collect();

      if let Some(all) = state.lines.as_mut() {
        all.insert(path, lines);
      }
    }
  }

  Ok(state)
}

// Whether every changed line now open in a file was already open then, counted with repeats.
fn lines_held(before: &[String], after: &[String]) -> bool {
  let mut held: HashMap<&str, usize> = HashMap::new();

  for line in before {
    *held.entry(line).or_default() += 1;
  }
  for line in after {
    match held.get_mut(line.as_str()) {
      Some(left) if *left > 0 => *left -= 1,
      _ => return false,
    }
  }

  true
}

// Whether a path open now carries nothing that was not open then.
fn preserved(before: &WorktreeState, after: &WorktreeState, path: &str) -> bool {
  let Some(then_digest) = before.tracked.get(path) else {
    return false;
  };
  let then = before.lines.as_ref().and_then(|lines| lines.get(path));
  let now = after.lines.as_ref().and_then(|lines| lines.get(path));

  // A change with no lines in it — a mode, an emptied file — says nothing a line could, so the digest
  // answers for it.
  if let (Some(then), Some(now)) = (then, now)
    && !now.is_empty()
  {
    return lines_held(then, now);
  }

  after.tracked.get(path) == Some(then_digest)
}

/// What is open now that was not open then: the current actor's own unclosed work.
pub fn new_work(before: &WorktreeState, after: &WorktreeState) -> Work {
  Work {
    tracked: after.tracked.keys().filter(|path| !preserved(before, after, path)).cloned().collect(),
    untracked: after
      .untracked
      .iter()
      .filter(|path| !before.untracked.contains(path))
      .cloned()
      .collect(),
  }
}

/// What was open then and is untouched since: somebody else's work, preserved.
pub fn preserved_work(before: &WorktreeState, after: &WorktreeState) -> Work {
  Work {
    tracked: after.tracked.keys().filter(|path| preserved(before, after, path)).cloned().collect(),
    untracked: after.untracked.iter().filter(|path| before.untracked.contains(path)).cloned().collect(),
  }
}

/// A recorded baseline as whatever wrote it left it. A work order carries its own across its whole run,
/// and one recorded before untracked files were counted is read as a baseline with no untracked file in
/// it rather than as no baseline at all.
pub fn read_worktree_state(value: &Value) -> WorktreeState {
  let Some(held) = value.as_object() else {
    return nothing_open();
  };
  let strings = |value: &Value| -> IndexMap<String, String> {
    value
      .as_object()
      .map(|all| {
        all
          .iter()
          .filter_map(|(path, digest)| digest.as_str().map(|digest| (path.clone(), digest.to_owned())))
          .collect()
      })
      .unwrap_or_default()
  };

  match held.get("tracked") {
    Some(tracked) if tracked.is_object() => WorktreeState {
      tracked: strings(tracked),
      untracked: held
        .get("untracked")
        .and_then(Value::as_array)
        .map(|all| all.iter().filter_map(|path| path.as_str().map(str::to_owned)).collect())
        .unwrap_or_default(),
      lines: held.get("lines").and_then(Value::as_object).map(|all| {
        all
          .iter()
          .map(|(path, lines)| {
            let lines = lines
              .as_array()
              .map(|lines| lines.iter().filter_map(|line| line.as_str().map(str::to_owned)).collect())
              .unwrap_or_default();

            (path.clone(), lines)
          })
          .collect()
      }),
    },
    _ => WorktreeState {
      tracked: strings(value),
      untracked: Vec::new(),
      lines: None,
    },
  }
}

// Where a checkout's baseline is left for anything else running in it to read.
fn baseline_path(data_root: &Path, cwd: &str) -> std::path::PathBuf {
  keyed_path(data_root, cwd, "")
}

/// Leaves the baseline where a deployment run from inside this job can read it. A failure to write it
/// is no reason to fail the job: what follows then reads no baseline and holds the actor to the whole
/// tree, which is the stricter answer.
pub fn record_baseline(data_root: &Path, cwd: &str, state: &WorktreeState, now_ms: i64) {
  use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};

  let path = baseline_path(data_root, cwd);
  let text = json!({ "cwd": resolved(cwd), "at": now_ms, "state": state }).to_string();
  let _ = std::fs::DirBuilder::new()
    .recursive(true)
    .mode(0o700)
    .create(data_root.join("worktree"));
  let _ = std::fs::OpenOptions::new()
    .write(true)
    .create(true)
    .truncate(true)
    .mode(0o600)
    .open(&path)
    .and_then(|mut file| std::io::Write::write_all(&mut file, text.as_bytes()));
}

/// Takes it away, so a deployment run when no job holds the checkout reads none.
pub fn clear_baseline(data_root: &Path, cwd: &str) {
  let _ = std::fs::remove_file(baseline_path(data_root, cwd));
}

/// The baseline recorded for this checkout, or a checkout with nothing open. A reading for some other
/// checkout is no reading: the digest names the path, and a file that disagrees with its own name is
/// not trusted.
pub fn recorded_baseline(data_root: &Path, cwd: &str) -> WorktreeState {
  let Some(held) = std::fs::read_to_string(baseline_path(data_root, cwd))
    .ok()
    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
  else {
    return nothing_open();
  };

  if held["cwd"].as_str() != Some(resolved(cwd).as_str()) {
    return nothing_open();
  }

  read_worktree_state(&held["state"])
}
