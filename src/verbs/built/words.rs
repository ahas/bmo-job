//! A word the owner said to one worker (docs/api.md, "A worker's word"): it launches nothing; its files are
//! written under the worker's workbench and named at the end of the words, and the words are handed to
//! bmo-agency for the worker's own session.

use job_agency::Agency;
use job_codes::{AgentRefusal, JobError};
use queue_read::lines::BUILTIN_WORKER;

use super::Built;
use crate::queue::units::agency_failed;
use crate::queue::word_files::{self, WordFile};
use crate::queue::workers::is_worker;
use crate::verbs::{Done, WorkerWord, WorkerWords};
use crate::{Refusal, now_ms};

impl WorkerWords for Built {
  async fn say_to_worker(&self, word: WorkerWord) -> Done<()> {
    let files: Vec<WordFile<'_>> = word
      .files
      .iter()
      .map(|file| WordFile {
        name: &file.name,
        bytes: &file.bytes,
      })
      .collect();

    word_files::refusal(&files, &self.policy.queue)?;

    // A name nobody registered is refused rather than falling back to the built-in worker in silence.
    let worker = word.worker.as_deref().unwrap_or(BUILTIN_WORKER);

    if !is_worker(&self.db, worker).await? {
      return Err(
        JobError::from(AgentRefusal::NoWorker {
          worker: worker.to_owned(),
        })
        .into(),
      );
    }

    let mut text = word.text.trim().to_owned();

    if !files.is_empty() {
      let agency = &self.ground.agency;
      let bench = agency.workbench(worker).await.map_err(agency_failed)?;
      let directory = word_files::directory(&bench.root, now_ms());
      let paths = word_files::save(&directory, &files).await.map_err(|why| {
        Refusal::from(sea_orm::DbErr::Custom(format!("the files were not written: {why}")))
      })?;

      text = word_files::with_files(&text, &paths);
    }

    self.ground.agency.say_to_worker(worker, &text).await.map_err(agency_failed)
  }
}
