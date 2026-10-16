//! The records a workflow leaves, which a branch is split out of: refusals where none is found.

use axum::http::StatusCode;
use strum::{IntoStaticStr, VariantNames};

use crate::Refused;

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error, IntoStaticStr, VariantNames)]
#[strum(serialize_all = "snake_case")]
pub enum BranchSplitRefusal {
  #[error("no workflow record is left under the project folder {project}")]
  NoWorkflowRecords { project: String },
  #[error("the workflow {workflow} has no record under the project folder {project}")]
  WorkflowNotFound { workflow: String, project: String },
}

coded!(BranchSplitRefusal);

impl Refused for BranchSplitRefusal {
  fn status(&self) -> StatusCode {
    StatusCode::NOT_FOUND
  }
}
