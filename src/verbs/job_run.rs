//! `job-run`: the verb a job's unit is started as (docs/runs.md, "What the runner is handed"). Its words are
//! the runner's arguments; the runner's ground is built on the queue's ground the settings name, the
//! checkout's locks are held, and the job is run, the verb ending with the run's own exit code.

use std::process::ExitCode;

use job_base::Settings;
use job_policy::{Names, Policy};
use queue_rows::closing::Reader;

use crate::job_run::{self, Ground, JobArgs, hold_locks};
use crate::queue::ground::QueueGround;

/// The verb's usage, said where its words do not read.
pub const USAGE: &str = "usage: job-run -- <chat> <prompt file> <job> [<model> <unit> <worker> <cwd> <phase> <session word> <structure> <final baseline> <retry model>]";

// How many database connections a run holds open at most.
const CONNECTIONS: u32 = 2;

/// The verb on its words, after the `--` that ends the program's own: the job run, or why it never started.
pub async fn job_run(settings: &Settings, words: &[String]) -> ExitCode {
  let words = words.strip_prefix(&["--".to_owned()]).unwrap_or(words);
  let Some(args) = JobArgs::read(words) else {
    eprintln!("job-run: the words name no chat, prompt file and job.\n{USAGE}");
    return ExitCode::from(2);
  };
  let built = QueueGround::of_process(settings)
    .map_err(|why| why.to_string())
    .and_then(|queue| Ok((queue, Policy::load(settings).map_err(|why| why.to_string())?)));
  let (queue, policy) = match built {
    Ok(built) => built,
    Err(why) => {
      eprintln!("job-run: {why}");
      return ExitCode::from(1);
    }
  };
  let db = match settings.database.url.connect(CONNECTIONS).await {
    Ok(db) => db,
    Err(why) => {
      eprintln!("job-run: the database cannot be reached: {why}");
      return ExitCode::from(1);
    }
  };
  let names = Names::of(&settings.deployment);
  let reader = Reader::new(&policy.closing, &names);
  let units = queue.units();
  let calls = queue.run_calls();
  let ground = Ground {
    db: &db,
    units: &units,
    calls: &calls,
    policy: &policy,
    reader: &reader,
    glossary: &queue.banned,
    teller: queue.teller(&db, &units, &policy, None),
    host: &queue.host,
    source: queue.source.clone(),
    tree: queue.tree.clone(),
    documents: queue.documents.clone(),
    testbed_logs: queue.testbed_logs.clone(),
    siren: queue.siren.clone(),
    owner_chat: queue.owner_chat,
    agent_key: queue.agent_key.clone(),
    agent_name: names.agent.clone(),
  };
  let cwd = args.cwd.clone().unwrap_or_else(|| queue.source.display().to_string());
  let _held = hold_locks(&queue.host, &cwd).await;

  job_run::run(args, &ground).await
}
