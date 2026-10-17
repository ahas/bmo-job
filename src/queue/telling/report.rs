//! Where a job's end is told (docs/queue.md, "Telling a room"). The report is the worker's English to the
//! agent and carries the system, so the agent tells it to the owner in the owner's own chat; a room other
//! than that which asked for the work hears one voiced fact of what it came to. Nothing here sends the
//! report: the roads are written down for the job's ending, which carries the report to the agent.

use std::sync::LazyLock;

use job_codes::{Fact, JobFact};
use pulldown_cmark::{Event, Parser};
use regex::Regex;
use serde_json::json;

use super::Teller;
use crate::queue::outcomes::JOB_DONE;
use crate::queue::settings;
use crate::queue::voiced::{self, VoicedNotice};
use crate::{Refusal, one};

// The marks a runner leaves for the service running now.
const WAITING_REPORT: &str = "job_report_waiting:";
const REPORT_ROADS: &str = "job_report_roads:";

// What a sentence of a report says about the system rather than about the result: a commit, a test or a
// suite, a worker, a model or its provider, a command, the deployment; a file count; anything written as
// code; a bare commit hash; and a path.
static SYSTEM_TERMS: LazyLock<[Regex; 5]> = LazyLock::new(|| {
  [
    Regex::new(r"(?i)\b(commits?|tests?|suites?|deploy\w*|workers?|co-?workers?|runners?|checkouts?|worktrees?|branch\w*|main|claude|anthropic|opus|sonnet|haiku|fable|codex|openai|gpt[\w.-]*|gemini|openrouter|llama|qwen|models?|providers?|prompts?|sessions?|scripts?|mcp|git|npm|cargo|typecheck|lint\w*)\b")
      .expect("the system terms build"),
    Regex::new(r"(?i)\b(files?|documents?)\s*[0-9]|\b[0-9]+\s*(files?|documents?)\b").expect("the file count builds"),
    Regex::new("`").expect("the code mark builds"),
    Regex::new(r"\b[0-9a-f]{7,40}\b").expect("the hash builds"),
    Regex::new(r"[\w.-]+/[\w./-]+|\.(ts|rs|vue|md|json|toml|sh|py)\b").expect("the path builds"),
  ]
});

// The first block of a report as it was written: the text the markdown's first top-level block stands in.
fn first_block(report: &str) -> &str {
  Parser::new(report)
    .into_offset_iter()
    .find_map(|(event, range)| matches!(event, Event::Start(_)).then(|| report[range].trim()))
    .unwrap_or_default()
}

// A text's sentences: parted after a sentence's closing mark and the space that follows it, or at a line's
// end, each trimmed and the empty ones left out.
fn sentences(text: &str) -> Vec<&str> {
  let mut found = vec![];
  let mut start = 0;
  let mut previous: Option<char> = None;
  let mut characters = text.char_indices().peekable();

  while let Some((at, character)) = characters.next() {
    if !character.is_whitespace() {
      previous = Some(character);
      continue;
    }

    let mut end = at + character.len_utf8();
    let mut newline = character == '\n';

    while let Some(&(next_at, next)) = characters.peek() {
      if !next.is_whitespace() {
        break;
      }

      newline |= next == '\n';
      end = next_at + next.len_utf8();
      characters.next();
    }

    if newline || previous.is_some_and(|closing| matches!(closing, '.' | '!' | '?')) {
      found.push(&text[start..at]);
      start = end;
    }

    previous = text[..end].chars().next_back();
  }

  found.push(&text[start..]);
  found.into_iter().map(str::trim).filter(|sentence| !sentence.is_empty()).collect()
}

/// A report's summary, cut to the sentences that say what the work came to: a sentence naming the system,
/// or one of the deployment's own names, is left out whole.
pub fn outward_summary(report: &str, names: &[String]) -> String {
  let own: Vec<String> = names
    .iter()
    .map(|name| name.trim().to_lowercase())
    .filter(|name| name.chars().count() >= 2)
    .collect();

  sentences(first_block(report))
    .into_iter()
    .filter(|sentence| !SYSTEM_TERMS.iter().any(|term| term.is_match(sentence)))
    .filter(|sentence| {
      let lowered = sentence.to_lowercase();

      !own.iter().any(|name| lowered.contains(name.as_str()))
    })
    .collect::<Vec<_>>()
    .join(" ")
}

/// What the room that asked hears: the work finished or not, and what it came to, told by that room's
/// voice in its own words.
pub fn room_notice(report: &str, todo_id: Option<i64>, done: bool, names: &[String]) -> VoicedNotice {
  let summary = outward_summary(report, names);
  let fact = Fact::Job(JobFact::WorkReported {
    item: todo_id,
    done,
    summary: (!summary.is_empty()).then_some(summary),
  });
  let intent = if done {
    "tell this room that the work it asked for is done and what it came to."
  } else {
    "tell this room that the work it asked for did not get done."
  };

  VoicedNotice {
    follow_up: Some("The owner has the whole account in the owner's own chat. Say nothing of how it was built or checked: no commit, test or suite count, file count, worker, model or provider name, path, command or deployment.".into()),
    ..VoicedNotice::of(fact, intent)
  }
}

impl Teller<'_> {
  /// The companion that tells a job's ending, where the job's item was filed by a companion that still
  /// answers the room the job was launched for; none where the room's voice tells it.
  pub async fn companion_teller(&self, todo_id: Option<i64>, room: i64) -> Result<Option<String>, Refusal> {
    let Some(todo_id) = todo_id else { return Ok(None) };
    let filer = one(self.db, r#"SELECT "agent" FROM "todos" WHERE "id" = $1"#, vec![todo_id.into()])
      .await?
      .map(|row| row.try_get::<Option<String>>("", "agent"))
      .transpose()?
      .flatten();
    let Some(filer) = filer else { return Ok(None) };
    let answerers = self.units.answerers(room).await?;

    Ok(
      answerers
        .answerers
        .iter()
        .any(|one| one.agent == filer && one.companion)
        .then_some(filer),
    )
  }

  /// A finished job's report, delivered at `now` by the service running now: taken off the waiting list
  /// before anything is said, so it is told once however often it is asked for; the room that asked hears
  /// its line, and the roads are written down for the job's ending.
  pub async fn deliver_report(&self, job_id: i64, now: i64) -> Result<(), Refusal> {
    if settings::take(self.db, &format!("{WAITING_REPORT}{job_id}")).await?.is_none() {
      return Ok(());
    }

    let Some(job) = one(
      self.db,
      r#"SELECT "chat_id", "report", "outcome", "todo_id", "worker", "model", "execution_model" FROM "jobs" WHERE "id" = $1 AND "finished_at" IS NOT NULL"#,
      vec![job_id.into()],
    )
    .await?
    else {
      return Ok(());
    };
    let news = self.news_room(job.try_get("", "chat_id")?).await?;
    let asked = news.chat;
    let owner_room = self.units.room(asked).await?.owner_room;
    let todo_id: Option<i64> = job.try_get("", "todo_id")?;
    let companion = self.companion_teller(todo_id, asked).await?;
    // A companion's item is the companion's to tell, in the room it filed it in: that room hears the whole
    // of it from the companion and no line of it besides.
    let (report_chat, room) = match (&companion, owner_room) {
      (Some(_), _) | (None, true) => (asked, None),
      (None, false) => (self.owner_chat, Some(asked)),
    };
    let mut heard = None;

    if let Some(room) = room {
      // What names the deployment's own machinery beyond the words every report uses for it: the workers by
      // name and label, and the models the run was asked for and went out on.
      let mut names: Vec<String> = vec![job.try_get::<Option<String>>("", "worker")?.unwrap_or_default()];

      for model in [
        job.try_get::<Option<String>>("", "model")?,
        job.try_get::<Option<String>>("", "execution_model")?,
      ]
      .into_iter()
      .flatten()
      .filter(|model| !model.is_empty())
      {
        if !names.contains(&model) {
          names.push(model);
        }
      }

      for worker in self.workers().await? {
        names.push(worker.name);
        names.push(worker.label);
      }

      let report: String = job.try_get::<Option<String>>("", "report")?.unwrap_or_default();
      let done = job.try_get::<Option<String>>("", "outcome")?.as_deref() == Some(JOB_DONE);
      let notice = room_notice(&report, todo_id, done, &names);

      match voiced::queue(self.units, room, &notice, now).await {
        Ok(_) => heard = Some(room),
        Err(failed) => {
          eprintln!("bmo-job: job {job_id}'s line to the room that asked was not queued: {}", failed.error)
        }
      }
    }

    settings::set(
      self.db,
      &format!("{REPORT_ROADS}{job_id}"),
      &json!({ "chat": report_chat, "room": heard, "agent": companion, "removed_room": news.removed })
        .to_string(),
    )
    .await
  }

  /// Every report left waiting, oldest job first: the ones no service took as their jobs ended.
  pub(super) async fn deliver_waiting_reports(&self, now: i64) -> Result<(), Refusal> {
    for job_id in settings::numbered(self.db, WAITING_REPORT).await? {
      self.deliver_report(job_id, now).await?;
    }

    Ok(())
  }
}
