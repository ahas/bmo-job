//! The `bmo-job` command (docs/cli.md): the service, its MCP server, and its OpenAPI document.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{Arc, Mutex};

use bmo_job::Settings;
use bmo_job::close_watch::Running;
use bmo_job::job_ask::{self, Ending, NewAsk, Receiver, Shown, TALK_EXIT, TalkAsk};
use bmo_job::job_close::{self, JobCloseArgs, Standing, Stands};
use bmo_job::todo_cli::{self, AddArgs, StopHereArgs, TodoArgs};
use bmo_job_client::JobClient;
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
  /// The run's account left on an item it finished or stopped short on, or a row raised, each through the
  /// service's own routes; a run moves no item's state.
  Todo {
    /// The item the run finished, by its number, with or without a leading `#`.
    #[arg(long, value_name = "item", conflicts_with_all = ["stop_here", "add"])]
    done: Option<String>,
    /// The item the run stops short on: its account left on it.
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
    /// A repository the new row's work is in, given again or joined with commas for each; the first is its
    /// assignee, and two or more its label `repositories`. The raising item's where left out.
    #[arg(long = "repository", value_name = "name", value_delimiter = ',')]
    repositories: Vec<String>,
    /// Who writes it; the run's agent where left out.
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
  /// Checks what the job changed, merges its commits onto the remote's main and leaves the item waiting for its
  /// repository's batch deployment.
  Close {
    /// Why this goes live, in English.
    #[arg(long = "then", value_name = "why", allow_hyphen_values = true)]
    then_reason: String,
    /// A reason in English, read for its language alone: the close runs no suite.
    #[arg(long = "skip-suite", value_name = "why", allow_hyphen_values = true)]
    skip_suite: Option<String>,
    /// The item the job works on, which the close writes where it stands onto.
    #[arg(long, value_name = "item")]
    todo: Option<i64>,
    /// A checkout to close, given once for each; with none, the checkout the close runs in.
    #[arg(long = "checkout", value_name = "path")]
    checkouts: Vec<PathBuf>,
  },
  /// Puts a question to the job's supervisor in the middle of a job, which settles it or puts it to the owner, and
  /// waits for it to be settled; with --talk answers the talk over the question and waits on it again, or with
  /// --show reads a question or an item's questions.
  Ask {
    /// The item it is asked about.
    #[arg(long, value_name = "item")]
    todo: Option<String>,
    /// What is to be settled or done.
    #[arg(long, value_name = "words", allow_hyphen_values = true)]
    question: Option<String>,
    /// One option apiece, in the order the room numbers them; none for an open question.
    #[arg(long = "option", value_name = "words", allow_hyphen_values = true)]
    options: Vec<String>,
    /// The option you would take, by its number, where there are options.
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
    /// With --talk: the words ask nothing, an acknowledgement such as that you wait, and are handed to nobody.
    #[arg(long)]
    ack: bool,
    /// Reads the question by its number, or with --todo and no number every question asked about the item.
    #[arg(long, value_name = "question", num_args = 0..=1)]
    show: Option<Option<String>>,
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

// The run's stop as many waits hear it: each future the returned call makes ends once the run is told to stop,
// at once where it already was.
fn stop_heard() -> impl Fn() -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> + Clone {
  let (tell, heard) = tokio::sync::watch::channel(false);

  tokio::spawn(async move {
    told_to_stop().await;
    let _ = tell.send(true);
  });

  move || {
    let mut heard = heard.clone();

    Box::pin(async move {
      if heard.wait_for(|told| *told).await.is_err() {
        std::future::pending::<()>().await;
      }
    })
  }
}

// The close, with where it stands written on the item it was told as it moves, each write made again while the
// service gives no answer, for two minutes at most. It ends at the merge and waits for no deployment. Told to stop
// while its check runs, it ends the check with everything under it, takes its standing off the item and ends as a
// stopped run does. Whatever ends it, its last line says so (docs/cli.md, 「`bmo-job job close`」). With
// checkouts named, it closes them all in one close (「Several checkouts in one close」).
async fn job_close(args: JobCloseArgs, todo: Option<i64>, checkouts: Vec<PathBuf>) -> ExitCode {
  let code = if checkouts.is_empty() {
    closed(args, todo).await
  } else {
    closed_checkouts(args, todo, checkouts).await
  };

  close_ended(code)
}

// The close's last line, which a caller reading its log takes as its end.
fn close_ended(code: u8) -> ExitCode {
  println!("job-close: ended, exit {code}");
  ExitCode::from(code)
}

// The run told to stop: ended at once, its last line said.
fn stopped() -> ! {
  println!("job-close: ended, exit 130");
  std::process::exit(130);
}

// The run's stop as one wait hears it.
type Stop = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>;

// The item a close writes on, where it was told one and the job service can be named.
fn close_item(todo: Option<i64>) -> Option<(JobClient, i64)> {
  todo.and_then(|item| match todo_cli::client() {
    Ok(client) => Some((client, item)),
    Err(why) => {
      eprintln!("job-close: item {item} is not written on — {why}");
      None
    }
  })
}

// Where the close's repositories stand written on its item, from the close's own thread; a write that does not
// land is said in one line. Answers whether nothing is left unwritten: true where it landed or no item is named.
fn told(
  runtime: &tokio::runtime::Handle,
  item: Option<&(JobClient, i64)>,
  standings: &[Stands],
  stop: &dyn Fn() -> Stop,
) -> bool {
  let Some((client, item)) = item else { return true };

  match runtime.block_on(job_close::keep_standings(
    client,
    *item,
    standings,
    job_close::STANDING_WITHIN,
    stop(),
  )) {
    Ok(()) => true,
    Err(why) => {
      eprintln!("job-close: {why}");
      false
    }
  }
}

async fn closed(args: JobCloseArgs, todo: Option<i64>) -> u8 {
  let root = std::env::current_dir().unwrap_or_default();
  let data_root = data_root();
  let repository = job_close::repository_of(&root);
  let item = close_item(todo);
  let stop = stop_heard();
  let runtime = tokio::runtime::Handle::current();
  let written = item.clone();
  let stop_writing = stop.clone();
  let running = Running::default();
  let running_now = running.clone();
  let written_for = repository.clone();
  let close = tokio::task::spawn_blocking(move || {
    let steps = job_close::Steps {
      testbed: Path::new("bmo-testbed"),
      clocks: job_close::Clocks::for_checkout(&root),
      running: running_now,
      say: &|line| println!("{line}"),
    };

    job_close::close(&root, &data_root, &args, &steps, &|standing| {
      told(&runtime, written.as_ref(), &[(written_for.clone(), standing)], &stop_writing);
    })
  });

  awaited(close, running, item, stop(), move || vec![(repository, Standing::Off)]).await
}

async fn closed_checkouts(args: JobCloseArgs, todo: Option<i64>, paths: Vec<PathBuf>) -> u8 {
  let from = std::env::current_dir().unwrap_or_default();
  let roots = match job_close::checkout_roots(&from, &paths) {
    Ok(roots) => roots,
    Err(why) => {
      eprintln!("job-close: refused — {why}");
      return 1;
    }
  };
  let data_root = data_root();
  let item = close_item(todo);
  let waiting = match &item {
    Some((client, item)) => waiting_on(client, *item).await,
    None => BTreeMap::new(),
  };
  let stop = stop_heard();
  let runtime = tokio::runtime::Handle::current();
  let written = item.clone();
  let stop_writing = stop.clone();
  let running = Running::default();
  let running_now = running.clone();
  // Each repository's standing the item does not carry yet, or carries as `testing`: kept before its write and
  // let go once a write that leaves it off `testing` lands, so a stop that cuts the write still writes it.
  let owed: Arc<Mutex<BTreeMap<String, Standing>>> = Arc::default();
  let owed_now = Arc::clone(&owed);
  let close = tokio::task::spawn_blocking(move || {
    let steps = job_close::Steps {
      testbed: Path::new("bmo-testbed"),
      clocks: job_close::Clocks::for_checkouts(&roots),
      running: running_now,
      say: &|line| println!("{line}"),
    };
    let checkouts = job_close::Checkouts {
      from: &from,
      roots: &roots,
      waiting: &waiting,
    };

    let held = || owed_now.lock().unwrap_or_else(|poisoned| poisoned.into_inner());

    job_close::close_checkouts(&checkouts, &data_root, &args, &steps, &|standings| {
      held().extend(standings.iter().cloned());

      if told(&runtime, written.as_ref(), standings, &stop_writing) {
        let mut held = held();

        for (repository, standing) in standings {
          if *standing != Standing::Testing && held.get(repository) == Some(standing) {
            held.remove(repository);
          }
        }
      }
    })
  });

  awaited(close, running, item, stop(), move || {
    let owed = std::mem::take(&mut *owed.lock().unwrap_or_else(|poisoned| poisoned.into_inner()));

    owed
      .into_iter()
      .map(|(repository, standing)| match standing {
        Standing::Testing => (repository, Standing::Off),
        standing => (repository, standing),
      })
      .collect()
  })
  .await
}

// The commits the item already names as waiting on main, by repository, which a close of several checkouts leaves
// out; none where the item cannot be read, said in one line, so every checkout on main is checked.
async fn waiting_on(client: &JobClient, item: i64) -> BTreeMap<String, String> {
  match tokio::time::timeout(job_close::ANSWER_WITHIN, client.item(item)).await {
    Ok(Ok(read)) => job_close::waiting_commits(&read.labels),
    Ok(Err(why)) => {
      eprintln!("job-close: item {item} could not be read, so every checkout on main is checked: {why}");
      BTreeMap::new()
    }
    Err(_) => {
      eprintln!(
        "job-close: item {item} could not be read, so every checkout on main is checked: the job service gave no answer"
      );
      BTreeMap::new()
    }
  }
}

// A close run to its end, or to the run's stop: told to stop, the run has no wait left in it: the step it runs is
// ended with everything under it, what `left` names is written once, each repository still `testing` off, and
// the close's own thread ends with the process rather than going on to write after it.
async fn awaited(
  close: tokio::task::JoinHandle<Result<Vec<String>, String>>,
  running: Running,
  item: Option<(JobClient, i64)>,
  stop: Stop,
  left: impl FnOnce() -> Vec<Stands>,
) -> u8 {
  tokio::select! {
    closed = close => match closed {
      Ok(Ok(lines)) => {
        for line in lines {
          println!("{line}");
        }

        0
      }
      Ok(Err(why)) => {
        eprintln!("job-close: refused — {why}");
        1
      }
      Err(why) => {
        eprintln!("job-close: the close did not end — {why}");
        1
      }
    },
    () = stop => {
      let ended = tokio::task::spawn_blocking(move || running.take_down(&job_close::Clocks::default()))
        .await
        .unwrap_or_default();

      if !ended.is_empty() {
        eprintln!("job-close: ended what the step was running: {}", ended.join("; "));
      }

      if let Some((client, item)) = &item {
        let left = left();
        let written =
          tokio::time::timeout(job_close::ANSWER_WITHIN, job_close::write_standings(client, *item, &left)).await;

        match written {
          Ok(Ok(())) => {}
          Ok(Err(why)) => eprintln!("job-close: item {item} was not written on: {why}"),
          Err(_) => eprintln!("job-close: item {item} was not written on: the job service gave no answer"),
        }
      }

      eprintln!("job-close: told to stop");
      stopped();
    }
  }
}

// A talk as the verb was given it: the question's number, the job's words, and whether they ask nothing.
struct Talking {
  question: Option<String>,
  say: Option<String>,
  ack: bool,
}

// The verb asked: a new question or a talk, and how its wait ended as the job reads it.
async fn job_ask(
  todo: Option<String>,
  question: Option<String>,
  options: Vec<String>,
  recommend: Option<String>,
  why: Option<String>,
  talking: Talking,
) -> ExitCode {
  let Talking {
    question: talk,
    say,
    ack,
  } = talking;

  if ack && talk.is_none() {
    eprintln!(
      "ask: --ack acknowledges a talk: name its question with --talk <question>\n{}",
      job_ask::USAGE
    );

    return ExitCode::FAILURE;
  }

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
        ack,
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
    Ok(Ending::SettledBefore { question, line }) => {
      println!("{line}");
      eprintln!("ask: question #{question} was settled before; its answer stands");

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

// `--show`: a question by its number or an item's questions, each printed as JSON on a line of its own.
async fn job_ask_show(todo: Option<String>, question: Option<String>, asked: bool) -> ExitCode {
  let refused = |why: &str| {
    eprintln!("ask: {why}\n{}", job_ask::USAGE);

    ExitCode::FAILURE
  };

  if asked {
    return refused("--show reads, and takes no words of a question or a talk.");
  }

  let shown = match (question, todo) {
    (Some(question), None) => match numbered(&question) {
      Some(id) => Shown::Question(id),
      None => return refused(&format!("--show {question:?} is not a question's number.")),
    },
    (None, Some(todo)) => match numbered(&todo) {
      Some(item) => Shown::Item(item),
      None => return refused(&format!("--todo {todo:?} is not an item's number.")),
    },
    (Some(_), Some(_)) => {
      return refused("--show reads a question by its number or an item's questions, not both.");
    }
    (None, None) => return refused("--show names a question's number, or --todo names the item."),
  };
  let client = match todo_cli::client() {
    Ok(client) => client,
    Err(why) => {
      eprintln!("ask: {why}");

      return ExitCode::FAILURE;
    }
  };

  match job_ask::show(&client, shown).await {
    Ok(readings) => {
      for reading in readings {
        match serde_json::to_string(&reading) {
          Ok(line) => println!("{line}"),
          Err(why) => {
            eprintln!("ask: {why}");

            return ExitCode::FAILURE;
          }
        }
      }

      ExitCode::SUCCESS
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

      ExitCode::SUCCESS
    }
    Verb::Job { verb } => match verb {
      JobVerb::Close {
        then_reason,
        skip_suite,
        todo,
        checkouts,
      } => {
        let args = JobCloseArgs {
          then_reason,
          skip_suite,
        };

        job_close(args, todo, checkouts).await
      }
      JobVerb::Ask {
        todo,
        question,
        options,
        recommend,
        why,
        talk,
        say,
        ack,
        show: Some(show),
      } => {
        let asked = question.is_some()
          || !options.is_empty()
          || recommend.is_some()
          || why.is_some()
          || talk.is_some()
          || say.is_some()
          || ack;

        job_ask_show(todo, show, asked).await
      }
      JobVerb::Ask {
        todo,
        question,
        options,
        recommend,
        why,
        talk,
        say,
        ack,
        show: None,
      } => {
        let talking = Talking {
          question: talk,
          say,
          ack,
        };

        job_ask(todo, question, options, recommend, why, talking).await
      }
    },
    Verb::Todo {
      done,
      stop_here,
      add,
      report,
      unfinished,
      title,
      judgement,
      from_todo,
      raised_by,
      repositories,
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
            repositories,
            from_todo,
            by,
          };

          vec![todo_cli::add(&client, &args).await?]
        } else if let Some(said) = stop_here {
          let item =
            numbered(&said).ok_or_else(|| format!("--stop-here {said:?} is not an item number"))?;
          let args = StopHereArgs { item, report, by };

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
