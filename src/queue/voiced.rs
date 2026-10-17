//! A fact the service has to tell a room on its own account, voiced by the agent that speaks for the room
//! so it is said in the room's own voice (docs/queue.md, "Telling a room"). bmo-receiver frames it and
//! hands it to that agent's session; the service owns only the fact and what the line is for.

use job_codes::Fact;
use job_receiver::Notice;

use super::units::Units;
use crate::Refusal;

/// One notice: what happened, what the line is for, what follows it, and how its row is kept, its times
/// counted from the moment it is voiced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VoicedNotice {
  pub fact: Fact,
  pub intent: String,
  pub follow_up: Option<String>,
  /// Whether the voice may let it go unsaid where the fact leaves nothing to hear.
  pub optional: bool,
  pub kind: Option<String>,
  pub ttl_ms: Option<i64>,
  pub events: Vec<String>,
  pub rides_ms: Option<i64>,
}

impl VoicedNotice {
  /// A notice of a fact, said for this reason, with nothing else to it.
  pub fn of(fact: Fact, intent: &str) -> Self {
    Self {
      fact,
      intent: intent.to_owned(),
      follow_up: None,
      optional: false,
      kind: None,
      ttl_ms: None,
      events: vec![],
      rides_ms: None,
    }
  }

  /// The notice as bmo-receiver takes it, voiced at `now`.
  pub fn notice(&self, now: i64) -> Notice {
    Notice {
      fact: self.fact.clone(),
      intent: self.intent.trim().to_owned(),
      follow_up: self
        .follow_up
        .as_deref()
        .map(str::trim)
        .filter(|follow_up| !follow_up.is_empty())
        .map(str::to_owned),
      optional: self.optional,
      kind: self.kind.clone(),
      expires_at: self.ttl_ms.map(|ttl| now + ttl),
      events: self.events.clone(),
      rides_until: self.rides_ms.map(|ride| now + ride),
    }
  }
}

/// The notice voiced in the room at `now`; its instruction's id.
pub async fn queue(units: &dyn Units, chat: i64, notice: &VoicedNotice, now: i64) -> Result<i64, Refusal> {
  units.voice_notice(chat, &notice.notice(now)).await
}
