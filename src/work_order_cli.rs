//! `bmo-job work-order` from a shell (docs/cli.md): a step added to an item's work order through the service's own
//! routes.

use bmo_job_client::{JobClient, WorkOrderChange};

/// A step added: its title, and the instruction it carries where one is given. The work order answered is printed
/// as its JSON.
pub async fn add_step(
  client: &JobClient,
  item: i64,
  title: &str,
  instruction: Option<&str>,
) -> Result<String, String> {
  let title = title.trim();

  if title.is_empty() {
    return Err("--add takes the step's title".to_owned());
  }

  let order = client
    .work_order(item)
    .await
    .map_err(|why| format!("item {item} has no work order to add a step to: {why}"))?;
  let step = match instruction.map(str::trim).filter(|said| !said.is_empty()) {
    Some(said) => format!("{title}: {said}"),
    None => title.to_owned(),
  };
  let mut steps = order.steps;

  steps.push(step);

  let change = WorkOrderChange {
    steps: Some(steps),
    ..WorkOrderChange::default()
  };
  let changed = client
    .change_work_order(item, &change)
    .await
    .map_err(|why| format!("the step was not added to item {item}'s work order: {why}"))?;

  Ok(serde_json::to_string(&changed).unwrap_or_default())
}
