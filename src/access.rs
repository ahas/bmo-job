//! Who a request is made by: the tokens the service takes, each naming its caller, whether a caller's approval
//! opens a draft, and whether it supervises the work (docs/api.md, "Access", "Approvals" and 「The addressee」;
//! docs/settings.md, `[callers]`).

use bmo_job_client::ApprovalKind;
use subtle::ConstantTimeEq;

/// The caller a request's token names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Caller {
  /// The caller's name in the settings, or none for the shared token.
  pub name: Option<String>,
  /// Whether the caller is an approver, whose approval opens a draft.
  pub approves: bool,
  /// Whether the caller supervises the work: it alone settles a question put to the supervisor, or puts one to a
  /// person.
  pub supervises: bool,
}

impl Caller {
  /// The caller of the shared token: no named caller, and no approver.
  pub fn shared() -> Self {
    Self {
      name: None,
      approves: false,
      supervises: false,
    }
  }

  /// A caller with a token of its own.
  pub fn named(name: &str, approves: bool) -> Self {
    Self {
      name: Some(name.to_owned()),
      approves,
      supervises: false,
    }
  }

  /// The same caller marked as supervising the work.
  pub fn supervising(self) -> Self {
    Self {
      supervises: true,
      ..self
    }
  }

  /// What an approval this caller gives counts as.
  pub fn approval_kind(&self) -> ApprovalKind {
    if self.approves { ApprovalKind::Approval } else { ApprovalKind::Recommendation }
  }
}

/// The tokens the service takes, each with the caller it names.
#[derive(Debug, Clone)]
pub struct Access {
  tokens: Vec<(String, Caller)>,
}

impl Access {
  /// The shared token alone.
  pub fn shared(token: &str) -> Self {
    Self {
      tokens: vec![(token.to_owned(), Caller::shared())],
    }
  }

  /// Adds a caller's own token. A token another entry already takes is refused, so a token names one caller.
  pub fn with(mut self, token: &str, caller: Caller) -> Result<Self, String> {
    if self.tokens.iter().any(|(taken, _)| taken == token) {
      let name = caller.name.as_deref().unwrap_or("the shared token");
      return Err(format!("the token of {name} is the token of another caller"));
    }
    self.tokens.push((token.to_owned(), caller));
    Ok(self)
  }

  /// The caller `given` names, or none. Every token is compared in constant time, whichever matches.
  pub fn caller(&self, given: &str) -> Option<&Caller> {
    let mut found = None;
    for (token, caller) in &self.tokens {
      if bool::from(given.as_bytes().ct_eq(token.as_bytes())) {
        found = Some(caller);
      }
    }
    found
  }
}
