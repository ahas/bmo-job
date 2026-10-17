//! The service's binary (docs/cli.md): one noun a call — the server, the program a job's unit runs, a job's
//! own verbs, the TODO, a work order, the workers, a run past its clock, a checkout's setup and a room's
//! jobs.

use std::process::ExitCode;

use bmo_job::cli::{self, Cli};
use clap::Parser;

#[tokio::main]
async fn main() -> ExitCode {
  cli::run(Cli::parse()).await
}
