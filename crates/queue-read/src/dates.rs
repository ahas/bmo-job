//! An instant as a worker's lines write it: an ISO stamp in UTC to the millisecond.

use chrono::{DateTime, SecondsFormat};

/// `2026-10-05T04:17:26.000Z` for an instant in epoch milliseconds.
pub fn iso(ms: i64) -> String {
  DateTime::from_timestamp_millis(ms)
    .map_or_else(|| ms.to_string(), |at| at.to_rfc3339_opts(SecondsFormat::Millis, true))
}
