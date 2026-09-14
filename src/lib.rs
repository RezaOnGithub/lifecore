mod entries;
mod graph;
mod values;
mod view;

pub mod analysis;
pub mod inspection;
pub mod transaction;
pub mod typing;

pub use entries::{Edge, Entry, EntryId, Node};
pub use graph::{BasicValidationFailure, LifeCoreList};
pub use jiff::Timestamp;
pub use values::{Body, Payload, PayloadKind};
pub use view::{DiagnosticLocation, View, ViewError, ViewId};

#[derive(Debug, thiserror::Error)]
pub enum LifeCoreError {
    #[error(transparent)]
    BasicValidationFailure(#[from] BasicValidationFailure),
    #[error("analyzer {name:?} is not registered")]
    AnalyzerNotFound { name: String },
    #[error("analyzer {dupe_name:?} is already registered")]
    DuplicateAnalyzer { dupe_name: String },
    #[error("invalid configuration for analyzer {analyzer_name:?}: {message}")]
    InvalidAnalyzerConfiguration {
        analyzer_name: String,
        message: String,
    },
    #[error("analysis emitted blocking inconsistencies")]
    SystemsError(Vec<analysis::Inconsistency>),
}

#[cfg(test)]
mod test;
