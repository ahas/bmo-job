//! The `bmo-job` command (docs/cli.md): the service, its MCP server, and its OpenAPI document.

use std::io::Write;
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
  }
}
