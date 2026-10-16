//! What a block of an item's words opens with.

use serde::{Deserialize, Serialize};
use strum::{Display, IntoStaticStr, VariantNames};

/// What a block opens with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, Display, IntoStaticStr, VariantNames)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum BlockHead {
  /// Words added while the item stood, `at` in epoch milliseconds.
  Note {
    note: i64,
    author: String,
    at: i64,
  },
  Judgement,
  Source {
    id: i64,
    title: String,
    strand: String,
  },
  RaisedByItem {
    id: i64,
  },
  FiledBy {
    filer: String,
  },
  FastTrack {
    tool: String,
  },
  Message {
    id: String,
  },
}
