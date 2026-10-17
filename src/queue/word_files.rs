//! Files the owner hands a worker with a word from the console (docs/api.md, "A worker's word"). A worker
//! reads files, not uploads, so each one is written where the worker keeps what outlives a session — its
//! workbench, which bmo-agency names — in a directory of the word's own, and the word names where they
//! stand at its end.

use std::path::{Path, PathBuf};

use job_codes::{ControlRefusal, JobError};
use job_policy::queue::QueueLimits;
use queue_read::dates::iso;

use crate::Refusal;

/// One file as it came: the name it was sent with and its bytes.
pub struct WordFile<'a> {
  pub name: &'a str,
  pub bytes: &'a [u8],
}

fn refused(refusal: ControlRefusal) -> Refusal {
  JobError::from(refusal).into()
}

/// Why the files cannot go: too many of them, an empty one, or one too large.
pub fn refusal(files: &[WordFile<'_>], limits: &QueueLimits) -> Result<(), Refusal> {
  if files.len() > limits.word_files {
    return Err(refused(ControlRefusal::TooManyFiles {
      most: u32::try_from(limits.word_files).unwrap_or(u32::MAX),
    }));
  }

  for file in files {
    let size = file.bytes.len() as u64;

    if size == 0 {
      return Err(refused(ControlRefusal::FileEmpty {
        name: file.name.to_owned(),
      }));
    }

    if size > limits.word_file_bytes {
      return Err(refused(ControlRefusal::FileTooLarge {
        name: file.name.to_owned(),
        size,
        most: limits.word_file_bytes,
      }));
    }
  }

  Ok(())
}

/// Where one word's files go: a directory of their own under the worker's workbench, named by the moment
/// they arrived and the process's clock below it, so two words never write over each other's files.
pub fn directory(workbench: &Path, now: i64) -> PathBuf {
  let stamp: String = iso(now).chars().filter(char::is_ascii_alphanumeric).collect();
  let below = std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .map_or(0, |since| since.subsec_nanos());

  workbench.join("uploads").join(format!("{stamp}-{below:08x}"))
}

// A file's name as it may stand on disk: its last path part, with nothing that could leave the directory
// or hide the file.
fn safe_name(name: &str, index: usize) -> String {
  let slashed = name.replace('\\', "/");
  let last = slashed.trim_end_matches('/').rsplit('/').next().unwrap_or_default();
  let kept: String = last.chars().filter(|character| !character.is_control()).collect();
  let base = kept.trim();

  if base.is_empty() || base.starts_with('.') {
    format!("file-{}{}", index + 1, if base.starts_with('.') { base } else { "" })
  } else {
    base.to_owned()
  }
}

/// The files written, two of one name both kept with the later one numbered; where each stands.
pub async fn save(directory: &Path, files: &[WordFile<'_>]) -> std::io::Result<Vec<PathBuf>> {
  if files.is_empty() {
    return Ok(vec![]);
  }

  tokio::fs::create_dir_all(directory).await?;

  let mut taken: Vec<String> = vec![];
  let mut paths = vec![];

  for (index, file) in files.iter().enumerate() {
    let mut name = safe_name(file.name, index);
    let mut copy = 2;

    while taken.contains(&name) {
      name = match name.rfind('.') {
        Some(dot) if dot > 0 => format!("{}-{copy}{}", &name[..dot], &name[dot..]),
        _ => format!("{name}-{copy}"),
      };
      copy += 1;
    }

    taken.push(name.clone());

    let path = directory.join(&name);

    tokio::fs::write(&path, file.bytes).await?;
    paths.push(path);
  }

  Ok(paths)
}

/// The word with the files named after it. Words and files both may stand alone.
pub fn with_files(text: &str, paths: &[PathBuf]) -> String {
  if paths.is_empty() {
    return text.to_owned();
  }

  let mut list = vec!["Files the owner attached to this request:".to_owned()];

  list.extend(paths.iter().map(|path| format!("- {}", path.display())));

  let list = list.join("\n");

  if text.is_empty() { list } else { format!("{text}\n\n{list}") }
}
