//! `bmo-job todo` from a shell (docs/cli.md): the closing a worker runs when it ends its item, the hold it leaves
//! its account on where it stops short, and the row it raises for what it found, all through the job service's
//! own routes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bmo_job_client::{ItemChange, ItemState, JobClient, NewItem, NewReport, Report, ReportKind};

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

/// The item closed: its unfinished note kept as a held report, its account as a final report, and its state
/// set to done, each through the route that holds it; an item whose commit waits to go live keeps its state.
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

  // An item whose commit waits for its deployment keeps its state: it is done once that commit is live.
  let stands = client
    .item(args.done)
    .await
    .map_err(|why| format!("item {} could not be read: {why}", args.done))?;

  if let Some(commit) = waiting_commit(&stands) {
    lines.push(
      serde_json::json!({ "itemId": args.done, "state": stands.state, "waitsOn": "live", "commit": commit })
        .to_string(),
    );

    return Ok(lines);
  }

  let change = ItemChange {
    state: Some(ItemState::Done),
    ..ItemChange::default()
  };
  let item = client
    .change_item(args.done, &change)
    .await
    .map_err(|why| format!("item {} was not closed: {why}", args.done))?;

  lines.push(serde_json::to_string(&item).unwrap_or_default());

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

/// What a job leaves on an item it stops short on: its account, and, for an item not yet held, why it holds it and
/// the condition that makes it ready.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StopHereArgs {
  pub item: i64,
  pub report: Option<String>,
  pub why: Option<String>,
  pub after: Option<String>,
  pub after_deploy: Option<String>,
  pub by: String,
}

/// The run's account left on the item as a held report, and the item held where it is not yet. A condition names
/// what makes it ready and releases nothing.
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

  if args.after.is_some() && args.after_deploy.is_some() {
    return Err("--after and --after-deploy name one condition between them".to_owned());
  }

  if let Some(refusal) = outside_refusal(&[account]) {
    return Err(refusal);
  }

  let why = args.why.as_deref().map(str::trim).filter(|why| !why.is_empty());
  let item = client
    .item(args.item)
    .await
    .map_err(|why| format!("item {} could not be read: {why}", args.item))?;
  let holds = item.state != ItemState::Held;

  if holds && why.is_none() {
    return Err(format!(
      "item {} is not held: --stop-here {} --why '<reason>' holds it",
      args.item, args.item
    ));
  }

  let kept = report(client, args.item, ReportKind::Held, account, by).await?;
  let mut lines = vec![serde_json::to_string(&kept).unwrap_or_default()];

  if holds {
    let mut labels = item.labels.clone();

    for (label, value) in [
      ("hold-why", why),
      ("hold-after", args.after.as_deref()),
      ("hold-after-deploy", args.after_deploy.as_deref()),
    ] {
      if let Some(value) = value {
        labels.insert(label.to_owned(), value.trim().to_owned());
      }
    }

    let change = ItemChange {
      state: Some(ItemState::Held),
      labels: Some(labels),
      ..ItemChange::default()
    };
    let held = client
      .change_item(args.item, &change)
      .await
      .map_err(|why| format!("item {} was not held: {why}", args.item))?;

    lines.push(serde_json::to_string(&held).unwrap_or_default());
  }

  Ok(lines)
}

/// A row a job raises out of its own work.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddArgs {
  pub title: String,
  pub judgement: String,
  pub raised_by: Option<i64>,
  pub from_todo: Option<String>,
  pub by: String,
}

/// The row raised in the queue and scope of the item it was raised from, a draft until an approver takes it.
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
  let raised = client
    .create_item(&NewItem {
      title: args.title.trim().to_owned(),
      brief: args.judgement.trim().to_owned(),
      queue_name: from.queue_name,
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
