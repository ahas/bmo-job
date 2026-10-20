//! What several bodies share: the caller's labels, and the three readings of a field in a merge patch.

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer};

/// The caller's own metadata on a member, which the service keeps and never reads.
pub type Labels = BTreeMap<String, String>;

/// A nullable field of a merge patch read in its three ways: left out (`None`), set to null (`Some(None)`),
/// or set to a value (`Some(Some(value))`). Used with `#[serde(default)]`, which gives the left-out case.
pub(crate) fn present<'de, T, D>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
  T: Deserialize<'de>,
  D: Deserializer<'de>,
{
  Option::<T>::deserialize(deserializer).map(Some)
}
