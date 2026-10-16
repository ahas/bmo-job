//! A call on another unit that failed, refused with 502 under the unit's name (docs/api.md, "Routes").

use job_base::Refusal;
use job_codes::UnitRefusal;

use super::request::refused;

pub fn agency(failed: job_agency::AgencyError) -> Refusal {
  refused(UnitRefusal::Agency {
    failure: failed.to_string(),
  })
}
