//! What a job worker may be, judged before anything about it is written (docs/queue.md, "Workers"): its
//! name, its label, the directory it works in, its preamble and its dev server's command, its support
//! workers, and what may be changed or removed of one already standing. Each refusal is a code of
//! `AgentRefusal`.

use std::path::Path;

use job_codes::{AgentRefusal, JobError};
use job_policy::workers::WorkerRules;
use regex::Regex;

use super::workers::{Changes, Worker, support_name};
use crate::Refusal;

/// The policy's worker rules with the name pattern built once.
pub struct Judge<'a> {
  rules: &'a WorkerRules,
  name: Regex,
}

/// A worker to be added, as the request names it.
pub struct Adding<'a> {
  pub name: &'a str,
  pub label: &'a str,
  pub cwd: &'a str,
  /// Whether its directory is one to be made for it.
  pub made: bool,
  pub preamble: Option<&'a str>,
  pub dev_command: Option<&'a str>,
}

/// What a change asks beyond a worker's row.
#[derive(Clone, Copy, Debug, Default)]
pub struct Beyond {
  /// Its pin or its keepalive, kept by bmo-agency, which a support worker takes from its base.
  pub runtime: bool,
}

fn refused(refusal: AgentRefusal) -> Refusal {
  JobError::from(refusal).into()
}

fn length(text: &str) -> usize {
  text.chars().count()
}

fn most(bound: usize) -> u32 {
  u32::try_from(bound).unwrap_or(u32::MAX)
}

impl<'a> Judge<'a> {
  /// The judge of these rules, or why their name pattern does not build.
  pub fn new(rules: &'a WorkerRules) -> Result<Self, regex::Error> {
    Ok(Self {
      rules,
      name: Regex::new(&rules.name_pattern)?,
    })
  }

  /// Why a name cannot be taken: the names taken are every worker's, resting ones included.
  pub fn name_fault(&self, name: &str, taken: &[String]) -> Result<(), Refusal> {
    if !self.name.is_match(name) {
      return Err(refused(AgentRefusal::WorkerNameForm {
        name: name.to_owned(),
      }));
    }

    if taken.iter().any(|one| one == name) {
      return Err(refused(AgentRefusal::AgentNameTaken {
        name: name.to_owned(),
      }));
    }

    Ok(())
  }

  fn label_fault(&self, label: &str) -> Result<(), Refusal> {
    if label.trim().is_empty() {
      return Err(refused(AgentRefusal::LabelEmpty));
    }

    if length(label) > self.rules.label_most {
      return Err(refused(AgentRefusal::LabelLong {
        most: most(self.rules.label_most),
      }));
    }

    Ok(())
  }

  // The two workbench fields: a preamble bounded the way a note is, and a dev command of one line.
  fn bench_fault(&self, preamble: Option<&str>, dev_command: Option<&str>) -> Result<(), Refusal> {
    if preamble.is_some_and(|preamble| length(preamble) > self.rules.preamble_most) {
      return Err(refused(AgentRefusal::PreambleLong {
        most: most(self.rules.preamble_most),
      }));
    }

    let Some(command) = dev_command else {
      return Ok(());
    };

    if command.contains(['\r', '\n']) {
      return Err(refused(AgentRefusal::DevCommandLines));
    }

    if length(command) > self.rules.dev_command_most {
      return Err(refused(AgentRefusal::DevCommandLong {
        most: most(self.rules.dev_command_most),
      }));
    }

    Ok(())
  }

  /// What is wrong with adding a worker. A directory still to be made is judged only on what stands in
  /// its way.
  pub fn new_worker_fault(&self, worker: &Adding<'_>, taken: &[String]) -> Result<(), Refusal> {
    self.name_fault(worker.name, taken)?;
    self.label_fault(worker.label)?;

    if worker.made {
      made_directory_fault(worker.cwd)?;
    } else {
      path_fault(worker.cwd)?;
    }

    self.bench_fault(worker.preamble, worker.dev_command)
  }

  /// What is wrong with changing one. A support worker's settings follow from its base, and the built-in
  /// worker's directory is not the owner's to move.
  pub fn change_fault(&self, target: &Worker, changes: &Changes, beyond: Beyond) -> Result<(), Refusal> {
    if let Some(base) = target.support_of.as_deref()
      && (changes.any() || beyond.runtime)
    {
      return Err(refused(AgentRefusal::SupportChangedByBase {
        worker: target.name.clone(),
        base: base.to_owned(),
      }));
    }

    if changes.cwd.is_some() && target.builtin {
      return Err(refused(AgentRefusal::BuiltinPathFixed {
        worker: target.name.clone(),
      }));
    }

    self.bench_fault(
      changes.preamble.as_ref().and_then(Option::as_deref),
      changes.dev_command.as_ref().and_then(Option::as_deref),
    )?;

    if let Some(label) = changes.label.as_deref() {
      self.label_fault(label)?;
    }

    match changes.cwd.as_deref() {
      Some(cwd) => path_fault(cwd),
      None => Ok(()),
    }
  }

  /// What is wrong with a base's support settings, judged as the pair they make.
  pub fn support_fault(&self, base: &Worker, count: f64, keepalive: f64) -> Result<(), Refusal> {
    if let Some(of) = base.support_of.as_deref() {
      return Err(refused(AgentRefusal::SupportHasNoSettings {
        worker: base.name.clone(),
        base: of.to_owned(),
      }));
    }

    let support_most = self.rules.support_most;

    if count.fract() != 0.0 || count < 0.0 || count > support_most as f64 {
      return Err(refused(AgentRefusal::SupportCountRange {
        most: u32::try_from(support_most).unwrap_or(u32::MAX),
      }));
    }

    if keepalive.fract() != 0.0 || keepalive < 0.0 {
      return Err(refused(AgentRefusal::KeepaliveCountForm));
    }

    // Both are whole numbers within the most by now.
    let (count, keepalive) = (count as u32, keepalive as u32);

    if keepalive > count {
      return Err(refused(AgentRefusal::KeepaliveOverCount { keepalive, count }));
    }

    let last = support_name(&base.name, i64::from(count) + self.rules.first_support_slot - 1);

    if count > 0 && !self.name.is_match(&last) {
      return Err(refused(AgentRefusal::SupportNameLong {
        base: base.name.clone(),
        name: last,
      }));
    }

    Ok(())
  }
}

/// Why a directory cannot be worked in. It has to be a directory that is there; whether it is a checkout
/// decides what the worker's jobs may do, not whether it may be one.
fn path_fault(path: &str) -> Result<(), Refusal> {
  if path.trim().is_empty() {
    return Err(refused(AgentRefusal::PathEmpty));
  }

  let at = Path::new(path);
  let path = path.to_owned();

  if !at.is_absolute() {
    return Err(refused(AgentRefusal::PathNotAbsolute { path }));
  }

  if !at.exists() {
    return Err(refused(AgentRefusal::PathAbsent { path }));
  }

  if !at.is_dir() {
    return Err(refused(AgentRefusal::PathNotDirectory { path }));
  }

  Ok(())
}

// What stands in the way of the directory made for a worker added with none.
fn made_directory_fault(path: &str) -> Result<(), Refusal> {
  let at = Path::new(path);

  if at.exists() && !at.is_dir() {
    return Err(refused(AgentRefusal::MadePathBlocked {
      path: path.to_owned(),
    }));
  }

  Ok(())
}

/// Why a worker cannot be removed.
pub fn remove_fault(target: Option<&Worker>, asked: &str) -> Result<(), Refusal> {
  let Some(target) = target else {
    return Err(refused(AgentRefusal::NoWorker {
      worker: asked.to_owned(),
    }));
  };

  if let Some(base) = target.support_of.as_deref() {
    return Err(refused(AgentRefusal::SupportRemovedByCount {
      worker: target.name.clone(),
      base: base.to_owned(),
    }));
  }

  if target.builtin {
    return Err(refused(AgentRefusal::BuiltinNotRemoved {
      worker: target.name.clone(),
    }));
  }

  Ok(())
}
