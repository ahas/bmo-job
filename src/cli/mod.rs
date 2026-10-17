//! The command line (docs/cli.md): the binary `bmo-job` as the platform's entry point finds it, one noun a
//! call — the server, a job's unit, a job's own verbs, the TODO, a work order, the workers, a run past its
//! clock, a checkout's setup, a room's jobs and a research run's verbs — each read into its verb and what it
//! answers printed.

mod research;
mod serve;
mod todo;
mod worker;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use job_base::{Refusal, Settings};

use crate::verbs::job::{self, Job, Place};
use crate::verbs::stand::{Answer, JobProcess, Stand, VerbError};
use crate::verbs::todo::TodoArgs;
use crate::verbs::{checkout_setup, job_run, node_land, overtime, work_order};

use self::research::ResearchVerb;
use self::worker::WorkerVerb;

// How many database connections a verb that opens the database alone holds.
const CONNECTIONS: u32 = 2;

/// The binary's words: one noun, and what it takes.
#[derive(Parser, Debug)]
#[command(name = "bmo-job", version, about = "The TODO, work orders and the job queue")]
pub struct Cli {
  #[command(subcommand)]
  pub noun: Noun,
}

/// Every noun the binary carries (docs/cli.md, "The nouns").
#[derive(Subcommand, Debug)]
pub enum Noun {
  /// Serves the routes, with the work-order runner beside them.
  Serve,
  /// The program a job's unit runs, on the words its launch gave it.
  #[command(name = "job-run")]
  JobRun {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    words: Vec<String>,
  },
  /// A job's own verbs.
  #[command(subcommand)]
  Job(Job),
  /// The operations machine's side of a landing from a checkout on a work node.
  #[command(name = "node-land")]
  NodeLand {
    #[arg(long)]
    worker: String,
    /// Records the landed commit as waiting to go live, for this reason.
    #[arg(long, allow_hyphen_values = true)]
    record: Option<String>,
  },
  /// The TODO's verbs, each flag one verb.
  Todo(Box<TodoArgs>),
  /// One item's work order read, or a change to it asked of the server.
  #[command(name = "work-order")]
  WorkOrder {
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    words: Vec<String>,
  },
  /// The workers, through the server.
  #[command(subcommand)]
  Worker(WorkerVerb),
  /// The agent's answer to a run that reached its clock: the asks still open, or keep or cut on one.
  Overtime {
    #[arg(long)]
    list: bool,
    #[arg(long)]
    keep: Option<String>,
    #[arg(long)]
    cut: Option<String>,
    /// How long a keep lasts: 20m, 1h or a plain number of minutes.
    #[arg(long = "for")]
    for_given: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    why: Option<String>,
  },
  /// A checkout.
  #[command(subcommand)]
  Checkout(CheckoutVerb),
  /// A room.
  #[command(subcommand)]
  Room(RoomVerb),
  /// A research run's verbs, as a research tool calls them.
  #[command(subcommand)]
  Research(ResearchVerb),
  /// Every noun this binary carries, one a line.
  Nouns,
}

/// What `checkout` takes.
#[derive(Subcommand, Debug)]
pub enum CheckoutVerb {
  /// Sets up the checkout it is run in the way its repository sets one up.
  Setup,
}

/// What `room` takes.
#[derive(Subcommand, Debug)]
pub enum RoomVerb {
  /// A room's jobs, newest first.
  #[command(allow_negative_numbers = true)]
  Jobs {
    #[arg(long)]
    chat: i64,
    #[arg(long)]
    limit: Option<i64>,
  },
}

/// The nouns as `nouns` prints them, a noun of two words as two words.
pub const NOUNS: [&str; 15] = [
  "serve",
  "job-run",
  "job",
  "node-land",
  "todo",
  "work-order",
  "worker",
  "overtime",
  "checkout setup",
  "room jobs",
  "research admit",
  "research run",
  "research close",
  "research branch",
  "nouns",
];

/// What a verb answered, printed: its lines on standard output, a refusal's sentence on standard error,
/// ending with its code.
pub fn printed(answered: Result<Answer, VerbError>) -> ExitCode {
  match answered {
    Ok(answer) => {
      for line in answer.lines {
        println!("{line}");
      }

      ExitCode::from(answer.code)
    }
    Err(VerbError::Partly { lines, refusal }) => {
      for line in lines {
        println!("{line}");
      }

      eprintln!("{refusal}");
      ExitCode::from(1)
    }
    Err(failed) => {
      eprintln!("{failed}");
      ExitCode::from(failed.code())
    }
  }
}

fn settings() -> Result<Settings, VerbError> {
  Settings::load(&Settings::place()).map_err(|why| VerbError::Ended {
    code: 1,
    said: why.to_string(),
  })
}

async fn stand(settings: &Settings) -> Result<Stand, VerbError> {
  Stand::of(settings).await.map_err(|why| VerbError::Ended {
    code: 1,
    said: why.to_string(),
  })
}

fn here() -> PathBuf {
  std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"))
}

/// One call of the binary.
pub async fn run(cli: Cli) -> ExitCode {
  match cli.noun {
    Noun::Nouns => printed(Ok(NOUNS.map(str::to_owned).to_vec().into())),
    Noun::Checkout(CheckoutVerb::Setup) => printed(checkout_setup::checkout_setup(&here())),
    Noun::Serve => match settings() {
      Ok(settings) => serve::serve(&settings).await,
      Err(failed) => printed(Err(failed)),
    },
    Noun::JobRun { words } => match settings() {
      Ok(settings) => job_run::job_run(&settings, &words).await,
      Err(failed) => printed(Err(failed)),
    },
    noun => printed(answered(noun).await),
  }
}

async fn answered(noun: Noun) -> Result<Answer, VerbError> {
  let settings = settings()?;

  match noun {
    Noun::Job(verb) => {
      let process = JobProcess::of_process();
      let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
      let here = here();
      let place = Place {
        settings: &settings,
        process: &process,
        home: &home,
        here: &here,
      };

      job::run(&place, verb).await
    }
    Noun::NodeLand { worker, record } => {
      node_land::node_land(&stand(&settings).await?, &worker, record.as_deref()).await
    }
    Noun::Todo(args) => todo::todo(&stand(&settings).await?, &args).await,
    Noun::WorkOrder { words } => Ok(work_order::work_order(&stand(&settings).await?, &words).await),
    Noun::Worker(verb) => worker::worker(&stand(&settings).await?, verb).await,
    Noun::Overtime {
      list,
      keep,
      cut,
      for_given,
      why,
    } => {
      let db = settings.database.url.connect(CONNECTIONS).await.map_err(Refusal::from)?;
      let now = crate::now_ms();

      if list {
        return overtime::list(&db, now).await.map(Answer::from);
      }

      let agent = settings.deployment.agent_key.to_string();
      let (id, verdict) = match (keep, cut) {
        (Some(id), None) => (
          id,
          overtime::Verdict::Keep {
            for_given: for_given.as_deref(),
          },
        ),
        (None, Some(id)) if for_given.is_none() => (id, overtime::Verdict::Cut),
        _ => {
          return Err(VerbError::Ended {
            code: 2,
            said: overtime::USAGE.to_owned(),
          });
        }
      };

      overtime::answer(&db, &agent, &id, verdict, why.as_deref(), now).await.map(Answer::from)
    }
    Noun::Room(RoomVerb::Jobs { chat, limit }) => {
      let db = settings.database.url.connect(CONNECTIONS).await.map_err(Refusal::from)?;
      let jobs = queue_read::runs::jobs(&db, chat, limit).await?;

      Ok(Answer::from(
        jobs
          .iter()
          .map(|job| serde_json::to_string(job).unwrap_or_default())
          .collect::<Vec<_>>(),
      ))
    }
    Noun::Research(verb) => research::research(&stand(&settings).await?, verb, &here()).await,
    Noun::Serve | Noun::JobRun { .. } | Noun::Nouns | Noun::Checkout(_) => Ok(Answer::default()),
  }
}
