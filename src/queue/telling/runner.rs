//! What a job's own runner asks of the telling as it writes its ending: the job's row closed with its
//! report and ending, and a run that broke while it worked handed to the agent as an order to find the
//! cause.

use super::Teller;
use crate::Refusal;

impl Teller<'_> {
  /// The job's row closed at `now` with its report and how it ended. A job a person stopped ends as stopped
  /// whatever the runner read.
  pub async fn close_job(
    &self,
    job_id: i64,
    report: &str,
    ok: bool,
    outcome: &str,
    now: i64,
  ) -> Result<(), Refusal> {
    super::dead::finish_job(self, job_id, report, ok, outcome, now).await
  }

  /// A job whose closed row records a breakage handed to the agent; any other ending raises nothing.
  pub async fn order_broken_job(&self, job_id: i64, now: i64) -> Result<(), Refusal> {
    self.order_job_failure(job_id, now).await
  }
}
