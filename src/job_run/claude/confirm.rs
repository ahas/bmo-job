//! A confirmation the program stops on in the middle of a job's turn, put to the owner (docs/runs.md, "A
//! command the runner allows itself"). The question is an ordinary job question — the same row, the same
//! minute, the same stop of the clock, the same ways to answer — told as a pane's question so a reader can
//! tell it from one the job asked itself. The recommendation is the answer that carries the work on, and an
//! unanswered question settles there: allow, and close for a question this reading does not know. Only a
//! command that destroys what nothing here can restore is recommended refused. A command that stays inside
//! the job's own places is never put at all (`own_work`).

use std::sync::LazyLock;

use job_agency::{ConfirmAnswer, Confirmation};
use job_codes::job_questions::{PaneOption, PaneWindow};
use job_codes::{Fact, QuestionFact};
use regex::Regex;

use super::super::Ground;
use crate::Refusal;
use crate::now_ms;
use crate::queue::questions::{self, Answer, NewQuestion, Question};

/// The mark a pane's question is told under, so a reader can tell it from a job's own.
pub const PANE_MARK: &str = "pane";

/// The answer a question is settled with when the pane stopped asking before the answer came.
pub const ANSWERED_IN_PANE: &str = "answered in the pane";

/// What the run's log says of a command the runner allowed itself.
pub const OWN_WORK_NOTE: &str =
  "A command that stays inside the job's own places was allowed without asking";

// What a pane's question is asked for.
const WHY: &str = "The job's program stopped on a confirmation in the middle of its turn";

// What a question the reading does not know is shown as.
const UNKNOWN_TEXT: &str = "The program stopped on a question this reading does not know.";

// How long a question waits before the recommendation settles it.
const ASK_WAIT_MS: i64 = 60_000;

fn options(window: PaneWindow) -> [PaneOption; 2] {
  match window {
    PaneWindow::Permission => [PaneOption::Allow, PaneOption::Refuse],
    PaneWindow::Unknown => [PaneOption::Close, PaneOption::Leave],
  }
}

fn window_of(confirmation: &Confirmation) -> PaneWindow {
  match confirmation {
    Confirmation::Permission { .. } => PaneWindow::Permission,
    Confirmation::Unknown { .. } => PaneWindow::Unknown,
  }
}

// The commands an unanswered question must not let through: a recursive removal of the root or of a home
// directory itself, making a filesystem, a raw write over a device, a force push.
static DESTRUCTIVE: LazyLock<[Regex; 4]> = LazyLock::new(|| {
  [
    Regex::new(r#"\brm\s+(?:-\S+\s+)*(?:/|/\*|~/?|~/\*|"?\$HOME"?/?|/home/[^/\s]+/?|/root/?)(?:\s|$)"#)
      .expect("a destructive pattern"),
    Regex::new(r"\bmkfs\b").expect("a destructive pattern"),
    Regex::new(r"\bdd\b[^\n]*\bof=/dev/").expect("a destructive pattern"),
    Regex::new(r"\bgit\s+push\b[^\n]*(?:\s--force\b|\s-f\b|\s\+\S)").expect("a destructive pattern"),
  ]
});

/// Whether a command is one nothing here can restore after.
pub fn is_destructive(text: &str) -> bool {
  DESTRUCTIVE.iter().any(|pattern| pattern.is_match(text))
}

/// What a question is recommended answered with: the first option, refuse only for a command nothing
/// restores after.
pub fn recommended(confirmation: &Confirmation) -> i64 {
  match confirmation {
    Confirmation::Permission { text, .. } if is_destructive(text) => 1,
    _ => 0,
  }
}

/// What a refusal of the program's question carries: the owner's own words where he gave some.
fn refusal(words: Option<&str>) -> ConfirmAnswer {
  ConfirmAnswer::Refuse {
    words: words.map(str::to_owned),
  }
}

/// What a settled question tells the pane. Words of the owner's own are a refusal carrying them.
pub fn confirmation_answer(confirmation: &Confirmation, asked: &Question) -> ConfirmAnswer {
  if asked.answer.as_deref() == Some(ANSWERED_IN_PANE) {
    return ConfirmAnswer::Leave;
  }

  if let Some(answer) = &asked.answer {
    return refusal(Some(answer));
  }

  let chosen = asked.chosen.unwrap_or(asked.recommended);

  match confirmation {
    Confirmation::Permission { .. } if chosen == 0 => ConfirmAnswer::Allow,
    Confirmation::Permission { .. } => refusal(None),
    Confirmation::Unknown { .. } if chosen == 0 => refusal(None),
    Confirmation::Unknown { .. } => ConfirmAnswer::Leave,
  }
}

/// A pane's question put to the owner and waiting.
#[derive(Clone, Debug)]
pub struct Asking {
  /// The question's row.
  pub row: i64,
  /// The question the program stopped on, as bmo-agency names it.
  pub question: String,
  pub confirmation: Confirmation,
}

/// Who asks for a job, and what the asking stands on: the job's item and row.
#[derive(Clone, Copy, Debug)]
pub struct Asker {
  pub todo_id: Option<i64>,
  pub job_id: i64,
}

impl Asker {
  /// The question the program stopped on put to the owner, or the refusal it is answered with at once where
  /// the job has no item to hang it on, since waiting is the one thing such a run must not do.
  pub async fn ask(
    &self,
    ground: &Ground<'_>,
    question: &str,
    confirmation: &Confirmation,
  ) -> Result<Result<Asking, ConfirmAnswer>, Refusal> {
    let Some(todo) = self.todo_id else {
      return Ok(Err(refusal(None)));
    };
    let window = window_of(confirmation);
    let shown: Vec<String> =
      options(window).iter().map(|option| Into::<&str>::into(*option).to_owned()).collect();
    let text = match confirmation {
      Confirmation::Permission { text, .. } => text.clone(),
      Confirmation::Unknown { .. } => UNKNOWN_TEXT.to_owned(),
    };
    let now = now_ms();
    let opened = questions::open(
      ground.db,
      &NewQuestion {
        job_id: Some(self.job_id),
        todo_id: todo,
        chat_id: ground.owner_chat,
        question: &text,
        options: &shown,
        recommended: recommended(confirmation),
        why: WHY,
        asked_at: now,
        due_at: now + ASK_WAIT_MS,
        talks: false,
      },
    )
    .await?;

    ground
      .tell(Fact::Question(QuestionFact::PaneQuestion {
        item: todo,
        mark: PANE_MARK.to_owned(),
        window,
        options: options(window).to_vec(),
      }))
      .await;
    ground.screens_moved().await;

    Ok(Ok(Asking {
      row: opened.id,
      question: question.to_owned(),
      confirmation: confirmation.clone(),
    }))
  }

  /// The answer an asked question came to, where it has settled: by the owner, or by its minute running
  /// out on the recommendation.
  pub async fn settled(
    &self,
    ground: &Ground<'_>,
    asking: &Asking,
  ) -> Result<Option<ConfirmAnswer>, Refusal> {
    let Some(row) = questions::question(ground.db, asking.row).await? else {
      return Ok(Some(refusal(None)));
    };
    let row = if row.settled_at.is_none() && now_ms() >= row.due_at {
      questions::settle_unanswered(ground.db, asking.row, row.recommended, now_ms()).await?;
      questions::question(ground.db, asking.row).await?.unwrap_or(row)
    } else {
      row
    };

    if row.settled_at.is_none() {
      return Ok(None);
    }

    ground.tell(Fact::Question(row.settled(false, 0))).await;
    ground.screens_moved().await;

    Ok(Some(confirmation_answer(&asking.confirmation, &row)))
  }

  /// A question the pane stopped asking before anybody answered it, settled as answered there and withdrawn.
  pub async fn withdraw(&self, ground: &Ground<'_>, asking: &Asking) -> Result<(), Refusal> {
    let settled =
      questions::settle(ground.db, asking.row, &Answer::Words(ANSWERED_IN_PANE.to_owned()), now_ms())
        .await?;

    if settled && let Some(todo) = self.todo_id {
      ground.tell(Fact::Question(QuestionFact::PaneQuestionWithdrawn { item: todo })).await;
      ground.screens_moved().await;
    }

    Ok(())
  }
}
