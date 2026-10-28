//! The `bmo-job` command (docs/cli.md): the service, its MCP server, and its OpenAPI document.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bmo_job::Settings;
use bmo_job::job_close::{self, JobCloseArgs};
use bmo_job::todo_cli::{self, TodoArgs};
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
  /// Every noun this command carries, one a line, which `bmo` reads to know which commands are this unit's.
  Nouns,
  /// The work of a job at its end, handed over to the repository's deployment.
  Job {
    #[command(subcommand)]
    verb: JobVerb,
  },
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
    /// Tools the server leaves out, comma-separated.
    #[arg(long, value_name = "tools", env = "BMO_JOB_MCP_WITHOUT", value_delimiter = ',')]
    without: Vec<String>,
  },
  /// Closes an item through the service's own routes: its report and what it could not reach are kept on it,
  /// and its state is set to done.
  Todo {
    /// The item to close, by its number, with or without a leading `#`.
    #[arg(long, value_name = "item")]
    done: String,
    /// How the work was done, kept as the item's final report.
    #[arg(long, value_name = "text", allow_hyphen_values = true)]
    report: Option<String>,
    /// What the job's hands could not reach, kept as a held report.
    #[arg(long, value_name = "text", allow_hyphen_values = true)]
    unfinished: Option<String>,
    /// Who writes the closing; the run's agent where left out.
    #[arg(long, value_name = "name")]
    by: Option<String>,
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

#[derive(Subcommand)]
enum JobVerb {
  /// Hands the job's commit over to the deployment that records what goes live.
  Close {
    /// Why this goes live, in English.
    #[arg(long = "then", value_name = "why", allow_hyphen_values = true)]
    then_reason: String,
    /// Why the change reaches nothing the suite runs, in English.
    #[arg(long = "skip-suite", value_name = "why", allow_hyphen_values = true)]
    skip_suite: Option<String>,
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
    Verb::Nouns => {
      println!("job");
      println!("todo");

      ExitCode::SUCCESS
    }
    Verb::Job { verb } => match verb {
      JobVerb::Close {
        then_reason,
        skip_suite,
      } => {
        let root = std::env::current_dir().unwrap_or_default();
        let args = JobCloseArgs {
          then_reason,
          skip_suite,
        };

        match job_close::close(&root, &data_root(), &args, Path::new("bmo-testbed")) {
          Ok(lines) => {
            for line in lines {
              println!("{line}");
            }

            ExitCode::SUCCESS
          }
          Err(why) => {
            eprintln!("job-close: refused — {why}");
            ExitCode::FAILURE
          }
        }
      }
    },
    Verb::Todo {
      done,
      report,
      unfinished,
      by,
    } => ended(match done.trim().trim_start_matches('#').parse::<i64>() {
      Err(_) => Err(format!("--done {done:?} is not an item number")),
      Ok(done) => match todo_cli::client() {
        Ok(client) => {
          let by = by
            .or_else(|| std::env::var("BMO_TURN_AGENT").ok())
            .or_else(|| std::env::var("USER").ok())
            .unwrap_or_default();

          todo_cli::close(
            &client,
            &TodoArgs {
              done,
              report,
              unfinished,
              by,
            },
          )
          .await
          .map(|lines| {
            for line in lines {
              println!("{line}")
            }
          })
        }
        Err(why) => Err(why),
      },
    }),
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
      without,
    } => ended(
      bmo_job_plugin::serve_stdio(Start {
        service,
        token,
        holder,
        queues,
        channel,
        without: without
          .into_iter()
          .map(|name| name.trim().to_owned())
          .filter(|name| !name.is_empty())
          .collect(),
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

/// The platform's data root: the one the environment names, else the one under the home directory.
fn data_root() -> PathBuf {
  std::env::var_os("BMO_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
    std::env::var_os("HOME")
      .map(PathBuf::from)
      .unwrap_or_default()
      .join(".local/share/bmo/data")
  })
}
