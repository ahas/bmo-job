//! Whose work an item's job is, and whether that user may have it run (docs/queue.md, "Whose work a job
//! is"): the platform's own work where the owner filed it, nobody linked filed it, it was approved on a
//! standing rule or the account model is not in force; otherwise the user, or what they lack — the
//! worker's `run`, the repository's `change`, or a Claude account of their own turned on to carry it. The
//! users and their grants are bmo-web's, the Claude accounts bmo-agency's.

use job_web::{ObjectCheck, ObjectKind};
use queue_rows::todos::Approval;

use super::units::Units;
use crate::Refusal;

/// What a user lacks to have an item's work run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lack {
  /// A grant bmo-web holds no answer yes to, as `kind:object#relation`.
  Grant { user: String, grant: String },
  /// A Claude account of their own, turned on.
  ClaudeAccount { user: String },
}

impl std::fmt::Display for Lack {
  fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Grant { user, grant } => {
        write!(out, "the item's requester, user {user}, does not hold {grant}")
      }
      Self::ClaudeAccount { user } => write!(
        out,
        "the item's requester, user {user}, has no Claude account of their own turned on to run it"
      ),
    }
  }
}

/// Whose work a job is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Requester {
  /// The platform's own work, which runs on what the platform holds.
  Platform,
  /// A user's, who holds what it takes.
  User(String),
  /// A user's, who does not, so the item waits.
  Waits(Lack),
}

/// Whose work an item filed by this sender and approved this way is, for the worker it would run on.
pub async fn requester_run(
  units: &dyn Units,
  owner_sender: i64,
  requester_id: Option<i64>,
  approval: Option<Approval>,
  worker: &str,
) -> Result<Requester, Refusal> {
  let Some(sender) = requester_id.filter(|sender| *sender != owner_sender) else {
    return Ok(Requester::Platform);
  };

  // A standing rule is the owner's own writing, and whether the request fell within its words was judged
  // as it was approved: the work runs on the owner's authority, as the item recorded it.
  if approval == Some(Approval::Rule) || !units.model_in_force().await? {
    return Ok(Requester::Platform);
  }

  let Some(user) = units.user_of_sender(sender).await? else {
    return Ok(Requester::Platform);
  };

  for (kind, relation) in [(ObjectKind::Worker, "run"), (ObjectKind::Repository, "change")] {
    let check = ObjectCheck {
      kind,
      object: worker.to_owned(),
      relation: relation.to_owned(),
      user: user.clone(),
    };
    let answer = units.check_object(&check).await?;

    if !answer.allowed {
      return Ok(Requester::Waits(Lack::Grant {
        user,
        grant: answer.grant,
      }));
    }
  }

  if !units.claude_account_held(&user).await? {
    return Ok(Requester::Waits(Lack::ClaudeAccount { user }));
  }

  Ok(Requester::User(user))
}
