//! The agent's answer to a run that reached its clock, from a shell (docs/verbs.md, "A run past its clock,
//! answered"): the asks still open, and keep or cut on one of them. The run's own clock reads the answer on
//! its next look, within seconds.

use sea_orm::DatabaseConnection;

use super::stand::{VerbError, clock};
use crate::queue::overtime::{LONGEST_KEEP_MS, spoken};
use crate::{all, one};

/// What `overtime` takes.
pub const USAGE: &str = "usage: overtime --list | --keep <id> [--for <length>] [--why '<one line>'] | --cut <id> [--why '<one line>']";

// The run's own clock closes its line within seconds of either moment, so a line still open a minute past
// it is one nobody watches any more: the service restarted while the run was asked.
const LAPSE_MS: i64 = 60_000;

fn refused(why: impl Into<String>) -> VerbError {
  VerbError::Refused(why.into())
}

/// The asks still open, oldest first, each with what it is and where its answer stands, read against
/// `now`.
pub async fn list(db: &DatabaseConnection, now: i64) -> Result<Vec<String>, VerbError> {
  let rows = all(
    db,
    r#"SELECT "id", "agent", "run", "what", "clock_ms", "asked_at", "answer_by", "verdict", "keep_until"
       FROM "clock_holds" WHERE "closed" IS NULL ORDER BY "id""#,
    vec![],
  )
  .await?;

  if rows.is_empty() {
    return Ok(vec!["(no turn is waiting on an answer)".to_owned()]);
  }

  rows
    .iter()
    .map(|row| {
      let answer_by: i64 = row.try_get("", "answer_by")?;
      let verdict: Option<String> = row.try_get("", "verdict")?;
      let keep_until: Option<i64> = row.try_get("", "keep_until")?;
      let due = if verdict.as_deref() == Some("keep") {
        keep_until.unwrap_or(answer_by)
      } else {
        answer_by
      };
      let standing = if now > due + LAPSE_MS {
        "no longer watched (the service restarted while it was asked); nothing to decide".to_owned()
      } else {
        match verdict.as_deref() {
          None => format!("no answer yet; it is cut at {}", clock(answer_by)),
          Some("keep") => format!("kept until {}", clock(due)),
          Some(_) => "cut, being carried out".to_owned(),
        }
      };

      Ok(format!(
        "#{} {} {}: {}\n    asked {} after {}; {standing}",
        row.try_get::<i64>("", "id")?,
        row.try_get::<String>("", "agent")?,
        row.try_get::<String>("", "run")?,
        row.try_get::<String>("", "what")?,
        clock(row.try_get("", "asked_at")?),
        spoken(row.try_get("", "clock_ms")?)
      ))
    })
    .collect()
}

/// A length as the keep takes it: `20m`, `1h`, `90s` or a plain number of minutes, in seconds.
pub fn duration_seconds(text: &str) -> Option<i64> {
  let text = text.trim();
  let digits = text.find(|letter: char| !letter.is_ascii_digit()).unwrap_or(text.len());
  let (amount, unit) = text.split_at(digits);
  let amount: i64 = amount.parse().ok()?;
  let unit = match unit.trim_start().to_ascii_lowercase().as_str() {
    "" | "m" => 60,
    "h" => 3600,
    "s" => 1,
    _ => return None,
  };

  Some(amount * unit)
}

/// The answer given on one ask.
pub enum Verdict<'a> {
  Keep { for_given: Option<&'a str> },
  Cut,
}

/// Keep or cut on one ask, `id` as it was given, decided by `agent`. A later answer replaces an earlier one
/// while the run goes on, so a keep can still become a cut or be lengthened; a keep is never longer than the
/// longest one keep gives, and lasts the run's own clock where no length is given.
pub async fn answer(
  db: &DatabaseConnection,
  agent: &str,
  id: &str,
  verdict: Verdict<'_>,
  why: Option<&str>,
  now: i64,
) -> Result<Vec<String>, VerbError> {
  if id.is_empty() || !id.bytes().all(|byte| byte.is_ascii_digit()) {
    return Err(refused(USAGE));
  }

  let id: i64 = id.parse().map_err(|_| refused(USAGE))?;
  let for_ms = match verdict {
    Verdict::Keep {
      for_given: Some(given),
    } => match duration_seconds(given) {
      Some(seconds) if seconds > 0 => Some(seconds * 1000),
      _ => {
        return Err(refused(format!("--for takes 20m, 1h or a plain number of minutes, not \"{given}\"")));
      }
    },
    _ => None,
  };
  let why = why.map(str::trim).filter(|why| !why.is_empty());
  let Some(hold) =
    one(db, r#"SELECT "clock_ms" FROM "clock_holds" WHERE "id" = $1"#, vec![id.into()]).await?
  else {
    return Err(refused(format!("no ask #{id}")));
  };
  let keep_until = match verdict {
    Verdict::Cut => None,
    Verdict::Keep { .. } => {
      let for_ms = for_ms.unwrap_or(hold.try_get("", "clock_ms")?);

      if for_ms > LONGEST_KEEP_MS {
        return Err(refused(format!(
          "one keep gives at most {}; you are asked again when it runs out",
          spoken(LONGEST_KEEP_MS)
        )));
      }

      Some(now + for_ms)
    }
  };
  let word = if keep_until.is_some() { "keep" } else { "cut" };
  let decided = one(
    db,
    r#"UPDATE "clock_holds" SET "verdict" = $1, "keep_until" = $2, "decided_by" = $3, "why" = $4, "decided_at" = $5
       WHERE "id" = $6 AND "closed" IS NULL RETURNING "id""#,
    vec![word.into(), keep_until.into(), agent.into(), why.into(), now.into(), id.into()],
  )
  .await?;

  if decided.is_none() {
    return Err(refused(format!(
      "ask #{id} is already over: the turn ended on its own or was cut. There is nothing left to decide."
    )));
  }

  Ok(vec![match keep_until {
    None => format!("ask #{id}: cut; the turn is cut within seconds"),
    Some(until) => format!("ask #{id}: kept until {}", clock(until)),
  }])
}
