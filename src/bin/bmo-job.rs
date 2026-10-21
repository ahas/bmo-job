//! The `bmo-job` command (docs/cli.md): the service, its MCP server, and its OpenAPI document.

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use bmo_job::Settings;
use bmo_job_plugin::Start;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "bmo-job", version, about = "A todo kept as middleware.")]
struct Command {
  #[command(subcommand)]
  verb: Verb,
}

#[derive(Subcommand)]
enum Verb {
  /// Serves the API by the settings file.
  Serve,
  /// Serves the harness tools over MCP on standard input and output, reaching the service at an address.
  Mcp {
    /// The base address the service's routes stand at.
    #[arg(long, value_name = "url")]
    service: String,
    /// The service's bearer token.
    #[arg(long, env = "BMO_JOB_TOKEN", hide_env_values = true)]
    token: String,
    /// The holder the queues named here are subscribed for.
    #[arg(long, value_name = "name", requires = "queues")]
    holder: Option<String>,
    /// A queue to be told of from the start; may be given several times.
    #[arg(long = "queue", value_name = "name", requires = "holder")]
    queues: Vec<String>,
    /// The experimental capability each event is also pushed on as a message.
    #[arg(long, value_name = "capability")]
    channel: Option<String>,
  },
  /// Prints the OpenAPI document.
  Openapi,
  /// Moves the old archive's exported rows into this store (docs/schema.md, TODO #1923).
  Import {
    /// The export directory the item's export hand wrote.
    #[arg(long, value_name = "dir")]
    export: PathBuf,
    /// The SQLite database written.
    #[arg(long, value_name = "url")]
    database: String,
    /// Reads, checks and counts everything, and writes nothing.
    #[arg(long)]
    dry_run: bool,
  },
}

/// The import's report, one table a line.
fn imported(report: &bmo_job::import::Report) -> Result<(), String> {
  for moved in &report.tables {
    println!(
      "{}: {} exported, {} written ({})",
      moved.source, moved.exported, moved.written, moved.note
    );
  }
  println!(
    "{} at snapshot {} read {}: {} rows sampled, {} failures",
    if report.dry_run { "dry run" } else { "import" },
    report.snapshot,
    report.read_at,
    report.sample.checked,
    report.sample.failures.len()
  );
  if report.sample.failures.is_empty() {
    Ok(())
  } else {
    for failure in &report.sample.failures {
      eprintln!("  {failure}");
    }

    Err("the count-and-sample compare failed".to_owned())
  }
}

fn ended(result: Result<(), String>) -> ExitCode {
  match result {
    Ok(()) => ExitCode::SUCCESS,
    Err(why) => {
      eprintln!("bmo-job: {why}");
      ExitCode::FAILURE
    }
  }
}

#[tokio::main]
async fn main() -> ExitCode {
  let Command { verb } = Command::parse();
  match verb {
    Verb::Serve => ended(match Settings::load(&Settings::place()) {
      Ok(settings) => bmo_job::serve(settings).await,
      Err(why) => Err(why),
    }),
    Verb::Mcp {
      service,
      token,
      holder,
      queues,
      channel,
    } => ended(
      bmo_job_plugin::serve_stdio(Start {
        service,
        token,
        holder,
        queues,
        channel,
      })
      .await,
    ),
    Verb::Openapi => ended(
      std::io::stdout()
        .write_all(bmo_job_plugin::openapi_json().as_bytes())
        .map_err(|error| format!("the document could not be written: {error}")),
    ),
    Verb::Import {
      export,
      database,
      dry_run,
    } => ended(
      bmo_job::import::run(&database, &export, dry_run)
        .await
        .and_then(|report| imported(&report)),
    ),
  }
}
