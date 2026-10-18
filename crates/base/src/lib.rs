//! The library base every crate of bmo-job stands on: how a request is refused (docs/api.md), how a
//! statement runs on the database, the settings a deployment gives (docs/settings.md), and the clock
//! the service's own columns keep.

pub mod database;
pub mod refusal;
pub mod settings;

pub use database::{all, one, run, statement};
pub use refusal::{Refusal, RefusalBody};
pub use settings::{
  AbsolutePath, Address, Chat, Command, Deployment, Host, HttpAddress, Name, NodeSettings, ServerSettings,
  Settings, SettingsError, Units, WaitSeconds,
};

/// Now, in the epoch milliseconds the service's own columns keep.
pub fn now_ms() -> i64 {
  chrono::Utc::now().timestamp_millis()
}

/// Now, in the epoch seconds bmo-agency's run ledger keeps.
pub fn now_seconds() -> i64 {
  chrono::Utc::now().timestamp()
}
