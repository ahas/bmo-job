//! The `bmo-job` command (docs/cli.md): the service, its MCP server, and its OpenAPI document.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use bmo_job::Settings;
use bmo_job::job_ask::{self, Ending, NewAsk, Receiver, TALK_EXIT, TalkAsk};
use bmo_job::job_close::{self, JobCloseArgs, Standing};
use bmo_job::todo_cli::{self, AddArgs, StopHereArgs, TodoArgs};
use bmo_job::work_order_cli;
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
  /// An item closed, held with the run's account on it, or raised, each through the service's own routes.
  Todo {
    /// The item to close, by its number, with or without a leading `#`.
    #[arg(long, value_name = "item", conflicts_with_all = ["stop_here", "add"])]
    done: Option<String>,
    /// The item the run stops short on: its account left on it, and the item held where it is not yet.
    #[arg(long = "stop-here", value_name = "item", conflicts_with = "add")]
    stop_here: Option<String>,
    /// Raises a new row out of the run's own work.
    #[arg(long)]
    add: bool,
    /// The account of the work: on a closing the final report, on a stop the held one.
    #[arg(long, value_name = "text", allow_hyphen_values = true)]
    report: Option<String>,
    /// What the job's hands could not reach, kept as a held report.
    #[arg(long, value_name = "text", allow_hyphen_values = true)]
    unfinished: Option<String>,
    /// Why the run holds the item, in English.
    #[arg(long, value_name = "reason", allow_hyphen_values = true)]
    why: Option<String>,
    /// The item whose finishing makes this one ready.
    #[arg(long, value_name = "item")]
    after: Option<String>,
    /// The item, or the commit, whose going live makes this one ready.
    #[arg(long = "after-deploy", value_name = "item or commit")]
    after_deploy: Option<String>,
    /// The new row's title.
    #[arg(long, value_name = "words", allow_hyphen_values = true)]
    title: Option<String>,
    /// What the work is, why it was left and what to do about it: the new row's brief.
    #[arg(long, value_name = "words", allow_hyphen_values = true)]
    judgement: Option<String>,
    /// The strands of notes the new row takes up, as `<id>:<strand>[,<id>:<strand>]`.
    #[arg(long = "from-todo", value_name = "strands")]
    from_todo: Option<String>,
    /// The item the new row was raised from.
    #[arg(long = "raised-by", value_name = "item")]
    raised_by: Option<String>,
    /// Who writes it; the run's agent where left out.
    #[arg(long, value_name = "name")]
    by: Option<String>,
  },
  /// A step added to an item's work order.
  #[command(name = "work-order")]
  WorkOrder {
    /// The item whose work order it is.
    #[arg(long, value_name = "item")]
    todo: String,
    /// The step's title.
    #[arg(long, value_name = "title", allow_hyphen_values = true)]
    add: String,
    /// What the step is to do.
    #[arg(long, value_name = "words", allow_hyphen_values = true)]
    instruction: Option<String>,
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
    /// The item the job works on, which the close writes where it stands onto.
    #[arg(long, value_name = "item")]
    todo: Option<i64>,
  },
  /// Puts a choice to the owner in the middle of a job and waits for a person to settle it, or with --talk answers
  /// his talk over the question and waits on it again.
  Ask {
    /// The item it is asked about.
    #[arg(long, value_name = "item")]
    todo: Option<String>,
    /// What is to be settled.
    #[arg(long, value_name = "words", allow_hyphen_values = true)]
    question: Option<String>,
    /// One option apiece, in the order the room numbers them.
    #[arg(long = "option", value_name = "words", allow_hyphen_values = true)]
    options: Vec<String>,
    /// The option you would take, by its number.
    #[arg(long, value_name = "n", allow_hyphen_values = true)]
    recommend: Option<String>,
    /// Why you cannot settle it yourself, in one line.
    #[arg(long, value_name = "words", allow_hyphen_values = true)]
    why: Option<String>,
    /// The question whose talk this answers, by the number its talk came back with.
    #[arg(long, value_name = "question")]
    talk: Option<String>,
    /// Your answer in the talk.
    #[arg(long, value_name = "words", allow_hyphen_values = true)]
    say: Option<String>,
  },
}

// A number as a shell names it, with or without a leading `#`.
fn numbered(said: &str) -> Option<i64> {
  said.trim().trim_start_matches('#').parse().ok()
}

// Who the run writes as: the name given, else the run's agent, else the shell's user.
fn writer(named: Option<String>) -> String {
  named
    .or_else(|| std::env::var("BMO_TURN_AGENT").ok())
    .or_else(|| std::env::var("USER").ok())
    .unwrap_or_default()
}

// What the run was told to stop by: the signals a run and its pane are ended with.
async fn told_to_stop() {
  use tokio::signal::unix::{SignalKind, signal};

  let kinds = [SignalKind::terminate(), SignalKind::interrupt(), SignalKind::hangup()];
  let mut heard: Vec<_> = kinds.into_iter().filter_map(|kind| signal(kind).ok()).collect();

  if heard.is_empty() {
    return std::future::pending().await;
  }

  let waits = heard.iter_mut().map(|one| Box::pin(one.recv()));

  futures_util::future::select_all(waits).await;
}

// The close, with where it stands written on the item it was told as it moves. Told to stop while its check
// runs, it takes its standing off the item and ends as a stopped run does.
async fn job_close(args: JobCloseArgs, todo: Option<i64>) -> ExitCode {
  let root = std::env::current_dir().unwrap_or_default();
  let data_root = data_root();
  let item = todo.and_then(|item| match todo_cli::client() {
    Ok(client) => Some((client, item)),
    Err(why) => {
      eprintln!("job-close: item {item} is not written on — {why}");
      None
    }
  });
  let runtime = tokio::runtime::Handle::current();
  let written = item.clone();
  let close = tokio::task::spawn_blocking(move || {
    job_close::close(&root, &data_root, &args, Path::new("bmo-testbed"), &|standing| {
      if let Some((client, item)) = &written
        && let Err(why) = runtime.block_on(job_close::write_standing(client, *item, &standing))
      {
        eprintln!("job-close: {why}");
      }
    })
  });

  tokio::select! {
    closed = close => match closed {
      Ok(Ok(lines)) => {
        for line in lines {
          println!("{line}");
        }

        ExitCode::SUCCESS
      }
      Ok(Err(why)) => {
        eprintln!("job-close: refused — {why}");
        ExitCode::FAILURE
      }
      Err(why) => {
        eprintln!("job-close: the close did not end — {why}");
        ExitCode::FAILURE
      }
    },
    () = told_to_stop() => {
      if let Some((client, item)) = &item
        && let Err(why) = job_close::write_standing(client, *item, &Standing::Off).await
      {
        eprintln!("job-close: {why}");
      }

      eprintln!("job-close: told to stop");
      ExitCode::from(130)
    }
  }
}

// The verb asked: a new question or a talk, and how its wait ended as the job reads it.
async fn job_ask(
  todo: Option<String>,
  question: Option<String>,
  options: Vec<String>,
  recommend: Option<String>,
  why: Option<String>,
  talk: Option<String>,
  say: Option<String>,
) -> ExitCode {
  let reached = todo_cli::client().and_then(|client| Ok((client, Receiver::of_process(&data_root())?)));
  let (client, receiver) = match reached {
    Ok(reached) => reached,
    Err(why) => {
      eprintln!("ask: {why}");

      return ExitCode::FAILURE;
    }
  };
  let recommend = recommend.as_deref().and_then(numbered);
  let waited = match talk {
    Some(talk) => {
      let Some(question_id) = numbered(&talk) else {
        eprintln!("ask: --talk {talk:?} is not a question's number\n{}", job_ask::USAGE);

        return ExitCode::FAILURE;
      };
      let asked = TalkAsk {
        question: question_id,
        say: say.unwrap_or_default(),
        options,
        recommend,
        text: question,
        why,
      };

      job_ask::talk(&client, &receiver, &asked, told_to_stop()).await
    }
    None => {
      let asked = NewAsk {
        item: todo.as_deref().and_then(numbered),
        question: question.unwrap_or_default(),
        options,
        recommend,
        why: why.unwrap_or_default(),
        by: std::env::var("BMO_RUN_NAME").unwrap_or_else(|_| writer(None)),
      };

      job_ask::ask(&client, &receiver, &asked, told_to_stop()).await
    }
  };

  match waited {
    Ok(Ending::Settled(line)) => {
      println!("{line}");

      ExitCode::SUCCESS
    }
    Ok(Ending::Talk(message)) => {
      println!("{message}");

      ExitCode::from(TALK_EXIT)
    }
    Err(why) => {
      eprintln!("{why}");

      ExitCode::FAILURE
    }
  }
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
      println!("job ask\thome");
      println!("todo");
      println!("work-order");

      ExitCode::SUCCESS
    }
    Verb::Job { verb } => match verb {
      JobVerb::Close {
        then_reason,
        skip_suite,
        todo,
      } => {
        let args = JobCloseArgs {
          then_reason,
          skip_suite,
        };

        job_close(args, todo).await
      }
      JobVerb::Ask {
        todo,
        question,
        options,
        recommend,
        why,
        talk,
        say,
      } => job_ask(todo, question, options, recommend, why, talk, say).await,
    },
    Verb::Todo {
      done,
      stop_here,
      add,
      report,
      unfinished,
      why,
      after,
      after_deploy,
      title,
      judgement,
      from_todo,
      raised_by,
      by,
    } => ended(
      async {
        let client = todo_cli::client()?;
        let by = writer(by);
        let lines = if add {
          let raised_by = match raised_by.as_deref() {
            Some(said) => {
              Some(numbered(said).ok_or_else(|| format!("--raised-by {said:?} is not an item number"))?)
            }
            None => None,
          };
          let args = AddArgs {
            title: title.unwrap_or_default(),
            judgement: judgement.unwrap_or_default(),
            raised_by,
            from_todo,
            by,
          };

          vec![todo_cli::add(&client, &args).await?]
        } else if let Some(said) = stop_here {
          let item =
            numbered(&said).ok_or_else(|| format!("--stop-here {said:?} is not an item number"))?;
          let args = StopHereArgs {
            item,
            report,
            why,
            after,
            after_deploy,
            by,
          };

          todo_cli::stop_here(&client, &args).await?
        } else if let Some(said) = done {
          let done = numbered(&said).ok_or_else(|| format!("--done {said:?} is not an item number"))?;
          let args = TodoArgs {
            done,
            report,
            unfinished,
            by,
          };

          todo_cli::close(&client, &args).await?
        } else {
          return Err("todo takes --done <item>, --stop-here <item> or --add".to_owned());
        };

        for line in lines {
          println!("{line}");
        }

        Ok::<(), String>(())
      }
      .await,
    ),
    Verb::WorkOrder {
      todo,
      add,
      instruction,
    } => ended(
      async {
        let item = numbered(&todo).ok_or_else(|| format!("--todo {todo:?} is not an item number"))?;
        let client = todo_cli::client()?;
        let line = work_order_cli::add_step(&client, item, &add, instruction.as_deref()).await?;

        println!("{line}");

        Ok::<(), String>(())
      }
      .await,
    ),
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
