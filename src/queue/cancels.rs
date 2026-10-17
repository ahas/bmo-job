//! A stopped run's cancellation taken back once it is gone (docs/queue.md, "What is running"). A cancel is
//! the signal that ends one run, never a verdict about a name for ever: while it stands every launch under
//! that name is refused, so it is dropped once the work it was raised over has ended. The cancels and what
//! one covers are bmo-agency's.

use super::units::Units;
use crate::Refusal;

/// Every cancel asked of this name, and of what it covers, taken back; the names actually dropped.
pub async fn take_back(units: &dyn Units, name: &str) -> Result<Vec<String>, Refusal> {
  units.drop_cancels(name).await
}
