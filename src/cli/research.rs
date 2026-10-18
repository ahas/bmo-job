//! `research` (docs/cli.md, "`research`"): a research run's verbs as a research tool calls them — the depth a
//! run goes at, the run carried whole, an entry left open closed, and one branch in a pool place — each on the
//! stand, a stop from outside stopping the model it carries.

use std::future::Future;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clap::Subcommand;
use tokio::signal::unix::{Signal, SignalKind, signal};

use crate::verbs::research::Profile;
use crate::verbs::research::admission::{Demand, admit, parse_demand};
use crate::verbs::research::branch::{Branch, research_branch};
use crate::verbs::research::run::{self, Opening, Started, close_run, research_run};
use crate::verbs::stand::{Answer, Stand, VerbError};

/// What `research` takes.
#[derive(Subcommand, Debug)]
pub enum ResearchVerb {
  /// The key a run goes at: the one asked, or the shallowest with why.
  Admit {
    #[arg(long)]
    key: String,
    #[arg(long)]
    shallowest: String,
    /// The launcher's copy of the key's profile, as JSON.
    #[arg(long)]
    profile: String,
    /// `explicit` or `optional`.
    #[arg(long)]
    demand: Option<String>,
  },
  /// One run carried whole on the prompt read from standard input.
  #[command(allow_negative_numbers = true)]
  Run {
    #[arg(long)]
    key: String,
    #[arg(long)]
    chat: i64,
    #[arg(long)]
    unit: String,
    #[arg(long)]
    profile: String,
    #[arg(long)]
    tier: Option<String>,
    #[arg(long)]
    demand: Option<String>,
    /// The entry its launcher opened in the ledger.
    #[arg(long)]
    run: Option<i64>,
    /// The number the ledger records for the run's depth.
    #[arg(long, default_value_t = 0)]
    depth: u32,
    #[arg(long, allow_hyphen_values = true)]
    started_by: Option<String>,
    /// The number of the search the run carries, as bmo-artifact numbers its searches.
    #[arg(long)]
    search_id: Option<i64>,
    /// Leaves the entry open on an answer, for `research close`.
    #[arg(long)]
    leave_open: bool,
  },
  /// A run's entry left open, closed with how it ended.
  Close {
    #[arg(long)]
    run: i64,
    #[arg(long)]
    outcome: String,
    #[arg(long)]
    context_tokens: Option<u64>,
  },
  /// One branch of a run, its angle read from standard input.
  #[command(allow_negative_numbers = true)]
  Branch {
    #[arg(long)]
    owner: String,
    #[arg(long)]
    source: String,
    #[arg(long)]
    role: String,
    #[arg(long)]
    chat: i64,
    #[arg(long)]
    index: i64,
    /// When the parent run's research closes, in epoch milliseconds.
    #[arg(long)]
    closes_at: Option<i64>,
    #[arg(long)]
    strategy: Option<String>,
    #[arg(long)]
    host_interval_ms: Option<i64>,
    #[arg(long)]
    engine_interval_ms: Option<i64>,
  },
}

/// One call of a research verb on the stand, run in `here`.
pub async fn research(stand: &Stand, verb: ResearchVerb, here: &Path) -> Result<Answer, VerbError> {
  let cwd = here.display().to_string();

  match verb {
    ResearchVerb::Admit {
      key,
      shallowest,
      profile,
      demand,
    } => {
      let profile = profile_of(stand, &profile, &key)?;
      let admitted = admit(stand, &key, &shallowest, &profile, demand_of(demand.as_deref())?).await;

      Ok(
        [Some(admitted.depth), admitted.fallback]
          .into_iter()
          .flatten()
          .collect::<Vec<_>>()
          .into(),
      )
    }
    ResearchVerb::Run {
      key,
      chat,
      unit,
      profile,
      tier,
      demand,
      run,
      depth,
      started_by,
      search_id,
      leave_open,
    } => {
      let profile = profile_of(stand, &profile, &key)?;
      let started = Started {
        demand: demand_of(demand.as_deref())?,
        key,
        chat_id: chat,
        unit,
        tier,
        run_id: run,
        opening: Opening {
          depth,
          cap_ms: profile.cap_ms(),
          started_by,
          search_id,
        },
        profile,
        leave_open,
        cwd,
      };
      let prompt = standard_input("prompt")?;
      let stopping = Arc::new(AtomicBool::new(false));
      let ending =
        until_stopped(research_run(stand, &started, &prompt, stopping.clone()), &stopping).await?;

      Ok(Answer::coded(&ending))
    }
    ResearchVerb::Close {
      run,
      outcome,
      context_tokens,
    } => close_run(stand, run, &outcome, context_tokens).await,
    ResearchVerb::Branch {
      owner,
      source,
      role,
      chat,
      index,
      closes_at,
      strategy,
      host_interval_ms,
      engine_interval_ms,
    } => {
      let branch = Branch {
        owner,
        source,
        role,
        chat_id: chat,
        index,
        closes_at,
        strategy,
        host_interval_ms,
        engine_interval_ms,
        angle: standard_input("angle")?,
        cwd,
      };
      let stopping = Arc::new(AtomicBool::new(false));
      let said = until_stopped(research_branch(stand, &branch, stopping.clone()), &stopping).await;

      Ok(vec![said].into())
    }
  }
}

fn profile_of(stand: &Stand, snapshot: &str, key: &str) -> Result<Profile, VerbError> {
  Profile::parse(&stand.policy.research, snapshot, key).map_err(VerbError::Refused)
}

fn demand_of(given: Option<&str>) -> Result<Demand, VerbError> {
  parse_demand(given).ok_or_else(|| VerbError::Ended {
    code: 2,
    said: run::USAGE.to_owned(),
  })
}

fn standard_input(what: &str) -> Result<String, VerbError> {
  let mut text = String::new();

  std::io::stdin()
    .read_to_string(&mut text)
    .map_err(|why| VerbError::Refused(format!("the {what} could not be read: {why}")))?;

  Ok(text)
}

// The work carried to its end, an interrupt, a termination or a hang-up setting `stopping` on the way, which
// the work reads to stop what it carries and end with what it had.
async fn until_stopped<F: Future>(work: F, stopping: &AtomicBool) -> F::Output {
  let streams = [SignalKind::interrupt(), SignalKind::terminate(), SignalKind::hangup()];
  let [mut interrupt, mut terminate, mut hangup] = streams.map(|kind| signal(kind).ok());

  tokio::pin!(work);

  tokio::select! {
    done = &mut work => return done,
    () = next(&mut interrupt) => {}
    () = next(&mut terminate) => {}
    () = next(&mut hangup) => {}
  }

  stopping.store(true, Ordering::SeqCst);
  work.await
}

// The next signal of one kind, or never where the kind could not be listened for.
async fn next(stream: &mut Option<Signal>) {
  match stream {
    Some(stream) => {
      stream.recv().await;
    }
    None => std::future::pending().await,
  }
}
