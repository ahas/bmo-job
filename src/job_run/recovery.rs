//! What a failed run says of its road, and the names a work order's task runs under (docs/queue.md, "Stops
//! and retries"): a window gone full, the service out, or no access, read off the failure's own words; and a
//! task's key once a round has carried it on.

use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

/// What a task's error opens with once every road its job could go out on has been tried.
pub const ROUTE_EXHAUSTED: &str = "job-route-exhausted:";

/// What a failed attempt says of its road.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoadFailure {
  Quota,
  Outage,
  Access,
}

impl RoadFailure {
  /// The word the failure is kept under.
  pub fn word(self) -> &'static str {
    match self {
      Self::Quota => "quota",
      Self::Outage => "outage",
      Self::Access => "access",
    }
  }
}

static QUOTA: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)ClaudeLimitError|usage limit|rate.?limit|quota|weekly limit|too many requests|\b429\b|insufficient_quota")
    .expect("the quota wording is a pattern")
});
static OUTAGE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)ClaudeOutageError|api unavailable|overloaded|service unavailable|\b50[0234]\b|ECONNRESET|ECONNREFUSED|ETIMEDOUT|fetch failed|network error|timed? ?out")
    .expect("the outage wording is a pattern")
});
static ACCESS: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(r"(?i)\b40[134]\b|unauthorized|authentication|invalid.api.key|model.*(?:not found|not available|does not exist|not supported)|ENOENT|not installed")
    .expect("the access wording is a pattern")
});

/// What a failure kept as its text alone says of its road, as a stopped task keeps its error; nothing for a
/// failure of anything else, which the walk does not carry past.
pub fn text_failure(said: &str) -> Option<RoadFailure> {
  if QUOTA.is_match(said) {
    Some(RoadFailure::Quota)
  } else if OUTAGE.is_match(said) || said.contains("SelectedRouteUnavailableError") {
    Some(RoadFailure::Outage)
  } else if ACCESS.is_match(said) {
    Some(RoadFailure::Access)
  } else {
    None
  }
}

/// The name a task of a work order runs under once a round has carried it on: the round's prefix before an
/// id that has none of its own.
pub fn task_key(order: &Value, task: &Value) -> String {
  static CARRIED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^r\d+-").expect("the carried pattern builds"));
  let round = order.get("round").and_then(Value::as_i64).unwrap_or(1);
  let id = task.get("id").and_then(Value::as_str).unwrap_or_default();

  if round > 1 && !CARRIED.is_match(id) {
    format!("r{round}-{id}")
  } else {
    id.to_owned()
  }
}

/// The step a work order's unit names: its task's key without the attempt.
pub fn unit_step(unit: &str) -> Option<String> {
  static STEP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^bmo-work-\d+-(.+)-\d+$").expect("the step pattern builds"));

  STEP.captures(unit).and_then(|read| read.get(1)).map(|step| step.as_str().to_owned())
}

/// The root a run's executions are counted under: a work order's unit without its attempt, and a task unit's
/// own name; none for anything else.
pub fn execution_root(owner: &str) -> Option<String> {
  if let Some(rest) = owner.strip_prefix("bmo-work-")
    && let Some((head, tail)) = rest.rsplit_once('-')
    && !head.is_empty()
    && !tail.is_empty()
    && tail.bytes().all(|byte| byte.is_ascii_digit())
  {
    return Some(format!("bmo-work-{head}"));
  }

  owner.starts_with("bmo-task-").then(|| owner.to_owned())
}
