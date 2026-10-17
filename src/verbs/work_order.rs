//! `work-order`: one item's work order read, and every change to one asked of the server (docs/verbs.md, "A
//! work order read and changed"). The read is the order the database holds, or the draft an item with none
//! would start from, in lines as `queue-read` writes them. A change is never written from here: the runner
//! holds an order across the awaits of a launch and would write its own copy over it, so the request goes to
//! the server's `POST /work-order` and is made in the order's lock. A server that does not answer is a
//! refusal.

use std::time::Duration;

use serde_json::{Map, Value};

use super::stand::{Answer, Stand};
use crate::queue::holds::{deploy_held, queue_paused};

/// The verb's whole usage.
pub const USAGE: &str = "usage: work-order --todo <id> [--json] [--reopen] [--intent <intent> --instruction <instruction> --model <model>] [--resume] [--task <task> --instruction <instruction> [--model <model>]] [--skip <task> --why <reason>] [--restore <task>] [--rerun <task> [--instruction <instruction>] [--model <model>]] [--move <task> --before|--after <task>] [--add <title> --instruction <instruction> [--before|--after <task>]] [--close --why <reason>] [--recover <first job>-<last job>] [--checkout <path>]";

/// What a refusal of the words ends with, pointing at the usage.
pub const HINT: &str = "Run work-order --help for the whole usage.";

// The verbs that change an order, one to a call.
const VERBS: [&str; 11] = [
  "--resume",
  "--reopen",
  "--task",
  "--skip",
  "--restore",
  "--rerun",
  "--move",
  "--add",
  "--close",
  "--recover",
  "--checkout",
];

// The runner may be launching a step of this very order; the request waits for it rather than being written
// beside it, and that wait is the launch's length.
const EDIT_WAIT: Duration = Duration::from_secs(180);

// How much of an order's lines are printed before the rest is asked for as the structure.
const LINES_CEILING: usize = 20_000;

/// The verb's flags as the request the server is asked: one verb a call, and no verb and no field a read,
/// which is answered from the database without asking the server anything. A flag lacking its value, two verbs
/// together and a job range that does not read are refused by what they are.
pub fn request_from_args(id: i64, words: &[String]) -> Result<Option<Value>, String> {
  let value = |flag: &str| -> Result<Option<String>, String> {
    let Some(at) = words.iter().position(|word| word == flag) else { return Ok(None) };

    match words.get(at + 1) {
      Some(next) if !next.starts_with("--") => Ok(Some(next.clone())),
      _ => Err(format!("{flag} needs its value")),
    }
  };
  let named: Vec<&str> = VERBS.into_iter().filter(|verb| words.iter().any(|word| word == verb)).collect();

  if named.len() > 1 {
    return Err(format!("one change a call, and this names {}", named.join(", ")));
  }

  let intent = value("--intent")?;
  let instruction = value("--instruction")?;
  let model = value("--model")?;
  let mut request = Map::new();
  let mut put = |key: &str, value: Option<String>| {
    if let Some(value) = value {
      request.insert(key.to_owned(), value.into());
    }
  };
  let action = match named.first().copied() {
    Some("--resume") => "resume",
    Some("--reopen") => {
      put("intent", intent);
      put("instruction", instruction);
      put("model", model);
      "reopen"
    }
    Some("--task") => {
      put("task", value("--task")?);
      put("instruction", instruction);
      put("model", model);
      "edit"
    }
    Some("--skip") => {
      put("task", value("--skip")?);
      put("why", value("--why")?);
      "skip"
    }
    Some("--restore") => {
      put("task", value("--restore")?);
      "restore"
    }
    Some("--rerun") => {
      put("task", value("--rerun")?);
      put("instruction", instruction);
      put("model", model);
      "rerun"
    }
    Some("--move") => {
      put("task", value("--move")?);
      put("before", value("--before")?);
      put("after", value("--after")?);
      "move"
    }
    Some("--add") => {
      put("title", value("--add")?);
      put("instruction", instruction);
      put("before", value("--before")?);
      put("after", value("--after")?);
      "add"
    }
    Some("--close") => {
      put("why", value("--why")?);
      "close"
    }
    Some("--checkout") => {
      put("cwd", value("--checkout")?);
      "checkout"
    }
    Some(_) => {
      let range = value("--recover")?.unwrap_or_default();
      let (from, to) = range
        .split_once('-')
        .and_then(|(from, to)| Some((digits(from)?, digits(to)?)))
        .ok_or_else(|| {
          "--recover takes the first and the last job of the run, as <first>-<last>".to_owned()
        })?;

      request.insert("from".to_owned(), from.into());
      request.insert("to".to_owned(), to.into());
      "recover"
    }
    None if intent.is_none() && instruction.is_none() && model.is_none() => return Ok(None),
    None => {
      put("intent", intent);
      put("instruction", instruction);
      put("model", model);
      "save"
    }
  };

  request.insert("id".to_owned(), id.into());
  request.insert("action".to_owned(), action.into());

  Ok(Some(Value::Object(request)))
}

// A run of digits alone, as a number.
fn digits(text: &str) -> Option<u64> {
  (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
    .then(|| text.parse().ok())
    .flatten()
}

/// The words before anything is read or asked: the usage for help, or the item's number, or the refusal of
/// a number that is not one, which ends with the usage fault's code.
pub fn item_of(words: &[String]) -> Result<Option<i64>, Answer> {
  if matches!(words.first().map(String::as_str), Some("--help" | "-h" | "help")) {
    return Ok(None);
  }

  let given = words.iter().position(|word| word == "--todo").and_then(|at| words.get(at + 1));

  match given.and_then(|id| id.trim().parse::<i64>().ok()).filter(|id| *id >= 1) {
    Some(id) => Ok(Some(id)),
    None => {
      let what = given.map_or_else(String::new, |given| format!(" ('{given}' is not a number)"));

      Err(Answer {
        lines: vec![format!("work-order: --todo needs one item number{what}."), HINT.to_owned()],
        code: 2,
      })
    }
  }
}

// The structure whole, as `--json` asks for it.
fn structure(order: &Value) -> Vec<String> {
  vec![serde_json::to_string_pretty(order).unwrap_or_default()]
}

/// The order's lines under the ceiling, the rest asked for as the structure.
pub fn capped(lines: Vec<String>, id: i64) -> Vec<String> {
  let mut kept = vec![];
  let mut length = 0;

  for (at, line) in lines.iter().enumerate() {
    length += line.chars().count() + 1;

    if length > LINES_CEILING {
      kept.push(format!(
        "… {} more lines; read the rest with work-order --todo {id} --json.",
        lines.len() - at
      ));
      return kept;
    }

    kept.push(line.clone());
  }

  kept
}

// What holds the queue still after a resume, so the order is active again and nothing starts: the owner's
// pause first. A database that cannot be asked says nothing, since the resume itself was answered.
async fn queue_hold(stand: &Stand) -> Option<String> {
  if queue_paused(&stand.db).await.ok()? {
    return Some("the queue is paused by the owner, so nothing starts until he resumes it".to_owned());
  }

  deploy_held(&stand.db)
    .await
    .ok()?
    .then(|| "a deployment holds the queue, so nothing starts until it lets go".to_owned())
}

/// The verb on its words: the order read, or a change asked of the server.
pub async fn work_order(stand: &Stand, words: &[String]) -> Answer {
  let id = match item_of(words) {
    Ok(Some(id)) => id,
    Ok(None) => return vec![USAGE.to_owned()].into(),
    Err(refused) => return refused,
  };
  let json = words.iter().any(|word| word == "--json");
  let refused = |lines: Vec<String>, code: u8| Answer { lines, code };
  let request = match request_from_args(id, words) {
    Ok(request) => request,
    Err(why) => return refused(vec![format!("work-order: {why}"), HINT.to_owned()], 2),
  };
  let owner = &stand.names.owner;

  let Some(request) = request else {
    return match queue_read::work_order::read(&stand.db, id).await {
      Ok(Some(order)) => if json {
        structure(&order)
      } else {
        capped(queue_read::work_order::lines(&order, owner), id)
      }
      .into(),
      Ok(None) => refused(vec![format!("work-order: no item #{id}")], 1),
      Err(why) => refused(vec![format!("work-order: {why}")], 1),
    };
  };
  let order = match stand.ask_server("work-order", &request, EDIT_WAIT).await {
    Ok(order) => order,
    Err(why) => return refused(vec![format!("work-order: {why}")], 1),
  };
  let mut out = if json || !queue_read::work_order::is_order(&order) {
    structure(&order)
  } else {
    capped(queue_read::work_order::lines(&order, owner), id)
  };

  // Resuming one order while the queue's own progress is held writes it back to active and then nothing
  // starts, so the answer says which of the two is holding it.
  if request.get("action").and_then(Value::as_str) == Some("resume") {
    out.extend(queue_hold(stand).await);
  }

  out.into()
}
