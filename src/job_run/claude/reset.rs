//! A reset the owner asked of a worker's sessions while a job of its ran, asked for by its next job as it
//! starts, before the job looks for a session to resume. The reset itself — the records let go and a session
//! a person opened in the standing program dropped — is bmo-agency's.

use super::super::Ground;
use crate::Refusal;

/// A reset waiting for the worker's sessions asked carried out; whether one waited.
pub async fn ask_worker_reset(ground: &Ground<'_>, worker: &str) -> Result<bool, Refusal> {
  ground.calls.carry_out_reset(worker).await
}
