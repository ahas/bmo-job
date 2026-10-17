//! A job's question to the owner in the middle of its work, put from its shell and waited on (docs/verbs.md,
//! "A job's question to the owner").
//!
//! Every other verb that runs apart returns at once. This one waits on purpose: the caller is a job holding
//! its checkout's lock, and its own run is what waits, so nothing else is held up and no second job starts in
//! that checkout meanwhile. A settled question comes back as one line — the chosen option's words, or the
//! owner's own — whichever way it was settled. Where the owner talks instead of choosing, the question stays
//! open: what he said comes back as `owner_talked`, and a talk answers him and waits on the same question
//! again.

use std::time::Duration;

use job_base::Refusal;
use job_codes::{Fact, JobAskAnswer, JobAskRefusal, JobError};
use queue_rows::closing::outside_addresses;
use queue_rows::todos::running_job_for;
use queue_rows::wording::{BannedTerm, banned_terms_in};

use super::stand::Stand;
use crate::now_ms;
use crate::queue::questions::{self, NewQuestion, Question, TALK_BY_JOB, Waited};

/// How long a question waits before the recommendation settles it.
pub const ASK_WAIT_MS: i64 = 60_000;

/// The exit code of a call that comes back with talk instead of an answer: zero is an answer, one a
/// refusal, and this one says the question is still open and the job has something to answer.
pub const TALK_EXIT: u8 = 3;

// How often a waiting job reads its question again.
const POLL: Duration = Duration::from_secs(1);

// The verb's name, as what it says on the error stream opens.
const VERB: &str = "ask";

/// A new question as the job asked it, before it is judged.
#[derive(Clone, Debug, Default)]
pub struct AskRequest {
  /// The room the job named, where it named one: the owner's private chat alone takes a question.
  pub room: Option<i64>,
  pub todo_id: Option<i64>,
  pub question: String,
  pub options: Vec<String>,
  /// The option the job would take by itself, as the chat is shown it: 1 is the first.
  pub recommended: Option<i64>,
  pub why: String,
}

/// The question put again in a talk: every option anew with the recommendation, and the question or the why
/// where they changed.
#[derive(Clone, Debug, Default)]
pub struct Revised {
  pub question: Option<String>,
  pub options: Vec<String>,
  pub recommended: Option<i64>,
  pub why: Option<String>,
}

/// How a wait came back to the job: the settled question's one line, or the owner's talk.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
  Settled(String),
  Talked(JobAskAnswer),
}

fn refused(refusal: JobAskRefusal) -> Refusal {
  JobError::from(refusal).into()
}

/// Why words meant for the owner cannot go out as they stand, or none where they can: every term in them the
/// owner set aside, then every address in them that hands him a page hosted outside the deployment's own
/// archive. The job writes them itself and nothing renders them on the way, so they are held where they are
/// written.
pub fn words_refusal(texts: &[&str], banned: &[BannedTerm]) -> Option<JobAskRefusal> {
  let terms = banned_terms_in(&texts.join("\n"), banned);

  if !terms.is_empty() {
    return Some(JobAskRefusal::QuestionBannedTerms {
      terms: terms.into_iter().map(|term| term.term).collect(),
    });
  }

  let mut addresses: Vec<String> = vec![];

  for address in texts.iter().flat_map(|text| outside_addresses(text)) {
    if !addresses.contains(&address) {
      addresses.push(address);
    }
  }

  (!addresses.is_empty()).then_some(JobAskRefusal::QuestionOutsideHosting { addresses })
}

/// What a question's wording must hold, judged alone: the question, its options, the recommendation by its
/// number among them, and why it is asked, none of it using a word the owner set aside or handing him a page
/// hosted outside.
pub fn wording_refusal(
  question: &str,
  options: &[String],
  recommended: Option<i64>,
  why: &str,
  banned: &[BannedTerm],
) -> Option<JobAskRefusal> {
  if question.trim().is_empty() {
    return Some(JobAskRefusal::QuestionText);
  }

  if options.len() < 2 {
    return Some(JobAskRefusal::QuestionOptions);
  }

  if options.iter().any(|option| option.trim().is_empty() || option.contains('\n')) {
    return Some(JobAskRefusal::QuestionOptionEmpty);
  }

  if !recommended.is_some_and(|number| number >= 1 && number <= options.len() as i64) {
    return Some(JobAskRefusal::QuestionRecommend {
      count: u32::try_from(options.len()).unwrap_or(u32::MAX),
    });
  }

  if why.trim().is_empty() {
    return Some(JobAskRefusal::QuestionWhy);
  }

  let texts: Vec<&str> = std::iter::once(question)
    .chain(options.iter().map(String::as_str))
    .chain([why])
    .collect();

  words_refusal(&texts, banned)
}

/// Why a question is turned away, or none for one that may be put. Everything here is refused before the row
/// is written and before the chat is told, since a question the owner cannot answer is worse than none.
pub fn ask_refusal(request: &AskRequest, owner_chat: i64, banned: &[BannedTerm]) -> Option<JobAskRefusal> {
  if request.room.is_some_and(|room| room != owner_chat) {
    return Some(JobAskRefusal::QuestionRoom);
  }

  if request.todo_id.is_none() {
    return Some(JobAskRefusal::QuestionItem);
  }

  wording_refusal(&request.question, &request.options, request.recommended, &request.why, banned)
}

/// What the waiting job is handed when the owner talks instead of choosing: his words as he wrote them.
pub fn heard(question: &Question, heard: &[questions::Talk]) -> JobAskAnswer {
  JobAskAnswer::OwnerTalked {
    id: question.id,
    heard: heard.iter().map(|line| line.text.clone()).collect(),
  }
}

/// Where a question is asked: the stand its row is written on and its room told from.
pub struct Asking<'a> {
  pub stand: &'a Stand,
}

impl Asking<'_> {
  // The clock a talk or a stop holds a question for.
  fn hold_ms(&self) -> i64 {
    self.stand.policy.queue.question_stop_seconds * 1000
  }

  async fn tell(&self, question: &Question, fact: job_codes::QuestionFact) {
    self.stand.tell(VERB, question.chat_id, Fact::Question(fact)).await;
  }

  // How a wait ended, as the job reads it: the settled question's one line, or the owner's talk. A settled
  // question is told to the room.
  async fn ended(&self, waited: Waited) -> Asked {
    match waited {
      Waited::Talk {
        question,
        heard: lines,
      } => Asked::Talked(heard(&question, &lines)),
      Waited::Settled {
        question, talked, ..
      } => {
        self.stand.moved(VERB).await;
        self.tell(&question, question.settled(talked, self.hold_ms())).await;

        Asked::Settled(question.answer_text())
      }
    }
  }

  /// A new question opened, put to the room and waited on for `wait_ms`. Which job asked is found from the
  /// item: its job still running is the one asking, and a question asked outside a job belongs to the item
  /// alone.
  pub async fn ask(&self, request: &AskRequest, wait_ms: i64) -> Result<Asked, Refusal> {
    let stand = self.stand;

    if let Some(refusal) = ask_refusal(request, stand.owner.chat, &stand.ground.banned) {
      return Err(refused(refusal));
    }

    let todo_id = request.todo_id.unwrap_or_default();
    let job_id = running_job_for(&stand.db, todo_id).await?;
    let now = now_ms();
    let opened = questions::open(
      &stand.db,
      &NewQuestion {
        job_id,
        todo_id,
        chat_id: stand.owner.chat,
        question: &request.question,
        options: &request.options,
        recommended: request.recommended.unwrap_or(1) - 1,
        why: &request.why,
        asked_at: now,
        due_at: now + wait_ms,
        talks: true,
      },
    )
    .await?;

    stand.moved(VERB).await;
    // A question the room never saw still stands: the wait runs, and the recommendation settles it.
    self.tell(&opened, opened.asked(None)).await;

    let waited = questions::wait_on(&stand.db, opened.id, now + wait_ms, None, POLL).await?;

    // Said once, as the talk opens: every later wait is already inside it.
    if let Waited::Talk { question, .. } = &waited {
      stand.moved(VERB).await;
      self.tell(question, question.talk_held()).await;
    }

    Ok(self.ended(waited).await)
  }

  /// The job's answer in a talk put in the room, the question put again where the answer changed it, and the
  /// same question waited on again. A question settled while the job was writing is not answered into: the
  /// wait finds it settled at once and the room is told what it was settled on instead.
  pub async fn talk(
    &self,
    question_id: Option<i64>,
    say: &str,
    revised: Option<Revised>,
  ) -> Result<Asked, Refusal> {
    let stand = self.stand;
    let Some(id) = question_id else {
      return Err(refused(JobAskRefusal::QuestionTalkNumber));
    };
    let Some(asked) = questions::question(&stand.db, id).await? else {
      return Err(refused(JobAskRefusal::QuestionAbsent { id }));
    };

    if !asked.talks {
      return Err(refused(JobAskRefusal::QuestionTakesNoTalk { id }));
    }

    if say.trim().is_empty() {
      return Err(refused(JobAskRefusal::QuestionTalkSay));
    }

    if let Some(refusal) = words_refusal(&[say], &stand.ground.banned) {
      return Err(refused(refusal));
    }

    let revised = match revised {
      None => false,
      Some(revised) => {
        let question = revised.question.unwrap_or_else(|| asked.question.clone());
        let why = revised.why.unwrap_or_else(|| asked.why.clone());

        if let Some(refusal) =
          wording_refusal(&question, &revised.options, revised.recommended, &why, &stand.ground.banned)
        {
          return Err(refused(refusal));
        }

        let recommended = revised.recommended.unwrap_or(1) - 1;

        questions::revise(&stand.db, asked.id, &question, &revised.options, recommended, &why).await?
      }
    };
    let current = questions::question(&stand.db, asked.id).await?.unwrap_or(asked);
    let hold_ms = self.hold_ms();

    if questions::talk(&stand.db, current.id, TALK_BY_JOB, say, hold_ms, now_ms()).await? {
      stand.moved(VERB).await;

      for fact in current.talk_line(say, revised, hold_ms) {
        self.tell(&current, fact).await;
      }
    }

    let waited = questions::wait_on(&stand.db, current.id, 0, None, POLL).await?;

    Ok(self.ended(waited).await)
  }
}
