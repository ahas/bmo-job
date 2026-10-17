//! A job's prompt as the runner composes it before the work starts (docs/runs.md, "The prompt"): the worker's
//! preamble as its row — or its base's, for a support worker — stands now, the job's own text passed through
//! bmo-gate's sanitiser as it was handed, and the trailer the job is held to after it. The preamble and the
//! trailer stand outside the sanitising, since they are the service's own text and the owner's setting rather
//! than anybody's message. The knowledge the job's words call for stands between its text and its trailer,
//! outside the sanitising too, since it is the agent's own memory; the impact grounds the agent filed the item
//! with stand ahead of the job's text, outside it for the same reason.

use std::path::{Path, PathBuf};

use job_gate::{KnowledgeAsked, Piece, Sanitise};
use queue_read::impact::{ImpactGrounds, Verdict};
use queue_read::queue::Queue;
use queue_read::worker_lines::{Kept, line_reading, show_texts};
use sea_orm::QueryResult;

use super::Ground;
use crate::queue::units::WORKER_LANGUAGE;
use crate::{Refusal, one, run};

/// What the sanitiser files a job's prompt under.
pub const LABEL: &str = "the prompt of a job about to be run by a code-editing agent";

/// What the job's text is followed by: the trailer every job is held to, or what a step of a work order is
/// told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trailer {
  Job,
  WorkStep,
}

/// The branch a job works on, where it works on one: its name, the main it was made from, and whether the
/// worker's directory is a worktree of the worker's own.
pub struct Branching<'a> {
  pub branch: &'a str,
  pub main: &'a str,
  pub own_worktree: bool,
}

/// One job's prompt asked for.
pub struct Asked<'a> {
  pub ground: &'a Ground<'a>,
  pub worker: &'a str,
  pub chat_id: i64,
  pub task: &'a str,
  /// The knowledge block the job's words called for, where the gate picked any.
  pub knowledge: Option<&'a str>,
  /// The block of the impact grounds the job's item was filed with, where it carries any.
  pub impact: Option<&'a str>,
  pub trailer: Trailer,
  pub branching: Option<Branching<'a>>,
  /// Whether the worker's checkout stands on a work node, which is a checkout whatever this machine sees.
  pub remote: bool,
}

/// The prompt, the job's own text in the English the worker reads it in, and whether anything in that text
/// was defused on the way.
pub struct Composed {
  pub prompt: String,
  pub english: String,
  pub defused: bool,
}

// What a worker's row says of its bench.
struct Bench {
  name: String,
  preamble: Option<String>,
  dev_command: Option<String>,
  cwd: Option<String>,
  support_of: Option<String>,
  support_slot: Option<i64>,
}

impl Bench {
  fn read(row: &QueryResult) -> Result<Self, sea_orm::DbErr> {
    Ok(Self {
      name: row.try_get("", "name")?,
      preamble: row.try_get("", "preamble")?,
      dev_command: row.try_get("", "dev_command")?,
      cwd: row.try_get("", "cwd")?,
      support_of: row.try_get("", "support_of")?,
      support_slot: row.try_get("", "support_slot")?,
    })
  }

  async fn of(ground: &Ground<'_>, name: &str) -> Result<Option<Self>, Refusal> {
    let row = one(
      ground.db,
      r#"SELECT "name", "preamble", "dev_command", "cwd", "support_of", "support_slot" FROM "workers" WHERE "name" = $1"#,
      vec![name.into()],
    )
    .await?;

    Ok(row.map(|row| Bench::read(&row)).transpose()?)
  }
}

/// The line a job starts, restarts or stops its worker's dev server with, naming the platform's command line
/// by its full path, since a worker's checkout may be any repository.
pub fn dev_verb(bmo: &str, verb: &str, worker: &str) -> String {
  format!("`{bmo} dev-server {verb} {worker}`")
}

/// The platform's command line, as the job's text names it.
pub fn verbs_word(tree: &Path) -> String {
  tree.join("bin").join("bmo").to_string_lossy().into_owned()
}

// The owner's words in the preamble, in English as the worker reads them.
async fn english(ground: &Ground<'_>, text: &str) -> String {
  match ground.units.english(&[text.to_owned()]).await {
    Ok(english) => english.into_iter().next().unwrap_or_else(|| text.to_owned()),
    Err(why) => {
      eprintln!("bmo-job: the preamble stays as it was written: {}", why.error);
      text.to_owned()
    }
  }
}

// The folder each item's scratch stands in under a worker's workbench.
async fn item_scratches(ground: &Ground<'_>, worker: &str) -> Result<(PathBuf, PathBuf), Refusal> {
  let bench = ground.units.workbench(worker).await?;
  let scratch = bench.root.join(&ground.policy.job_prompt.workbench.scratch);

  Ok((bench.root, scratch))
}

/// The folder an item's own scratch stands in under a worker's workbench.
pub async fn item_scratch(ground: &Ground<'_>, worker: &str, todo: i64) -> Result<PathBuf, Refusal> {
  Ok(item_scratches(ground, worker).await?.1.join(todo.to_string()))
}

/// The block a job prompt opens with, or none where the worker has no row, or neither a preamble nor a dev
/// server and works in a checkout on no branch of its own.
async fn preamble(asked: &Asked<'_>) -> Result<Option<String>, Refusal> {
  let ground = asked.ground;
  let policy = ground.policy;
  let Some(row) = Bench::of(ground, asked.worker).await? else { return Ok(None) };
  let base = match &row.support_of {
    Some(base) => Bench::of(ground, base).await?,
    None => None,
  };
  let bench = base.as_ref().unwrap_or(&row);
  let checkout = match &bench.cwd {
    None => true,
    Some(cwd) => (asked.remote && base.is_none()) || Path::new(cwd).join(".git").exists(),
  };

  if bench.preamble.is_none() && bench.dev_command.is_none() && checkout && asked.branching.is_none() {
    return Ok(None);
  }

  let said = &policy.job_prompt.preamble;
  let fill = |line: &str, marks: &[(&str, &str)]| {
    let mut all = marks.to_vec();

    all.push(("dev_tab", &policy.job_prompt.words.dev_tab));
    policy.fill(line, &all)
  };
  let bmo = verbs_word(&ground.tree);
  let land = format!("{bmo} land");
  let (bench_root, scratch) = item_scratches(ground, asked.worker).await?;
  let bench_dir = bench_root.to_string_lossy().into_owned();
  let scratch = scratch.to_string_lossy().into_owned();
  let mut lines = vec![
    said.heading.clone(),
    fill(&said.workbench, &[("workbench", &bench_dir)]),
    fill(
      &said.item,
      &[("scratch", &scratch), ("database", &policy.job_prompt.workbench.database)],
    ),
  ];
  let support = base.as_ref().zip(row.support_slot);

  if let Some(branching) = &asked.branching {
    let (branch, main) = (branching.branch, branching.main);

    match support {
      Some((base, slot)) => {
        let slot = slot.to_string();

        lines.push(fill(
          &said.support,
          &[("slot", &slot), ("base", &base.name), ("branch", branch), ("main", main)],
        ));
        lines.push(fill(&said.support_commit, &[("branch", branch), ("main", main), ("land", &land)]));
      }
      None => {
        let (works, after) = if branching.own_worktree {
          (&said.own_worktree, &said.worktree_after)
        } else {
          (&said.own_checkout, &said.checkout_after)
        };
        let after = fill(after, &[("branch", branch), ("main", main)]);

        lines.push(fill(works, &[("branch", branch), ("main", main)]));
        lines.push(fill(
          &said.branch_commit,
          &[("branch", branch), ("main", main), ("land", &land), ("after", &after)],
        ));
      }
    }
  }

  if !checkout {
    lines.push(said.not_checkout.clone());
  }

  let log = Path::new(&bench_dir)
    .join(&policy.job_prompt.workbench.dev_log)
    .to_string_lossy()
    .into_owned();
  let verbs = ["start", "restart", "stop"].map(|verb| dev_verb(&bmo, verb, asked.worker));
  let used = [
    fill(&said.dev_use, &[("start", &verbs[0]), ("restart", &verbs[1]), ("stop", &verbs[2])]),
    said.dev_up.clone(),
  ];

  if let Some(command) = &bench.dev_command {
    // A support worker's own number only reaches its lines where its job works on a branch of its own.
    match (support, &asked.branching) {
      (Some((base, slot)), Some(_)) => {
        let slot = slot.to_string();

        lines.push(fill(
          &said.dev_support,
          &[("command", command), ("slot", &slot), ("base", &base.name), ("log", &log)],
        ));
        lines.extend(used);
        lines.push(fill(&said.dev_support_named, &[("base", &base.name)]));
      }
      _ => {
        lines.push(fill(&said.dev_own, &[("command", command)]));
        lines.push(fill(&said.dev_own_log, &[("log", &log)]));
        lines.extend(used);
      }
    }
  }

  let lines = lines.join("\n");

  Ok(Some(match &bench.preamble {
    Some(words) => format!("{lines}\n\n{}", english(ground, words).await),
    None => lines,
  }))
}

/// The trailer the job's text is followed by: the worker's own from its job documents, or for a step of a
/// work order the step's line and what it is told of the graph.
pub async fn trailer(ground: &Ground<'_>, worker: &str, which: Trailer) -> Result<String, Refusal> {
  let documents = ground.calls.job_documents(worker).await?;

  Ok(match which {
    Trailer::WorkStep => format!("{}\n\n{}", ground.policy.job_prompt.work_step.told, documents.graph),
    Trailer::Job => documents.trailer,
  })
}

/// What a later turn of the job is told, as the job's own text is: through the sanitiser, with the trailer the
/// job is held to after it. Where the sanitiser does not answer, the words go as they stand.
pub async fn later(ground: &Ground<'_>, chat_id: i64, task: &str, trailer: &str) -> Composed {
  let trimmed = task.trim();
  let asked = Sanitise {
    text: trimmed.to_owned(),
    label: LABEL.to_owned(),
    chat: Some(chat_id),
  };
  let cleaned = match ground.calls.sanitise(&asked).await {
    Ok(cleaned) => cleaned.text,
    Err(why) => {
      eprintln!("bmo-job: a later turn's prompt went past the sanitiser as it stands: {}", why.error);
      trimmed.to_owned()
    }
  };

  Composed {
    defused: cleaned != trimmed,
    prompt: format!("{cleaned}\n\n{trailer}"),
    english: cleaned,
  }
}

/// The item a job is run for, as far as its knowledge lines and its impact block read it.
pub struct JobItem {
  pub title: String,
  pub instruction: String,
  pub requester: String,
  pub requester_id: Option<i64>,
  pub impact: Option<ImpactGrounds>,
}

impl JobItem {
  fn read(row: &QueryResult) -> Result<Self, sea_orm::DbErr> {
    Ok(Self {
      title: row.try_get("", "title")?,
      instruction: row.try_get("", "instruction")?,
      requester: row.try_get("", "requester")?,
      requester_id: row.try_get("", "requester_id")?,
      impact: row
        .try_get::<Option<String>>("", "impact")?
        .and_then(|written| ImpactGrounds::read(&written).ok()),
    })
  }

  /// The item `todo` as its row stands, or none where it has no row.
  pub async fn of(ground: &Ground<'_>, todo: i64) -> Result<Option<Self>, Refusal> {
    let row = one(
      ground.db,
      r#"SELECT "title", "instruction", "requester", "requester_id", "impact" FROM "todos" WHERE "id" = $1"#,
      vec![todo.into()],
    )
    .await?;

    Ok(row.map(|row| JobItem::read(&row)).transpose()?)
  }
}

/// A message as one of a piece of work's lines: its sender by name and number, and its text.
pub fn message_line(sender_name: &str, sender_id: Option<i64>, text: &str) -> String {
  format!(
    "{sender_name} (id {}): {text}",
    sender_id.map_or_else(|| "unknown".to_owned(), |id| id.to_string())
  )
}

/// A quiet instruction as one of a piece of work's lines.
pub fn instruction_line(text: &str) -> String {
  format!("quiet instruction from the service: {text}")
}

/// A job's lines as the knowledge gate reads them: its item's title and request as its requester's line, or
/// the job's own text as a line of the service's where no item stands behind it.
pub fn job_lines(item: Option<&JobItem>, task: &str) -> Vec<String> {
  vec![match item {
    Some(item) => {
      message_line(&item.requester, item.requester_id, &format!("{}\n{}", item.title, item.instruction))
    }
    None => instruction_line(task.trim()),
  }]
}

/// The item the job is a run of, where it is one and its row reads; a row that cannot be read is said in the
/// log and stands as none, so the job is read by its own text.
pub async fn job_item(ground: &Ground<'_>, todo: Option<i64>) -> Option<JobItem> {
  JobItem::of(ground, todo?).await.unwrap_or_else(|why| {
    eprintln!("bmo-job: the job's item could not be read: {}", why.error);
    None
  })
}

/// The item `todo` as the header over a job's turns names it: its number and its title as the queue's
/// readings show a worker, the kept English or the title marked where none is kept. None where its row does
/// not read.
pub async fn item_heading(ground: &Ground<'_>, todo: i64) -> Option<String> {
  let title = one(ground.db, r#"SELECT "title" FROM "todos" WHERE "id" = $1"#, vec![todo.into()])
    .await
    .ok()
    .flatten()
    .and_then(|row| row.try_get::<String>("", "title").ok())?;
  let kept: Kept = ground
    .calls
    .kept(WORKER_LANGUAGE, std::slice::from_ref(&title))
    .await
    .unwrap_or_default();

  Some(format!("#{todo} {}", line_reading(&kept, &title)))
}

/// Every text of item `todo` the queue's readings would show a worker put into English and kept before the
/// job first asks for its item and before the header over its turns is made. A text the gate could not render
/// stays unkept, read under the mark, and nothing here keeps the job from starting.
pub async fn keep_item_english(ground: &Ground<'_>, todo: i64) {
  let texts = async {
    let mut queue = Queue::open(ground.db).await?;

    queue.read_item(ground.db, todo).await?;

    Ok::<_, Refusal>(queue.get_todo(todo).map(|item| show_texts(&queue, item)).unwrap_or_default())
  };

  match texts.await {
    Ok(texts) if !texts.is_empty() => {
      if let Err(why) = ground.units.english(&texts).await {
        eprintln!("bmo-job: item #{todo}'s English was not kept: {}", why.error);
      }
    }
    Ok(_) => {}
    Err(why) => eprintln!("bmo-job: item #{todo} could not be read for its English: {}", why.error),
  }
}

/// The knowledge a job's words call for, as the block its prompt carries, and the paths of the pieces in it.
pub struct Knowledge {
  pub block: Option<String>,
  pub paths: Vec<String>,
}

/// The picked pieces in the shape a turn's reference stands in, so nothing in them can pass for the work's own
/// instructions, under one line saying whose they are and which way they cut; `noun` is what the work is
/// called and `agent` whose memory they are.
pub fn knowledge_block(noun: &str, agent: &str, pieces: &[Piece]) -> Option<String> {
  if pieces.is_empty() {
    return None;
  }

  let mut lines = vec![format!(
    "Reference for this {noun} ({}): what {agent}'s memory holds on its matter — facts as they stand now and the rules every room keeps. Where a document or a comment says otherwise, this is what is true.",
    pieces.len()
  )];

  for piece in pieces {
    lines.push(format!("[{}]", piece.path));
    lines.push(piece.text.trim().to_owned());
  }

  lines.push("END OF REFERENCE".to_owned());

  Some(lines.join("\n"))
}

/// The knowledge the job's words call for, read for the room it reports to; a failure of the gate hands over
/// nothing.
pub async fn job_knowledge(
  ground: &Ground<'_>,
  chat_id: i64,
  item: Option<&JobItem>,
  task: &str,
) -> Knowledge {
  let asked = KnowledgeAsked {
    agent: ground.agent_key.clone(),
    chat: chat_id,
    lines: job_lines(item, task),
  };
  let pieces = ground.calls.knowledge(&asked).await.unwrap_or_else(|why| {
    eprintln!("bmo-job: no knowledge was picked for the job: {}", why.error);
    vec![]
  });

  Knowledge {
    block: knowledge_block("job", &ground.agent_name, &pieces),
    paths: pieces.into_iter().map(|piece| piece.path).collect(),
  }
}

/// The impact grounds an item was filed with, as the job reads them: each node and what changes on it, the new
/// home where one must be decided, and the verdict they come to.
pub fn impact_block(policy: &job_policy::Policy, grounds: &ImpactGrounds) -> String {
  let said = &policy.job_prompt.impact;
  let contracts = grounds.contract_changes().to_string();
  let nodes = grounds.nodes.iter().map(|node| match &node.contract {
    Some(contract) => policy.fill(&said.contract, &[("id", &node.id), ("contract", contract)]),
    None => policy.fill(&said.inside, &[("id", &node.id)]),
  });
  let verdict = match grounds.verdict() {
    Verdict::Single => &said.single,
    Verdict::Cowork => &said.cowork,
  };

  [policy.fill(&said.heading, &[])]
    .into_iter()
    .chain(nodes)
    .chain(grounds.new_home.then(|| said.new_home.clone()))
    .chain([policy.fill(verdict, &[("contracts", &contracts)])])
    .collect::<Vec<_>>()
    .join("\n")
}

/// The job's prompt: its preamble, its text as handed through the sanitiser, the knowledge its words call for,
/// and its trailer. The text goes to no translator: whoever hands a job its prompt writes it in English.
pub async fn compose(asked: &Asked<'_>) -> Result<Composed, Refusal> {
  let preamble = preamble(asked).await?;
  let trimmed = asked.task.trim();
  let cleaned = asked
    .ground
    .calls
    .sanitise(&Sanitise {
      text: trimmed.to_owned(),
      label: LABEL.to_owned(),
      chat: Some(asked.chat_id),
    })
    .await?
    .text;
  let trailer = trailer(asked.ground, asked.worker, asked.trailer).await?;

  Ok(Composed {
    defused: cleaned != trimmed,
    prompt: assembled(preamble.as_deref(), asked.impact, &cleaned, asked.knowledge, &trailer),
    english: cleaned,
  })
}

/// The text the worker was handed for the job's own text, written onto the job's row beside the prompt as it
/// was handed. A row that cannot take it holds nothing up: the log says so and the job goes on.
pub async fn keep_english(ground: &Ground<'_>, job_id: i64, english: &str) {
  let kept = run(
    ground.db,
    r#"UPDATE "jobs" SET "prompt_english" = $1 WHERE "id" = $2"#,
    vec![english.into(), job_id.into()],
  )
  .await;

  if let Err(why) = kept {
    eprintln!("bmo-job: the job's prompt in English could not be kept: {}", why.error);
  }
}

/// The prompt's parts in the order a job reads them: the preamble, the impact grounds its item was filed with,
/// the job's text, the knowledge its words called for, and the trailer.
pub fn assembled(
  preamble: Option<&str>,
  impact: Option<&str>,
  text: &str,
  knowledge: Option<&str>,
  trailer: &str,
) -> String {
  preamble
    .into_iter()
    .chain(impact)
    .chain([text])
    .chain(knowledge)
    .chain([trailer])
    .collect::<Vec<_>>()
    .join("\n\n")
}
