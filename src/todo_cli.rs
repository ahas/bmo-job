//! `bmo-job todo` from a shell (docs/cli.md): the closing a worker runs when it ends its item, through the
//! job service's own routes — a report for what the run tells of it, and the item's state set to done —
//! rather than any database of an older era.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bmo_job_client::{ItemChange, ItemState, JobClient, NewReport, Report, ReportKind};

use crate::Settings;

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
/// set to done, each through the route that holds it. Each answer is printed as its JSON on a line.
pub async fn close(client: &JobClient, args: &TodoArgs) -> Result<Vec<String>, String> {
  if args.done <= 0 {
    return Err("todo --done names the item by a positive number".to_owned());
  }

  let by = args.by.trim();
  if by.is_empty() {
    return Err("the closing names who writes it with --by, BMO_TURN_AGENT or USER".to_owned());
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
