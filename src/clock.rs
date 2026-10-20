//! The time the service reads: the system's, or one a test moves by hand.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where the service reads the time, in epoch milliseconds.
#[derive(Debug, Clone)]
pub enum Clock {
  System,
  Manual(Arc<AtomicI64>),
}

impl Clock {
  /// A clock standing at `at` that moves only when it is told to.
  pub fn manual(at: i64) -> Self {
    Self::Manual(Arc::new(AtomicI64::new(at)))
  }

  pub fn now(&self) -> i64 {
    match self {
      Self::System => {
        let since = SystemTime::now().duration_since(UNIX_EPOCH).expect("the clock stands after the epoch");
        i64::try_from(since.as_millis()).expect("the time fits in 64 bits")
      }
      Self::Manual(at) => at.load(Ordering::SeqCst),
    }
  }

  /// Moves a manual clock on by `ms`; the system's moves itself.
  pub fn advance(&self, ms: i64) {
    if let Self::Manual(at) = self {
      at.fetch_add(ms, Ordering::SeqCst);
    }
  }
}
