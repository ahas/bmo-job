//! A worker's pin as the providers that would run it read it (docs/queue.md, "Workers"): `auto` for
//! none, a provider, or `<provider>::<model>` on a provider that takes a model of its own. Which
//! providers carry a worker's line and which models have an address are bmo-agency's to say, so every
//! value but `auto` is checked there.

use job_agency::PinChoice;

use super::units::Units;
use crate::Refusal;

/// The pin a value names; one that chooses nothing that can run is refused `pin_wrong`.
pub async fn pin_choice(units: &dyn Units, value: &str) -> Result<PinChoice, Refusal> {
  if value == "auto" {
    return Ok(PinChoice::Auto);
  }

  units.check_pin(value).await
}
