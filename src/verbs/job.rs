//! A job's own verbs (docs/verbs.md): the long wait's mark, the question to the owner, the live-Claude guard,
//! a checkout borrowed, a branch landed and a job closed, the shell hook, a launch, and the readings of what a
//! job cost. Each is a subcommand of the `job` group.

use std::path::{Path, PathBuf};

use clap::Subcommand;
use job_base::{Refusal, Settings};
use job_codes::{JobAskRefusal, JobError};

use super::ask::{self, AskRequest, Asked, Asking, Revised};
use super::borrow;
use super::branch_split;
use super::cowork_timing::{self, Orders};
use super::job_close::{self, JobCloseArgs};
use super::job_figures;
use super::job_guard::{self, Hooked};
use super::land;
use super::live_claude;
use super::stand::{Answer, JobProcess, Stand, StandError, VerbError};
use super::task::{self, TaskAsked};
use super::tool_calls;
use super::waiting::{self, WaitWords};

#[derive(Subcommand, Debug)]
pub enum Job {
  /// A job's own mark on a long wait: set on a unit or a pid, set around a command run after `--`, or cleared.
  Wait {
    #[arg(long, allow_hyphen_values = true)]
    todo: Option<String>,
    /// What the job waits on.
    #[arg(long, allow_hyphen_values = true)]
    what: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    unit: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    pid: Option<String>,
    /// A length from now (40m, 2h, 1h30m) or a clock time in UTC (14:30).
    #[arg(long, allow_hyphen_values = true)]
    until: Option<String>,
    #[arg(long)]
    clear: bool,
    /// The command run with the mark standing on this verb's own process.
    #[arg(last = true)]
    command: Vec<String>,
  },
  /// Opens the live-Claude guard for this job, saying what for, or with --list says what this job opened and
  /// ran.
  LiveClaude {
    #[arg(long, allow_hyphen_values = true)]
    purpose: Option<String>,
    #[arg(long)]
    list: bool,
  },
  /// Puts a choice to the owner in the middle of a job and waits for it, or with --talk answers his talk over
  /// the question and waits on it again.
  #[command(allow_negative_numbers = true)]
  Ask {
    #[arg(long, allow_hyphen_values = true)]
    todo: Option<String>,
    /// A room named for the question, which only the owner's private chat takes.
    #[arg(long)]
    chat: Option<i64>,
    #[arg(long, allow_hyphen_values = true)]
    question: Option<String>,
    /// One option apiece, in the order the chat numbers them.
    #[arg(long = "option", allow_hyphen_values = true)]
    options: Vec<String>,
    #[arg(long, allow_hyphen_values = true)]
    recommend: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    why: Option<String>,
    /// A wait in milliseconds, for a test that cannot sit through a minute.
    #[arg(long, allow_hyphen_values = true)]
    wait: Option<String>,
    /// The question whose talk this answers, by its number.
    #[arg(long, allow_hyphen_values = true)]
    talk: Option<String>,
    #[arg(long, allow_hyphen_values = true)]
    say: Option<String>,
  },
  /// Lands the job's branch the checkout stands on onto main and pushes main, for a repository with no close
  /// of its own that lands.
  Land,
  /// Closes a job in this repository: the close line, the checks and the suite as its gate, the branch landed,
  /// and the commit recorded as waiting to go live.
  Close(JobCloseArgs),
  /// Borrows a free checkout of another worker's repository for the rest of this job.
  Borrow {
    /// The worker whose repository is borrowed.
    worker: String,
  },
  /// The job's tool hook, with the hook's input on standard input.
  Guard,
  /// What the item's last job spent on its tool calls, one call at a time.
  #[command(allow_negative_numbers = true)]
  ToolCalls {
    #[arg(long)]
    todo: i64,
    /// How many of the longest calls to print; every call where none is named.
    #[arg(long)]
    top: Option<usize>,
  },
  /// What each job of the items named cost.
  Figures {
    items: Vec<i64>,
    #[arg(long)]
    json: bool,
  },
  /// What went on inside a Claude Workflow's branches, out of the records under this job's configuration
  /// directory: the newest workflow, the one named, or with --list every one still there.
  Branches {
    workflow: Option<String>,
    #[arg(long)]
    list: bool,
  },
  /// Where each work order's time went: the items named, or the newest that finished.
  CoworkTiming {
    items: Vec<i64>,
    #[arg(long)]
    recent: Option<i64>,
    #[arg(long)]
    steps: bool,
  },
  /// Launches a job on a worker through the server, which decides it.
  #[command(allow_negative_numbers = true)]
  Start {
    /// The room the job reports to.
    #[arg(long)]
    chat: Option<i64>,
    /// The item the job is a run of.
    #[arg(long)]
    todo: Option<i64>,
    #[arg(long)]
    worker: Option<String>,
    #[arg(long)]
    model: Option<String>,
    /// Who set the launch off.
    #[arg(long, allow_hyphen_values = true)]
    by: Option<String>,
    /// Why a launch with no item is made, in one line.
    #[arg(long, allow_hyphen_values = true)]
    why: Option<String>,
    #[arg(long)]
    cowork: bool,
    /// The task; read from standard input where none is given.
    #[arg(long, allow_hyphen_values = true)]
    prompt: Option<String>,
  },
}

/// Where a job verb runs: the settings, the job's markers, the home its paths are spelled under, and the
/// directory it was started in.
pub struct Place<'a> {
  pub settings: &'a Settings,
  pub process: &'a JobProcess,
  pub home: &'a Path,
  pub here: &'a Path,
}

fn stand_failed(why: StandError) -> VerbError {
  VerbError::Ended {
    code: 1,
    said: format!("job: {why}"),
  }
}

fn coded(refusal: Refusal) -> VerbError {
  VerbError::Coded(refusal)
}

fn number(given: Option<String>) -> Option<i64> {
  given.and_then(|given| given.trim().parse::<i64>().ok())
}

/// One job verb run where it was started.
pub async fn run(place: &Place<'_>, verb: Job) -> Result<Answer, VerbError> {
  // The hook runs before every command a job makes, so it builds only what it asks and stops nothing it
  // cannot read.
  if let Job::Guard = verb {
    let text = std::io::read_to_string(std::io::stdin()).unwrap_or_default();
    let home = place.home.to_string_lossy();
    let here = place.here.to_string_lossy();
    let hooked = Hooked {
      settings: place.settings,
      process: place.process,
      home: &home,
      here: &here,
    };

    return Ok(job_guard::hook(&hooked, &text).await);
  }

  if let Job::Branches { workflow, list } = verb {
    let config: Option<PathBuf> = std::env::var_os("CLAUDE_CONFIG_DIR")
      .filter(|named| !named.is_empty())
      .map(PathBuf::from);
    let project = branch_split::project_dir(config.as_deref(), place.home, place.here);

    return branch_split::branch_split_lines(&project, workflow.as_deref(), list)
      .map(Answer::from)
      .map_err(coded);
  }

  let stand = Stand::of(place.settings).await.map_err(stand_failed)?;
  let process = place.process;

  match verb {
    Job::Guard | Job::Branches { .. } => Ok(Answer::default()),
    Job::Wait {
      todo,
      what,
      unit,
      pid,
      until,
      clear,
      command,
    } => {
      let now = crate::now_ms();
      // Nothing after `--` is still a line that had one, which the reading refuses by name.
      let dashed = std::env::args().any(|word| word == "--");
      let words = WaitWords {
        todo,
        what,
        unit,
        pid,
        until,
        clear,
        command: dashed.then_some(command),
      };
      let request = waiting::read_request(words, now).map_err(waiting::refused_words)?;

      waiting::waiting(&stand, request, now).await
    }
    Job::LiveClaude { purpose, list } => {
      let run = process.job_run().ok_or_else(live_claude::not_a_job)?;

      if list {
        return live_claude::list(&stand.db, run).await.map(Answer::from);
      }

      live_claude::open(&stand.db, run, purpose.as_deref().unwrap_or_default(), crate::now_ms())
        .await
        .map(Answer::from)
    }
    Job::Ask {
      todo,
      chat,
      question,
      options,
      recommend,
      why,
      wait,
      talk,
      say,
    } => {
      let asking = Asking { stand: &stand };
      let asked = if let Some(talk) = talk {
        let revised = (!options.is_empty()).then(|| Revised {
          question,
          options,
          recommended: number(recommend),
          why,
        });

        asking.talk(number(Some(talk)), say.as_deref().unwrap_or_default(), revised).await
      } else {
        let wait_ms = match wait {
          None => ask::ASK_WAIT_MS,
          Some(given) => given
            .trim()
            .parse()
            .map_err(|_| coded(JobError::from(JobAskRefusal::QuestionWait).into()))?,
        };
        let request = AskRequest {
          room: chat,
          todo_id: number(todo),
          question: question.unwrap_or_default(),
          options,
          recommended: number(recommend),
          why: why.unwrap_or_default(),
        };

        asking.ask(&request, wait_ms).await
      };

      match asked.map_err(coded)? {
        Asked::Settled(line) => Ok(vec![line].into()),
        Asked::Talked(answer) => Ok(Answer {
          code: ask::TALK_EXIT,
          ..Answer::coded(&answer)
        }),
      }
    }
    Job::Land => land::land(&stand, place.here).await,
    Job::Close(args) => job_close::job_close(&stand, process, args, place.here).await,
    Job::Borrow { worker } => borrow::borrow(&stand, process.job_run(), &worker).await.map(Answer::from),
    Job::ToolCalls { todo, top } => tool_calls::tool_calls_lines(&stand, todo, top.unwrap_or(0)).await,
    Job::Figures { items, json } => {
      let read = job_figures::read(&stand, &items).await?;

      Ok(if json {
        vec![serde_json::to_string_pretty(&read).unwrap_or_default()].into()
      } else {
        job_figures::figures_lines(&read).into()
      })
    }
    Job::CoworkTiming {
      items,
      recent,
      steps,
    } => {
      let orders = match recent {
        Some(count) => Orders::Recent(count),
        None => Orders::Items(items),
      };

      cowork_timing::cowork_timing(&stand, orders, steps).await.map(Answer::from)
    }
    Job::Start {
      chat,
      todo,
      worker,
      model,
      by,
      why,
      cowork,
      prompt,
    } => {
      let prompt = prompt.unwrap_or_else(|| std::io::read_to_string(std::io::stdin()).unwrap_or_default());
      let Some(chat) = chat.filter(|_| !prompt.trim().is_empty()) else {
        return Err(VerbError::Ended {
          code: 2,
          said: task::USAGE.into(),
        });
      };
      let asked = TaskAsked {
        chat,
        prompt,
        model,
        item: todo,
        purpose: why.map(|why| why.trim().to_owned()).filter(|why| !why.is_empty()),
        worker,
        cowork,
        started_by: by,
      };

      task::task(&stand, &asked).await
    }
  }
}
