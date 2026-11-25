//! The job service's contract in Rust (docs/api.md, "The client").
//!
//! Every body the service takes and answers is a type here, every route a call of [`JobClient`], and the
//! OpenAPI document, `docs/openapi.json`, is generated from these same types by [`openapi::document`]. The
//! server answers in these types and the harness plugin calls through this client, so the three never
//! drift apart.

/// An enum carried on the wire as one of a fixed set of words, with every member listed and each read from
/// and written as its word.
macro_rules! wire_enum {
  ($(#[$meta:meta])* $name:ident { $($(#[$member_meta:meta])* $member:ident = $word:literal),+ $(,)? }) => {
    $(#[$meta])*
    #[derive(
      Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize, schemars::JsonSchema,
    )]
    pub enum $name {
      $($(#[$member_meta])* #[serde(rename = $word)] $member),+
    }

    impl $name {
      /// Every member, in the order the contract lists them.
      pub const ALL: &'static [Self] = &[$(Self::$member),+];

      /// The member as the wire writes it.
      pub fn as_str(self) -> &'static str {
        match self {
          $(Self::$member => $word),+
        }
      }

      /// The member a word names, or none.
      pub fn parse(word: &str) -> Option<Self> {
        match word {
          $($word => Some(Self::$member),)+
          _ => None,
        }
      }
    }

    impl std::fmt::Display for $name {
      fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
      }
    }
  };
}

mod client;
mod codes;
mod events;
mod fields;
mod health;
mod items;
mod label_values;
mod leases;
mod notes;
pub mod openapi;
mod page;
mod questions;
mod queues;
mod reports;
mod reviews;
mod routes;
mod scopes;
mod signature;
mod subscriptions;
mod work_orders;

pub use client::{ClientFailure, JobClient};
pub use codes::{Code, ErrorBody};
pub use events::{Event, EventKind, EventQuery, NewEvent};
pub use fields::Labels;
pub use health::Health;
pub use items::{
  Approval, ApprovalGiven, ApprovalKind, Item, ItemChange, ItemEvent, ItemHold, ItemQuery, ItemState,
  NewItem, NoticesOff, NoticesOffGiven,
};
pub use label_values::{LabelValues, LabelValuesBody, LabelValuesQuery};
pub use leases::{Frozen, ItemField, Lease, LeaseAsk, LeaseQuery, LeaseRenewal, LeaseWait};
pub use notes::{NewNote, Note, WaitQuery};
pub use page::{Page, PageQuery};
pub use questions::{
  NewQuestion, NewReply, Question, QuestionAddressee, QuestionChange, QuestionQuery,
  QuestionRecommendation, QuestionState, Reply, ReplySource,
};
pub use queues::{Hold, Queue, QueueChange, QueueQuery, QueueState};
pub use reports::{NewReport, Report, ReportKind};
pub use reviews::{NewReview, Review, ReviewKind, ReviewQuery};
pub use routes::{Method, Route, routes};
pub use scopes::{ScopeCounts, ScopeDeletion, ScopeHolding};
pub use signature::{SIGNATURE_HEADER, SUBSCRIPTION_HEADER, signature, signed_by};
pub use subscriptions::{
  NewSubscription, Subscription, SubscriptionChange, SubscriptionQuery, SubscriptionState,
};
pub use work_orders::{
  NewStep, Task, TaskAfter, TaskChange, TaskQuery, TaskState, WorkOrder, WorkOrderBody, WorkOrderChange,
  WorkOrderState,
};
