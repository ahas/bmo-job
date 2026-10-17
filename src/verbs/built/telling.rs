//! A job's end and the queue's standing told to the agent (docs/api.md, "A job's end"), by the queue's
//! telling.

use job_codes::QueueTold;

use super::Built;
use crate::now_ms;
use crate::verbs::{Done, JobEnded, Moment, Telling};

impl Telling for Built {
  async fn job_ended(&self, ended: JobEnded) -> Done<QueueTold> {
    let units = self.units();
    let teller = self.teller(&units);

    // The report the job left waiting goes before the fact of its end, which tells what came of it.
    if let Some(job) = ended.job {
      teller.deliver_report(job, now_ms()).await?;
    }

    let ending = teller.ending_of(ended.job, &ended.outcome).await?;
    let told = teller.tell_job_ended(&ending, now_ms()).await?;

    self.events.queue_moved(None);

    Ok(told)
  }

  async fn deliver_report(&self, job: i64) -> Done<()> {
    let units = self.units();

    self.teller(&units).deliver_report(job, now_ms()).await
  }

  async fn tell_queue(&self, moment: Moment) -> Done<QueueTold> {
    let units = self.units();

    self
      .teller(&units)
      .tell_queue_standing(&moment.moment, moment.repeat, moment.rides_ms, now_ms())
      .await
  }
}
