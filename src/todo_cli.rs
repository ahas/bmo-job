//! `bmo-job todo` from a shell (docs/cli.md): the account a worker leaves on its item when it ends it or stops
//! short, and the row it raises for what it found, all through the job service's own routes. A run moves no
//! item's state: whoever ordered the work does.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bmo_job_client::{JobClient, NewItem, NewReport, Report, ReportKind, ReviewKind, ReviewQuery};

use crate::Settings;
use crate::job_close::waiting_commit;
use crate::outside::outside_refusal;

/// What a closing hands the service: the item, its report and what it could not reach, and the actor the
/// reports are kept under.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TodoArgs {
  pub done: i64,
  pub report: Option<String>,
  pub unfinished: Option<String>,
  pub by: String,
}

/// The client a verb reaches the job service with: the address and token the worker's run already carries
/// (docs/cli.md), else the deployment's settings beside the binary.
pub fn client() -> Result<JobClient, String> {
  let from_file = |name: &str| std::env::var(name).ok().map(|path| PathBuf::from(path.trim()));
  if let Some(url) = nonempty("BMO_JOB_URL") {
    let token = nonempty("BMO_JOB_TOKEN")
      .or_else(|| from_file("BMO_JOB_TOKEN_FILE").and_then(|path| std::fs::read_to_string(path).ok()))
      .map(|token| token.trim().to_owned())
      .filter(|token| !token.is_empty())
      .ok_or("BMO_JOB_URL names the job service but no BMO_JOB_TOKEN or BMO_JOB_TOKEN_FILE does")?;

    return JobClient::new(url.trim_end_matches('/'), token).map_err(|why| why.to_string());
  }

  let settings = Settings::load(&settings_place())?;
  let listen = settings.server.listen.trim();
  let listen = listen
    .strip_prefix("0.0.0.0:")
    .map(|rest| format!("127.0.0.1:{rest}"))
    .or_else(|| listen.strip_prefix("[::]:").map(|rest| format!("[::1]:{rest}")))
    .unwrap_or_else(|| listen.to_owned());
  let base = if listen.starts_with("http://") || listen.starts_with("https://") {
    listen
  } else {
    format!("http://{listen}")
  };

  JobClient::new(&base, settings.token()?).map_err(|why| why.to_string())
}

/// The settings file: the one the environment names, else `settings.toml` in the working directory, else the
/// release's own file beside the binary.
fn settings_place() -> PathBuf {
  let named = Settings::place();
  if named.exists() {
    return named;
  }

  std::env::current_exe()
    .ok()
    .and_then(|exe| exe.parent().map(Path::to_path_buf))
    .and_then(|bin| bin.parent().map(|release| release.join("settings.toml")))
    .filter(|beside| beside.exists())
    .unwrap_or(named)
}

fn nonempty(name: &str) -> Option<String> {
  std::env::var(name)
    .ok()
    .map(|value| value.trim().to_owned())
    .filter(|value| !value.is_empty())
}

/// Whether the item's review stands open (docs/api.md, "Reviews"): it holds an entry, and its last is not
/// `accepted`.
async fn review_open(client: &JobClient, item: i64) -> Result<bool, String> {
  let mut after = None;
  let mut last = None;

  loop {
    let query = ReviewQuery {
      after,
      limit: Some(500),
      ..ReviewQuery::default()
    };
    let page = client
      .reviews(item, &query)
      .await
      .map_err(|why| format!("item {item}'s review could not be read: {why}"))?;

    if let Some(entry) = page.items.last() {
      after = Some(entry.id);
      last = Some(entry.kind);
    }

    if !page.more {
      return Ok(last.is_some_and(|kind| kind != ReviewKind::Accepted));
    }
  }
}

/// The run's account of the finished work left on the item: its unfinished note kept as a held report and its
/// account as a final report. The item's state stays as it stands: it is done once whoever ordered the work takes
/// it — its review, or whoever puts a waiting commit live —, and the last line says which it waits on.
/// Each answer is printed as its JSON on a line.
pub async fn close(client: &JobClient, args: &TodoArgs) -> Result<Vec<String>, String> {
  if args.done <= 0 {
    return Err("todo --done names the item by a positive number".to_owned());
  }

  let by = args.by.trim();
  if by.is_empty() {
    return Err("the closing names who writes it with --by, BMO_TURN_AGENT or USER".to_owned());
  }

  let texts = [
    args.report.as_deref().unwrap_or_default(),
    args.unfinished.as_deref().unwrap_or_default(),
  ];

  if let Some(refusal) = outside_refusal(&texts) {
    return Err(refusal);
  }

  let mut lines = vec![];

  if let Some(unfinished) = args.unfinished.as_deref().map(str::trim).filter(|said| !said.is_empty()) {
    let kept = report(client, args.done, ReportKind::Held, unfinished, by).await?;

    lines.push(serde_json::to_string(&kept).unwrap_or_default());
  }

  if let Some(report) = args.report.as_deref().map(str::trim).filter(|said| !said.is_empty()) {
    let kept = report_line(client, args.done, ReportKind::Final, report, by).await?;

    lines.push(serde_json::to_string(&kept).unwrap_or_default());
  }

  let stands = client
    .item(args.done)
    .await
    .map_err(|why| format!("item {} could not be read: {why}", args.done))?;

  lines.push(
    match waiting_commit(&stands) {
      Some(commit) => {
        serde_json::json!({ "itemId": args.done, "state": stands.state, "waitsOn": "deployment", "commit": commit })
      }
      None if review_open(client, args.done).await? => {
        serde_json::json!({ "itemId": args.done, "state": stands.state, "waitsOn": "review" })
      }
      None => serde_json::json!({ "itemId": args.done, "state": stands.state, "waitsOn": "orderer" }),
    }
    .to_string(),
  );

  Ok(lines)
}

async fn report(
  client: &JobClient,
  item: i64,
  kind: ReportKind,
  body: &str,
  by: &str,
) -> Result<Report, String> {
  client
    .add_report(
      item,
      &NewReport {
        kind,
        body: body.to_owned(),
        actor_name: by.to_owned(),
        labels: BTreeMap::new(),
      },
    )
    .await
    .map_err(|why| format!("item {item} did not keep its report: {why}"))
}

async fn report_line(
  client: &JobClient,
  item: i64,
  kind: ReportKind,
  body: &str,
  by: &str,
) -> Result<Report, String> {
  report(client, item, kind, body, by).await
}

/// What a job leaves on an item it stops short on: its account of what is finished and where the next run picks
/// up.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StopHereArgs {
  pub item: i64,
  pub report: Option<String>,
  pub by: String,
}

/// The run's account left on the item as a held report. The item's state stays as it stands: holding it is its
/// orderer's.
pub async fn stop_here(client: &JobClient, args: &StopHereArgs) -> Result<Vec<String>, String> {
  let by = args.by.trim();
  if by.is_empty() {
    return Err("the account names who writes it with --by, BMO_TURN_AGENT or USER".to_owned());
  }

  let Some(account) = args.report.as_deref().map(str::trim).filter(|said| !said.is_empty()) else {
    return Err(
      "--stop-here takes --report '<what is finished and where the next run picks up>'".to_owned(),
    );
  };

  if let Some(refusal) = outside_refusal(&[account]) {
    return Err(refusal);
  }

  let kept = report(client, args.item, ReportKind::Held, account, by).await?;

  Ok(vec![serde_json::to_string(&kept).unwrap_or_default()])
}

/// A row a job raises out of its own work.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddArgs {
  pub title: String,
  pub judgement: String,
  pub raised_by: Option<i64>,
  /// The repository the row's work is in; the raising item's where none is named.
  pub repository: Option<String>,
  pub from_todo: Option<String>,
  pub by: String,
}

/// The row raised in the queue and scope of the item it was raised from, a draft until an approver takes it,
/// assigned to the repository named, else to the raising item's own.
pub async fn add(client: &JobClient, args: &AddArgs) -> Result<String, String> {
  if args.title.trim().is_empty() {
    return Err("--add takes --title '<title>'".to_owned());
  }

  if args.judgement.trim().is_empty() {
    return Err(
      "--add takes --judgement '<what the work is, why it was left and what to do about it>'".to_owned(),
    );
  }

  let Some(raised_by) = args.raised_by else {
    return Err("--add takes --raised-by <your item>".to_owned());
  };
  let from = client
    .item(raised_by)
    .await
    .map_err(|why| format!("item {raised_by} could not be read: {why}"))?;
  let mut labels = BTreeMap::new();

  if let Some(strands) = args.from_todo.as_deref().map(str::trim).filter(|strands| !strands.is_empty()) {
    labels.insert("from-todo".to_owned(), strands.to_owned());
  }

  let by = args.by.trim();
  let assignee = args
    .repository
    .as_deref()
    .map(str::trim)
    .filter(|repository| !repository.is_empty())
    .map(str::to_owned)
    .or(from.assignee_name);
  let raised = client
    .create_item(&NewItem {
      title: args.title.trim().to_owned(),
      brief: args.judgement.trim().to_owned(),
      queue_name: from.queue_name,
      assignee_name: assignee,
      requester_name: (!by.is_empty()).then(|| by.to_owned()),
      scope: from.scope,
      labels,
      raised_by: Some(raised_by),
      ..NewItem::default()
    })
    .await
    .map_err(|why| format!("the row was not raised: {why}"))?;

  Ok(serde_json::to_string(&raised).unwrap_or_default())
}
