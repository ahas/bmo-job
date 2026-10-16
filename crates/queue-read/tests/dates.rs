//! An instant as a worker's lines write it.

use queue_read::dates::iso;

#[test]
fn an_instant_reads_as_an_iso_stamp_in_utc_to_the_millisecond() {
  assert_eq!(iso(1_759_637_846_000), "2025-10-05T04:17:26.000Z");
  assert_eq!(iso(1_759_637_846_500), "2025-10-05T04:17:26.500Z");
  assert_eq!(iso(i64::MAX), i64::MAX.to_string());
}
