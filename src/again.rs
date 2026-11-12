//! A call made again while its peer gives no answer (docs/cli.md, 「`bmo-job job close`」 and 「`bmo-job job
//! ask`」). A restart is no answer: only the peer's own answer, or the run told to stop, ends the call.

use std::future::Future;
use std::pin::Pin;
use std::time::Duration;

use bmo_job_client::ClientFailure;

/// The rest before a call is made again the first time, doubled after each try up to the longest.
const FIRST_REST: Duration = Duration::from_millis(250);
const LONGEST_REST: Duration = Duration::from_secs(5);

/// A failure that tells whether its peer answered at all.
pub trait Unanswered: std::fmt::Display {
  /// Whether the peer gave no answer, so the call is made again.
  fn is_unanswered(&self) -> bool;
}

impl Unanswered for ClientFailure {
  fn is_unanswered(&self) -> bool {
    ClientFailure::is_unanswered(self)
  }
}

/// What ended a call short of its answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halt<E> {
  /// The peer answered, with a failure.
  Answered(E),
  /// The run was told to stop.
  Stopped,
}

/// `call` made until its peer answers or `stop` ends. Each try that went unanswered is said on standard error,
/// opened by `who`.
pub async fn until_answered<T, E, F, Fut, S>(
  who: &str,
  mut stop: Pin<&mut S>,
  mut call: F,
) -> Result<T, Halt<E>>
where
  E: Unanswered,
  F: FnMut() -> Fut,
  Fut: Future<Output = Result<T, E>>,
  S: Future<Output = ()>,
{
  let mut rest = FIRST_REST;

  loop {
    let failure = tokio::select! {
      () = &mut stop => return Err(Halt::Stopped),
      called = call() => match called {
        Ok(answer) => return Ok(answer),
        Err(failure) if failure.is_unanswered() => failure,
        Err(failure) => return Err(Halt::Answered(failure)),
      },
    };

    eprintln!("{who}: {failure}; asking again");

    tokio::select! {
      () = &mut stop => return Err(Halt::Stopped),
      () = tokio::time::sleep(rest) => {}
    }

    rest = (rest * 2).min(LONGEST_REST);
  }
}
