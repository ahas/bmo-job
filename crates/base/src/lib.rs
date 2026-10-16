//! The library base every crate of bmo-job stands on: how a request is refused (docs/api.md), how a
//! statement runs on the database, the settings a deployment gives (docs/settings.md), and the clock
//! the service's own columns keep.

pub mod database;
pub mod refusal;
pub mod settings;

pub use database::{all, one, run, statement};
pub use refusal::{Refusal, RefusalBody};
pub use settings::{
  Address, Deployment, HttpAddress, Name, ServerSettings, Settings, SettingsError, Units,
};

/// Now, in the epoch milliseconds the service's own columns keep.
pub fn now_ms() -> i64 {
  chrono::Utc::now().timestamp_millis()
}
