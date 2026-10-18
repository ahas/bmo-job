//! bmo-job: the TODO, work orders and the job queue, served through the service's own API (docs/api.md,
//! "Routes"). The server stands on the verbs' traits (`verbs`), the queue's own rows (`queue`), the item
//! talk (`talk`) and the event stream (`events`); the command line (`cli`) reads its words into the verbs.

pub mod cli;
pub mod events;
pub mod job_run;
pub mod order_run;
pub mod queue;
pub mod server;
pub mod talk;
pub mod verbs;

pub use job_base::{
  Refusal, RefusalBody, Settings, all, database, now_ms, now_seconds, one, run, settings,
};
