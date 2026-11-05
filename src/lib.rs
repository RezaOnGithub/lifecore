// The derives of `lifecore-derive` name this crate as `::lifecore`, here as
// in any dependent.
extern crate self as lifecore;

mod entries;
mod graph;
mod index;
mod time;
mod values;
mod view;

pub mod analysis;
pub mod decimal;
pub mod decode;
pub mod inspection;
pub mod transaction;
pub mod typing;

pub use entries::{Edge, Entry, EntryId, Node};
pub use graph::{BasicValidationFailure, LifeCoreList};
pub use index::{End, IndexTemplate, Indexes, TemplateIndex, TemplateScan};
pub use jiff::{SignedDuration, Timestamp, civil::Date};
pub use time::OffsetTimestamp;
pub use values::{Body, Payload, PayloadKind};
pub use view::{View, ViewError, ViewId};

// Equality lets tests compare failures, such as the runs of one history under
// every indexing mode.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
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
    #[error(transparent)]
    AnalyzerExecution(Box<analysis::ExecutionFailure>),
    #[error("invalid output from analyzer {analyzer:?}: {message}")]
    InvalidAnalyzerOutput { analyzer: String, message: String },
}

#[cfg(test)]
mod test;
