//! A job's question answered or talked over by the owner, and its clock stopped and started again
//! (docs/api.md, "Questions"). The chat and the console settle the same row with the same conditional write, so the write
//! itself decides who settled it.

use job_codes::job_questions::QuestionClock;
use job_codes::{JobAskRefusal, JobError, QuestionAnswer};

use super::Built;
use crate::Refusal;
use crate::now_ms;
use crate::queue::questions::{self, Answer, Question, TALK_BY_OWNER};
use crate::verbs::{Done, Questions, Settling};

fn absent(id: i64) -> Refusal {
  JobError::from(JobAskRefusal::QuestionAbsent { id }).into()
}

impl Built {
  async fn question(&self, id: i64) -> Result<Question, Refusal> {
    questions::question(&self.db, id).await?.ok_or_else(|| absent(id))
  }
}

fn settled_already(question: &Question) -> QuestionAnswer {
  QuestionAnswer::QuestionSettledAlready {
    settled_by: question.settled_by.clone(),
    answer: Some(question.answer_text()),
  }
}

fn clock(question: &Question) -> QuestionClock {
  QuestionClock {
    paused_left: question.paused_left,
    due_at: Some(question.due_at),
  }
}

impl Questions for Built {
  async fn answer_question(&self, id: i64, settling: Settling) -> Done<QuestionAnswer> {
    let question = self.question(id).await?;
    let answer = match settling {
      Settling::Chosen(chosen) if usize::try_from(chosen).is_ok_and(|at| at < question.options.len()) => {
        Answer::Chosen(i64::from(chosen))
      }
      Settling::Chosen(_) => return Err(JobError::from(JobAskRefusal::QuestionAnswerWrong).into()),
      Settling::Words(words) => Answer::Words(words),
    };

    if !questions::settle(&self.db, id, &answer, now_ms()).await? {
      return Ok(settled_already(&self.question(id).await?));
    }

    self.events.queue_moved(None);

    Ok(QuestionAnswer::QuestionSettled)
  }

  async fn pause_question(&self, id: i64, paused: bool) -> Done<QuestionAnswer> {
    self.question(id).await?;

    let now = now_ms();
    let landed = if paused {
      questions::pause(&self.db, id, self.policy.queue.question_stop_seconds * 1000, now).await?
    } else {
      questions::resume(&self.db, id, now).await?
    };
    let after = self.question(id).await?;

    if !landed {
      return Ok(if after.settled_at.is_some() {
        settled_already(&after)
      } else if paused {
        QuestionAnswer::QuestionPausedAlready(clock(&after))
      } else {
        QuestionAnswer::QuestionNotPaused(clock(&after))
      });
    }

    self.events.queue_moved(None);

    Ok(if paused {
      QuestionAnswer::QuestionPaused(clock(&after))
    } else {
      QuestionAnswer::QuestionResumed(clock(&after))
    })
  }

  async fn talk_question(&self, id: i64, say: String) -> Done<QuestionAnswer> {
    let question = self.question(id).await?;

    if !question.talks {
      return Err(JobError::from(JobAskRefusal::QuestionTakesNoTalk { id }).into());
    }

    let hold_ms = self.policy.queue.question_stop_seconds * 1000;

    if !questions::talk(&self.db, id, TALK_BY_OWNER, &say, hold_ms, now_ms()).await? {
      return Err(JobError::from(JobAskRefusal::QuestionTalkSettled { id }).into());
    }

    self.events.queue_moved(None);

    Ok(QuestionAnswer::QuestionTalked(clock(&self.question(id).await?)))
  }
}
