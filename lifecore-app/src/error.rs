use crate::{
    proposal::{DecodeError, ProposalLocation},
    service::{ProposalReport, Published},
};
use lifecore::{EntryId, LifeCoreError};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("LIFECORE_STATE_FILE_PATH is set but empty")]
    EmptyStatePath,
    #[error("cannot determine the working directory: {message}")]
    WorkingDirectory { message: String },
    #[error("the directory {path} for a new database does not exist")]
    DirectoryMissing { path: PathBuf },
    #[error("no database at {path}; run `lifecore init` to create one")]
    DatabaseMissing { path: PathBuf },
    #[error("cannot read the database at {path}: {message}")]
    DatabaseUnreadable { path: PathBuf, message: String },
    #[error("a database already exists at {path}")]
    DatabaseExists { path: PathBuf },
    #[error("cannot use the lock file {path}: {message}")]
    Lock { path: PathBuf, message: String },
    #[error("another writer holds {lock}; try again")]
    Busy { lock: PathBuf },
    #[error(
        "publication failed before replacing the database; stored state is unchanged: {message}"
    )]
    PublicationFailed { message: String },
    // The new state is already visible, so the caller gets the result it would
    // have had and can inspect whether its write is there; see FILES 8.1.
    #[error(
        "the new state was published, but synchronization failed, so it may or may not persist: {message}"
    )]
    PublicationUncertain {
        message: String,
        outcome: Box<Published>,
    },
    #[error("malformed {format} proposal: {error}", format = .0.format, error = .0)]
    MalformedProposal(DecodeError),
    #[error("invalid proposal{}: {message}", location.as_ref().map(|at| format!(" at entry {} ({:?})", at.operation, at.name)).unwrap_or_default())]
    InvalidProposal {
        location: Option<ProposalLocation>,
        message: String,
    },
    #[error(
        "`acknowledges` is not supported yet; remove it to check or apply these entries as an ordinary proposal"
    )]
    AcknowledgmentUnsupported,
    #[error("the plan ID names database {expected}, but the selected database is {actual}")]
    ForeignDatabase { expected: Uuid, actual: Uuid },
    #[error("the plan ID base {expected} is stale; the current graph revision is {actual}")]
    StaleBase { expected: Uuid, actual: Uuid },
    #[error("analysis emitted blocking inconsistencies; nothing was stored")]
    Rejected(Box<ProposalReport>),
    #[error("analyzer failure: {0}")]
    Analyzer(LifeCoreError),
    #[error("entry {id:?} is not present in the selected view")]
    EntryNotFound { id: EntryId },
    #[error("the boundary {requested} is past the visible end {visible}")]
    BeyondBoundary { requested: usize, visible: usize },
    #[error("invalid parameters: {message}")]
    InvalidParams { message: String },
    // Raised by a host, not the service: the task running a call ended
    // without a response, for example by panicking. A write it was making may
    // or may not have been published, so this is never a rollback.
    #[error(
        "the request's task failed before it responded, so any write it was making may or may not have been published: {message}"
    )]
    Backend { message: String },
    #[error("internal error: {message}")]
    Internal { message: String },
}

// One JSON-RPC-shaped error object shared by `rpc`, HTTP, and the dedicated
// commands. Codes outside the reserved JSON-RPC range identify application
// errors; `data.kind` is the stable machine-readable name.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ErrorObject {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl AppError {
    pub fn kind(&self) -> &'static str {
        match self {
        Self::EmptyStatePath => "state_path_empty",
        Self::WorkingDirectory { .. } => "working_directory",
        Self::DirectoryMissing { .. } => "directory_missing",
        Self::DatabaseMissing { .. } => "database_missing",
        Self::DatabaseUnreadable { .. } => "database_unreadable",
        Self::DatabaseExists { .. } => "database_exists",
        Self::Lock { .. } => "lock_failed",
        Self::Busy { .. } => "busy",
        Self::PublicationFailed { .. } => "publication_failed",
        Self::PublicationUncertain { .. } => "publication_uncertain",
        Self::MalformedProposal(_) => "malformed_proposal",
        Self::InvalidProposal { .. } => "invalid_proposal",
        Self::AcknowledgmentUnsupported => "acknowledgment_unsupported",
        Self::ForeignDatabase { .. } => "foreign_database",
        Self::StaleBase { .. } => "stale_base",
        Self::Rejected(_) => "rejected",
        Self::Analyzer(_) => "analyzer_failure",
        Self::EntryNotFound { .. } => "entry_not_found",
        Self::BeyondBoundary { .. } => "beyond_boundary",
        Self::InvalidParams { .. } => "invalid_params",
        Self::Backend { .. } => "backend_failure",
        Self::Internal { .. } => "internal",
        }
    }

    pub fn code(&self) -> i64 {
        match self {
        Self::InvalidParams { .. } => -32602,
        Self::Internal { .. } => -32603,
        Self::EmptyStatePath | Self::WorkingDirectory { .. } | Self::DirectoryMissing { .. } => {
            1001
        }
        Self::DatabaseMissing { .. } => 1002,
        Self::DatabaseUnreadable { .. } => 1003,
        Self::DatabaseExists { .. } => 1004,
        Self::Lock { .. } => 1005,
        Self::Busy { .. } => 1006,
        Self::PublicationFailed { .. } => 1007,
        Self::PublicationUncertain { .. } => 1008,
        Self::MalformedProposal(_) => 2001,
        Self::InvalidProposal { .. } => 2002,
        Self::AcknowledgmentUnsupported => 2003,
        Self::ForeignDatabase { .. } => 2004,
        Self::StaleBase { .. } => 2005,
        Self::Rejected(_) => 2006,
        Self::Analyzer(_) => 3001,
        Self::EntryNotFound { .. } => 4001,
        Self::BeyondBoundary { .. } => 4002,
        Self::Backend { .. } => 5001,
        }
    }

    fn details(&self) -> Value {
        let path = |path: &PathBuf| path.to_string_lossy().into_owned();
        match self {
        Self::DirectoryMissing { path: p }
        | Self::DatabaseMissing { path: p }
        | Self::DatabaseExists { path: p } => json!({ "path": path(p) }),
        Self::DatabaseUnreadable { path: p, .. } | Self::Lock { path: p, .. } => {
            json!({ "path": path(p) })
        }
        Self::Busy { lock } => json!({ "lock": path(lock) }),
        Self::MalformedProposal(error) => json!({ "decode": error }),
        Self::InvalidProposal {
            location: Some(location),
            ..
        } => json!({ "location": location }),
        Self::ForeignDatabase { expected, actual } | Self::StaleBase { expected, actual } => {
            json!({ "expected": expected, "actual": actual })
        }
        Self::PublicationUncertain { outcome, .. } => json!({ "outcome": outcome }),
        Self::Rejected(report) => json!({ "report": report }),
        Self::Analyzer(LifeCoreError::AnalyzerExecution(failure)) => {
            json!({ "failure": failure })
        }
        Self::EntryNotFound { id } => json!({ "id": id }),
        // Positions are decimal strings on the wire, like every boundary.
        Self::BeyondBoundary { requested, visible } => {
            json!({ "requested": requested.to_string(), "visible": visible.to_string() })
        }
        _ => json!({}),
        }
    }

    pub fn to_object(&self) -> ErrorObject {
        let mut data = self.details();
        data["kind"] = self.kind().into();
        ErrorObject {
            code: self.code(),
            message: self.to_string(),
            data: Some(data),
        }
    }
}
