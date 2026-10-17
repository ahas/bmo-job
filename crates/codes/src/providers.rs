//! The providers a job runs on: refusals of a run's provider and of the job model.

use axum::http::StatusCode;
use strum::{IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum ProviderRefusal {
  #[error("the run's provider is switched off")]
  ProviderOff,
  #[error("the provider serves one model alone, so it takes no co-worker")]
  SingleModelProvider,
  #[error("the job model {selection} is no tier, no provider and no model")]
  JobModelWrong { selection: String },
  #[error("no provider that is on offers a job model")]
  NoJobModel,
  #[error("the pin {value} chooses nothing that can run: {said}")]
  PinWrong { value: String, said: String },
}

coded!(ProviderRefusal);

impl Refused for ProviderRefusal {
  fn status(&self) -> StatusCode {
    match self {
      Self::JobModelWrong { .. } | Self::PinWrong { .. } => StatusCode::BAD_REQUEST,
      Self::ProviderOff | Self::SingleModelProvider | Self::NoJobModel => StatusCode::CONFLICT,
    }
  }
}
