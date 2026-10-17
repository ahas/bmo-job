//! What a failed turn was (docs/runs.md, "What a failed turn was"). A job's turn runs in the resident
//! program, so a turn that did not answer leaves no stream: what it left is the screen or the program's own
//! error, which bmo-agency hands back as the turn's evidence. That one text is judged here once — a full
//! window, a login gone, or the run having died — and the judgement is carried by name, the name its cause
//! is written under. A failure bmo-agency read itself comes as its own kind.

use std::sync::LazyLock;

use job_agency::{AccountFailure, RunEnding, TurnFailure};
use regex::Regex;

/// A failed turn, by the error it is known as.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
  /// The login's window is full, with the moment it resets where the run said one, and the login it is
  /// once the walk names the one the run went out on. `silenced` says the rest it wrote closed the last
  /// open login and nobody has told the owner yet.
  Limit {
    resets_at: Option<i64>,
    account: Option<String>,
    silenced: bool,
  },
  /// A failure of the login itself.
  Account {
    failure: AccountFailure,
    detail: String,
  },
  /// The run ended with no answer, carrying what was readable where it ended.
  Died {
    ending: RunEnding,
    said: Option<String>,
    message: String,
  },
  /// The run was waiting on a person before it could take any work.
  Person,
  /// The program would not get past the folder-trust question in the worker's own folder.
  Untrusted { folder: String, config: String },
  /// The run was stopped from outside while it was working.
  Stopped,
  /// The pane or the program the turn stood in failed under it, rather than the road or the work.
  Transport { message: String },
  /// The road answered and refused the request the turn sent, which is the work's failure and never the
  /// road's.
  Refused { message: String },
  /// The road the run was asked on is switched off.
  Unavailable {
    scope: String,
    mode: String,
    reason: String,
  },
  /// Anything else, by its error's name and message.
  Error { name: String, message: String },
  /// The error the last attempt of a walk raised, which the walk does not read as its road's.
  Attempt,
  /// Every road the walk could take was tried or is shut, with the selections it tried.
  Exhausted { attempted: Vec<String> },
}

impl From<TurnFailure> for Failure {
  fn from(failure: TurnFailure) -> Self {
    match failure {
      TurnFailure::Limit {
        resets_at,
        account,
        silenced,
      } => Self::Limit {
        resets_at,
        account,
        silenced,
      },
      TurnFailure::Account { failure, detail } => Self::Account { failure, detail },
      TurnFailure::Died {
        ending,
        said,
        message,
      } => Self::Died {
        ending,
        said,
        message,
      },
      TurnFailure::Person => Self::Person,
      TurnFailure::Untrusted { folder, config } => Self::Untrusted { folder, config },
      TurnFailure::Stopped => Self::Stopped,
      TurnFailure::Transport { message } => Self::Transport { message },
      TurnFailure::Refused { message } => Self::Refused { message },
      TurnFailure::Unavailable {
        scope,
        mode,
        reason,
      } => Self::Unavailable {
        scope,
        mode,
        reason,
      },
      TurnFailure::Error { name, message } => Self::Error { name, message },
    }
  }
}

impl Failure {
  /// A plain error with the name a thrown `Error` carries.
  pub fn error(message: impl Into<String>) -> Self {
    Self::Error {
      name: "Error".to_owned(),
      message: message.into(),
    }
  }

  /// Which of the login's failures this is, where it is one.
  pub fn account_failure(&self) -> Option<AccountFailure> {
    match self {
      Self::Account { failure, .. } => Some(*failure),
      Self::Limit { .. } => Some(AccountFailure::Limit),
      _ => None,
    }
  }

  /// The words the failure is known by.
  pub fn message(&self) -> String {
    match self {
      Self::Limit { .. } => "claude usage limit reached".to_owned(),
      Self::Account { detail, .. } => detail.clone(),
      Self::Died { message, .. } => message.clone(),
      Self::Person => "the run is waiting on a person before it can take any work".to_owned(),
      Self::Untrusted { folder, config } => format!(
        "Claude Code did not get past the folder-trust question: the folder {folder} was written down as trusted in {config} and the program started again, and the question still stood"
      ),
      Self::Stopped => "the run was stopped from outside while it was working".to_owned(),
      Self::Transport { message } | Self::Refused { message } => message.clone(),
      Self::Unavailable {
        scope,
        mode,
        reason,
      } if reason.is_empty() => {
        format!("{scope}: selected {mode} route did not answer")
      }
      Self::Unavailable {
        scope,
        mode,
        reason,
      } => {
        format!("{scope}: selected {mode} route did not answer: {reason}")
      }
      Self::Error { message, .. } => message.clone(),
      Self::Attempt => "the attempt's own error".to_owned(),
      Self::Exhausted { .. } => {
        "every work provider the run could take is shut, so the run stopped".to_owned()
      }
    }
  }

  /// The name the failure's error carries: its own, where the failure has a name of its own, and the plain
  /// error's otherwise.
  pub fn name(&self) -> &str {
    match self {
      Self::Person => "JobWaitingOnPerson",
      Self::Untrusted { .. } => "JobFolderUntrusted",
      Self::Stopped => "JobStopped",
      Self::Transport { .. } => "PaneTransportError",
      Self::Refused { .. } => "RequestRefusedError",
      Self::Unavailable { .. } => "SelectedRouteUnavailableError",
      Self::Error { name, .. } => name,
      _ => "Error",
    }
  }

  /// The failure as a cause is written of it: its name, then its words.
  pub fn cause(&self) -> String {
    format!("{}: {}", self.name(), self.message())
  }
}

// What the CLI says when the login is gone. Its own wording for it is the shape of the message, read
// beside the run's ending and the credential's own expiry.
static LOGIN_GONE: LazyLock<Regex> = LazyLock::new(|| {
  Regex::new(
    r"(?i)(?:please )?(?:run )?/?login\b|not (?:logged in|authenticated)|invalid api key|oauth token (?:has )?expired|oauth (?:access )?token (?:has been )?revoked|invalid bearer token",
  )
  .expect("the login wording is a pattern")
});

static LIMIT_WORDINGS: LazyLock<[Regex; 3]> = LazyLock::new(|| {
  [
    Regex::new(r"(?i)claude (?:ai )?usage limit reached").expect("a limit wording"),
    Regex::new(r"(?i)usage limit reached\|\d+").expect("a limit wording"),
    Regex::new(r"(?i)you'?ve (?:hit|reached) your [^\n]{0,40}limit").expect("a limit wording"),
  ]
});

static LIMIT_RESET: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"(?i)limit reached\|(\d{9,11})").expect("the reset wording is a pattern"));

/// Whether a run's words say its login's window is full.
pub fn mentions_limit(text: &str) -> bool {
  LIMIT_WORDINGS.iter().any(|pattern| pattern.is_match(text))
}

/// The moment a full window resets, in epoch seconds, where the run's words carry it.
pub fn limit_reset_from_text(text: &str) -> Option<i64> {
  LIMIT_RESET
    .captures(text)
    .and_then(|found| found.get(1))
    .and_then(|epoch| epoch.as_str().parse().ok())
}

/// How much of a failed run's output is kept, everywhere it is kept.
pub const FAILURE_TEXT_LIMIT: usize = 2000;

static CSI_ESCAPE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\x1b\[[0-9;:?]*[ -/]*[@-~]").expect("an escape pattern"));
static OS_ESCAPE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)").expect("an escape pattern"));
static LONE_ESCAPE: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"\x1b[@-Z\\-_]").expect("an escape pattern"));

// A line carrying no letter and no digit is a drawing, a rule or a row of dots, never a cause.
fn says_something(line: &str) -> bool {
  line.chars().any(char::is_alphanumeric)
}

fn stripped(raw: &str) -> String {
  let plain = CSI_ESCAPE.replace_all(raw, "");
  let plain = OS_ESCAPE.replace_all(&plain, "");
  let plain = LONE_ESCAPE.replace_all(&plain, "");

  plain.replace("\r\n", "\n").replace('\r', "\n")
}

// The last `limit` characters in UTF-16 units; the cut lands mid-line, so that half line is dropped unless
// dropping it would leave nothing.
fn tail_of(text: &str, limit: usize) -> String {
  let units: Vec<u16> = text.encode_utf16().collect();

  if units.len() <= limit {
    return text.to_owned();
  }

  let tail = String::from_utf16_lossy(&units[units.len() - limit..]);
  let line_break = tail.find('\n');

  match line_break {
    Some(at) if at != tail.len() - 1 => format!("…\n{}", &tail[at + 1..]),
    _ => format!("…{tail}"),
  }
}

/// What a failed run printed, shortened for a record: escape sequences gone, decoration lines gone, blank
/// runs closed up, and the last characters kept with a `…` where the head was dropped.
pub fn failure_text(raw: &str) -> String {
  let plain = stripped(raw);
  let said = plain
    .split('\n')
    .map(str::trim_end)
    .filter(|line| says_something(line))
    .collect::<Vec<_>>()
    .join("\n");
  let said = said.trim();

  tail_of(if said.is_empty() { plain.trim() } else { said }, FAILURE_TEXT_LIMIT)
}

/// Whether a run that exited says, in what its harness reads of it, that its login is gone.
pub fn login_gone(stderr: &str, code: i64) -> bool {
  LOGIN_GONE.is_match(&harness_failure(stderr, code))
}

// The harness's own reading of a process that exited: its last line said, cut short.
fn harness_failure(stderr: &str, code: i64) -> String {
  let tail = stderr.split('\n').map(str::trim).rfind(|line| !line.is_empty());

  match tail {
    None => format!("exit {code}"),
    Some(line) => format!("exit {code}: {}", line.chars().take(300).collect::<String>()),
  }
}

/// What a turn that did not answer comes to, read off the text it left as a run's error output on exit 1: a
/// login gone, by what the harness reads of it; a full window; or the run having died.
pub fn of_unanswered(evidence: &str, key: &str) -> Failure {
  let code = 1;

  if LOGIN_GONE.is_match(&harness_failure(evidence, code)) {
    return Failure::Account {
      failure: AccountFailure::Login,
      detail: format!("{key}: {}", failure_text(evidence)),
    };
  }

  let error_text = format!("{evidence}\n");

  if mentions_limit(&error_text) {
    return Failure::Limit {
      resets_at: limit_reset_from_text(&error_text),
      account: None,
      silenced: false,
    };
  }

  let said = failure_text(evidence);

  Failure::Died {
    ending: RunEnding::Exit { code },
    message: format!("claude exited with code {code}: {said}"),
    said: Some(said),
  }
}

static API_STATUS: LazyLock<Regex> =
  LazyLock::new(|| Regex::new(r"API Error:\s*(\d{3})").expect("an API status"));

/// What a turn whose program wrote its own error in the answer's place comes to: a full window, with the
/// reset it names; the service answering with a server error, which no other login would get past either;
/// or, read as a turn that did not answer, a login gone or the run having died.
pub fn of_errored(error: &str, key: &str) -> Failure {
  if mentions_limit(error) {
    return Failure::Limit {
      resets_at: limit_reset_from_text(error),
      account: None,
      silenced: false,
    };
  }

  if API_STATUS
    .captures(error)
    .and_then(|found| found[1].parse::<i64>().ok())
    .is_some_and(|status| status >= 500)
  {
    return Failure::Account {
      failure: AccountFailure::Outage,
      detail: failure_text(error),
    };
  }

  of_unanswered(error, key)
}
