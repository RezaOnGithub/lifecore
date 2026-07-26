use clap::error::ErrorKind;
use std::{ffi::OsStr, io, path::Path, process::ExitStatus};

use crate::{
    app::{AnalyzerRecord, EntryRecord, ViewSummary},
    cli::OutputFormat,
    messages::MessageRecord,
};

#[derive(Debug)]
pub struct TransactionAbortDetail {
    pub entries: Vec<EntryRecord>,
    pub analyzers: Vec<AnalyzerRecord>,
    pub messages: Vec<MessageRecord>,
    pub view: ViewSummary,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{message}")]
    Usage { message: String, exit_code: u8 },

    #[error("operation '{operation}' is unavailable in implementation stage 8")]
    Unavailable {
        operation: String,
        format: OutputFormat,
    },

    #[error("invalid state path: {message}")]
    InvalidStatePath {
        message: String,
        format: OutputFormat,
    },

    #[error("state I/O failure at {path}: {source}")]
    StateIo {
        path: String,
        #[source]
        source: io::Error,
        format: OutputFormat,
    },

    #[error("state JSON at {path} could not be decoded: {source}")]
    StateDecode {
        path: String,
        #[source]
        source: serde_json::Error,
        format: OutputFormat,
    },

    #[error("state JSON for {path} could not be encoded: {source}")]
    StateEncode {
        path: String,
        #[source]
        source: serde_json::Error,
        format: OutputFormat,
    },

    #[error("invalid canonical state: {message}")]
    InvalidState {
        message: String,
        format: OutputFormat,
    },

    #[error("analysis failed: {message}")]
    Analysis {
        message: String,
        format: OutputFormat,
    },

    #[error("invalid message ID: {message}")]
    InvalidMessageId {
        message: String,
        format: OutputFormat,
    },

    #[error("message ID {prefix:?} was not emitted by the current analysis")]
    UnknownMessageId {
        prefix: String,
        format: OutputFormat,
    },

    #[error("message ID {prefix:?} is ambiguous")]
    AmbiguousMessageId {
        prefix: String,
        format: OutputFormat,
    },

    #[error("message {message_id} has severity {actual}; expected {expected}")]
    WrongMessageSeverity {
        message_id: String,
        actual: String,
        expected: &'static str,
        format: OutputFormat,
    },

    #[error("message ID {prefix:?} is not present in the dismissal registry")]
    DismissalNotFound {
        prefix: String,
        format: OutputFormat,
    },

    #[error("invalid expected head: {message}")]
    InvalidExpectedHead {
        message: String,
        format: OutputFormat,
    },

    #[error("expected canonical head {supplied}, but found {actual}")]
    ExpectedHeadMismatch {
        supplied: String,
        actual: String,
        format: OutputFormat,
    },

    #[error("transaction staging failed: {message}")]
    TransactionStage {
        message: String,
        format: OutputFormat,
    },

    #[error("transaction aborted: {message}")]
    TransactionAborted {
        message: String,
        detail: Box<TransactionAbortDetail>,
        format: OutputFormat,
    },

    #[error("invalid entry locator: {message}")]
    InvalidEntryLocator {
        message: String,
        format: OutputFormat,
    },

    #[error("unknown entry index {index}")]
    UnknownEntryIndex { index: usize, format: OutputFormat },

    #[error("entry fingerprint mismatch at index {index}: supplied {supplied}, actual {actual}")]
    EntryFingerprintMismatch {
        index: usize,
        supplied: String,
        actual: String,
        format: OutputFormat,
    },

    #[error("entry {index} is not a node")]
    EntryNotNode { index: usize, format: OutputFormat },

    #[error("entry {index} is outside the selected view ending at {view_end}")]
    EntryOutsideView {
        index: usize,
        view_end: usize,
        format: OutputFormat,
    },

    #[error("invalid node reference: {message}")]
    InvalidNodeReference {
        message: String,
        format: OutputFormat,
    },

    #[error("invalid field assignment: {message}")]
    InvalidFieldAssignment {
        message: String,
        format: OutputFormat,
    },

    #[error("invalid payload: {message}")]
    InvalidPayload {
        message: String,
        format: OutputFormat,
    },

    #[error("invalid alias: {message}")]
    InvalidAlias {
        message: String,
        format: OutputFormat,
    },

    #[error("invalid query predicate {predicate:?}: {message}")]
    InvalidQueryPredicate {
        predicate: String,
        message: String,
        format: OutputFormat,
    },

    #[error("selected entry was not returned by query: {locator}")]
    QuerySelection {
        locator: String,
        format: OutputFormat,
    },

    #[error("invalid stored type declaration: {message}")]
    InvalidTypeDeclaration {
        message: String,
        format: OutputFormat,
    },

    #[error("no active, well-formed type declaration named {name:?} exists in the selected view")]
    TypeDeclarationNotFound { name: String, format: OutputFormat },

    #[error(
        "{count} active, well-formed type declarations named {name:?} exist in the selected view"
    )]
    AmbiguousTypeDeclaration {
        name: String,
        count: usize,
        format: OutputFormat,
    },

    #[error("invalid type patch: {message}")]
    InvalidTypePatch {
        message: String,
        format: OutputFormat,
    },

    #[error("type patch does not change declaration {name:?}")]
    TypePatchNoOp { name: String, format: OutputFormat },

    #[error("GraphViz generation failed: {message}")]
    GraphvizGeneration {
        message: String,
        format: OutputFormat,
    },

    #[error("invalid GraphViz target: {message}")]
    GraphvizTarget {
        message: String,
        format: OutputFormat,
    },

    #[error("could not {action} GraphViz target {path}: {source}")]
    GraphvizTargetIo {
        path: String,
        action: &'static str,
        #[source]
        source: io::Error,
        format: OutputFormat,
    },

    #[error("could not start GraphViz executable {program:?}: {source}")]
    GraphvizProcessSpawn {
        program: String,
        #[source]
        source: io::Error,
        format: OutputFormat,
    },

    #[error("GraphViz process I/O failed while attempting to {action}: {source}")]
    GraphvizProcessIo {
        action: &'static str,
        #[source]
        source: io::Error,
        format: OutputFormat,
    },

    #[error("GraphViz executable {program:?} exited with {status}: {stderr}")]
    GraphvizProcessFailed {
        program: String,
        status: String,
        stderr: String,
        format: OutputFormat,
    },

    #[error("I/O failure: {0}")]
    Io(#[from] std::io::Error),
}

impl AppError {
    pub fn usage(error: clap::Error) -> Self {
        let exit_code = if matches!(
            error.kind(),
            ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
        ) {
            0
        } else {
            2
        };

        Self::Usage {
            message: error.to_string(),
            exit_code,
        }
    }

    pub fn unavailable(operation: impl Into<String>, format: OutputFormat) -> Self {
        Self::Unavailable {
            operation: operation.into(),
            format,
        }
    }

    pub fn invalid_state_path(message: impl Into<String>) -> Self {
        Self::InvalidStatePath {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn state_io(path: &Path, source: io::Error) -> Self {
        Self::StateIo {
            path: path.display().to_string(),
            source,
            format: OutputFormat::Lines,
        }
    }

    pub fn state_decode(path: &Path, source: serde_json::Error) -> Self {
        Self::StateDecode {
            path: path.display().to_string(),
            source,
            format: OutputFormat::Lines,
        }
    }

    pub fn state_encode(path: &Path, source: serde_json::Error) -> Self {
        Self::StateEncode {
            path: path.display().to_string(),
            source,
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_state(message: impl Into<String>) -> Self {
        Self::InvalidState {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn analysis(message: impl Into<String>) -> Self {
        Self::Analysis {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_message_id(message: impl Into<String>) -> Self {
        Self::InvalidMessageId {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn unknown_message_id(prefix: impl Into<String>) -> Self {
        Self::UnknownMessageId {
            prefix: prefix.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn ambiguous_message_id(prefix: impl Into<String>) -> Self {
        Self::AmbiguousMessageId {
            prefix: prefix.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn wrong_message_severity(
        message_id: impl Into<String>,
        actual: impl Into<String>,
        expected: &'static str,
    ) -> Self {
        Self::WrongMessageSeverity {
            message_id: message_id.into(),
            actual: actual.into(),
            expected,
            format: OutputFormat::Lines,
        }
    }

    pub fn dismissal_not_found(prefix: impl Into<String>) -> Self {
        Self::DismissalNotFound {
            prefix: prefix.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_expected_head(message: impl Into<String>) -> Self {
        Self::InvalidExpectedHead {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn expected_head_mismatch(supplied: impl Into<String>, actual: impl Into<String>) -> Self {
        Self::ExpectedHeadMismatch {
            supplied: supplied.into(),
            actual: actual.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn transaction_stage(message: impl Into<String>) -> Self {
        Self::TransactionStage {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn transaction_aborted(
        message: impl Into<String>,
        entries: Vec<EntryRecord>,
        analyzers: Vec<AnalyzerRecord>,
        messages: Vec<MessageRecord>,
        view: ViewSummary,
    ) -> Self {
        Self::TransactionAborted {
            message: message.into(),
            detail: Box::new(TransactionAbortDetail {
                entries,
                analyzers,
                messages,
                view,
            }),
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_entry_locator(message: impl Into<String>) -> Self {
        Self::InvalidEntryLocator {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn unknown_entry_index(index: usize) -> Self {
        Self::UnknownEntryIndex {
            index,
            format: OutputFormat::Lines,
        }
    }

    pub fn entry_fingerprint_mismatch(
        index: usize,
        supplied: impl Into<String>,
        actual: impl Into<String>,
    ) -> Self {
        Self::EntryFingerprintMismatch {
            index,
            supplied: supplied.into(),
            actual: actual.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn entry_not_node(index: usize) -> Self {
        Self::EntryNotNode {
            index,
            format: OutputFormat::Lines,
        }
    }

    pub fn entry_outside_view(index: usize, view_end: usize) -> Self {
        Self::EntryOutsideView {
            index,
            view_end,
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_node_reference(message: impl Into<String>) -> Self {
        Self::InvalidNodeReference {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_field_assignment(message: impl Into<String>) -> Self {
        Self::InvalidFieldAssignment {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_payload(message: impl Into<String>) -> Self {
        Self::InvalidPayload {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_alias(message: impl Into<String>) -> Self {
        Self::InvalidAlias {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_query_predicate(
        predicate: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self::InvalidQueryPredicate {
            predicate: predicate.into(),
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn query_selection(locator: impl Into<String>) -> Self {
        Self::QuerySelection {
            locator: locator.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_type_declaration(message: impl Into<String>) -> Self {
        Self::InvalidTypeDeclaration {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn type_declaration_not_found(name: impl Into<String>) -> Self {
        Self::TypeDeclarationNotFound {
            name: name.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn ambiguous_type_declaration(name: impl Into<String>, count: usize) -> Self {
        Self::AmbiguousTypeDeclaration {
            name: name.into(),
            count,
            format: OutputFormat::Lines,
        }
    }

    pub fn invalid_type_patch(message: impl Into<String>) -> Self {
        Self::InvalidTypePatch {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn type_patch_no_op(name: impl Into<String>) -> Self {
        Self::TypePatchNoOp {
            name: name.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn graphviz_generation(message: impl Into<String>) -> Self {
        Self::GraphvizGeneration {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn graphviz_target(message: impl Into<String>) -> Self {
        Self::GraphvizTarget {
            message: message.into(),
            format: OutputFormat::Lines,
        }
    }

    pub fn graphviz_target_io(path: &Path, action: &'static str, source: io::Error) -> Self {
        Self::GraphvizTargetIo {
            path: path.display().to_string(),
            action,
            source,
            format: OutputFormat::Lines,
        }
    }

    pub fn graphviz_process_spawn(program: &OsStr, source: io::Error) -> Self {
        Self::GraphvizProcessSpawn {
            program: program.to_string_lossy().into_owned(),
            source,
            format: OutputFormat::Lines,
        }
    }

    pub fn graphviz_process_io(action: &'static str, source: io::Error) -> Self {
        Self::GraphvizProcessIo {
            action,
            source,
            format: OutputFormat::Lines,
        }
    }

    pub fn graphviz_process(message: impl Into<String>) -> Self {
        Self::graphviz_generation(message)
    }

    pub fn graphviz_process_failed(program: &OsStr, status: ExitStatus, stderr: &[u8]) -> Self {
        let stderr = String::from_utf8_lossy(stderr).trim().to_string();

        Self::GraphvizProcessFailed {
            program: program.to_string_lossy().into_owned(),
            status: status.to_string(),
            stderr: if stderr.is_empty() {
                "no stderr output".to_string()
            } else {
                stderr
            },
            format: OutputFormat::Lines,
        }
    }

    pub fn with_format(mut self, selected: OutputFormat) -> Self {
        let format = match &mut self {
        Self::Usage { .. } | Self::Io(_) => return self,
        Self::Unavailable { format, .. }
        | Self::InvalidStatePath { format, .. }
        | Self::StateIo { format, .. }
        | Self::StateDecode { format, .. }
        | Self::StateEncode { format, .. }
        | Self::InvalidState { format, .. }
        | Self::Analysis { format, .. }
        | Self::InvalidMessageId { format, .. }
        | Self::UnknownMessageId { format, .. }
        | Self::AmbiguousMessageId { format, .. }
        | Self::WrongMessageSeverity { format, .. }
        | Self::DismissalNotFound { format, .. }
        | Self::InvalidExpectedHead { format, .. }
        | Self::ExpectedHeadMismatch { format, .. }
        | Self::TransactionStage { format, .. }
        | Self::TransactionAborted { format, .. }
        | Self::InvalidEntryLocator { format, .. }
        | Self::UnknownEntryIndex { format, .. }
        | Self::EntryFingerprintMismatch { format, .. }
        | Self::EntryNotNode { format, .. }
        | Self::EntryOutsideView { format, .. }
        | Self::InvalidNodeReference { format, .. }
        | Self::InvalidFieldAssignment { format, .. }
        | Self::InvalidPayload { format, .. }
        | Self::InvalidAlias { format, .. }
        | Self::InvalidQueryPredicate { format, .. }
        | Self::QuerySelection { format, .. }
        | Self::InvalidTypeDeclaration { format, .. }
        | Self::TypeDeclarationNotFound { format, .. }
        | Self::AmbiguousTypeDeclaration { format, .. }
        | Self::InvalidTypePatch { format, .. }
        | Self::TypePatchNoOp { format, .. }
        | Self::GraphvizGeneration { format, .. }
        | Self::GraphvizTarget { format, .. }
        | Self::GraphvizTargetIo { format, .. }
        | Self::GraphvizProcessSpawn { format, .. }
        | Self::GraphvizProcessIo { format, .. }
        | Self::GraphvizProcessFailed { format, .. } => format,
        };

        *format = selected;
        self
    }

    pub fn exit_code(&self) -> u8 {
        match self {
        Self::Usage { exit_code, .. } => *exit_code,
        Self::Unavailable { .. }
        | Self::InvalidStatePath { .. }
        | Self::StateIo { .. }
        | Self::StateDecode { .. }
        | Self::StateEncode { .. }
        | Self::InvalidState { .. }
        | Self::Analysis { .. }
        | Self::InvalidMessageId { .. }
        | Self::UnknownMessageId { .. }
        | Self::AmbiguousMessageId { .. }
        | Self::WrongMessageSeverity { .. }
        | Self::DismissalNotFound { .. }
        | Self::InvalidExpectedHead { .. }
        | Self::ExpectedHeadMismatch { .. }
        | Self::TransactionStage { .. }
        | Self::TransactionAborted { .. }
        | Self::InvalidEntryLocator { .. }
        | Self::UnknownEntryIndex { .. }
        | Self::EntryFingerprintMismatch { .. }
        | Self::EntryNotNode { .. }
        | Self::EntryOutsideView { .. }
        | Self::InvalidNodeReference { .. }
        | Self::InvalidFieldAssignment { .. }
        | Self::InvalidPayload { .. }
        | Self::InvalidAlias { .. }
        | Self::InvalidQueryPredicate { .. }
        | Self::QuerySelection { .. }
        | Self::InvalidTypeDeclaration { .. }
        | Self::TypeDeclarationNotFound { .. }
        | Self::AmbiguousTypeDeclaration { .. }
        | Self::InvalidTypePatch { .. }
        | Self::TypePatchNoOp { .. }
        | Self::GraphvizGeneration { .. }
        | Self::GraphvizTarget { .. }
        | Self::GraphvizTargetIo { .. }
        | Self::GraphvizProcessSpawn { .. }
        | Self::GraphvizProcessIo { .. }
        | Self::GraphvizProcessFailed { .. }
        | Self::Io(_) => 1,
        }
    }

    pub fn output_format(&self) -> OutputFormat {
        match self {
        Self::Usage { .. } => OutputFormat::Lines,
        Self::Unavailable { format, .. }
        | Self::InvalidStatePath { format, .. }
        | Self::StateIo { format, .. }
        | Self::StateDecode { format, .. }
        | Self::StateEncode { format, .. }
        | Self::InvalidState { format, .. }
        | Self::Analysis { format, .. }
        | Self::InvalidMessageId { format, .. }
        | Self::UnknownMessageId { format, .. }
        | Self::AmbiguousMessageId { format, .. }
        | Self::WrongMessageSeverity { format, .. }
        | Self::DismissalNotFound { format, .. }
        | Self::InvalidExpectedHead { format, .. }
        | Self::ExpectedHeadMismatch { format, .. }
        | Self::TransactionStage { format, .. }
        | Self::TransactionAborted { format, .. }
        | Self::InvalidEntryLocator { format, .. }
        | Self::UnknownEntryIndex { format, .. }
        | Self::EntryFingerprintMismatch { format, .. }
        | Self::EntryNotNode { format, .. }
        | Self::EntryOutsideView { format, .. }
        | Self::InvalidNodeReference { format, .. }
        | Self::InvalidFieldAssignment { format, .. }
        | Self::InvalidPayload { format, .. }
        | Self::InvalidAlias { format, .. }
        | Self::InvalidQueryPredicate { format, .. }
        | Self::QuerySelection { format, .. }
        | Self::InvalidTypeDeclaration { format, .. }
        | Self::TypeDeclarationNotFound { format, .. }
        | Self::AmbiguousTypeDeclaration { format, .. }
        | Self::InvalidTypePatch { format, .. }
        | Self::TypePatchNoOp { format, .. }
        | Self::GraphvizGeneration { format, .. }
        | Self::GraphvizTarget { format, .. }
        | Self::GraphvizTargetIo { format, .. }
        | Self::GraphvizProcessSpawn { format, .. }
        | Self::GraphvizProcessIo { format, .. }
        | Self::GraphvizProcessFailed { format, .. } => *format,
        Self::Io(_) => OutputFormat::Lines,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
        Self::Usage { .. } => "usage",
        Self::Unavailable { .. } => "unavailable",
        Self::InvalidStatePath { .. } => "state_path",
        Self::StateIo { .. } => "state_io",
        Self::StateDecode { .. } => "state_decode",
        Self::StateEncode { .. } => "state_encode",
        Self::InvalidState { .. } => "invalid_state",
        Self::Analysis { .. } => "analysis",
        Self::InvalidMessageId { .. } => "message_id_syntax",
        Self::UnknownMessageId { .. } => "message_id_unknown",
        Self::AmbiguousMessageId { .. } => "message_id_ambiguous",
        Self::WrongMessageSeverity { .. } => "message_severity",
        Self::DismissalNotFound { .. } => "dismissal_not_found",
        Self::InvalidExpectedHead { .. } => "expected_head_syntax",
        Self::ExpectedHeadMismatch { .. } => "expected_head_mismatch",
        Self::TransactionStage { .. } => "transaction_stage",
        Self::TransactionAborted { .. } => "transaction_aborted",
        Self::InvalidEntryLocator { .. } => "entry_locator_syntax",
        Self::UnknownEntryIndex { .. } => "entry_index_unknown",
        Self::EntryFingerprintMismatch { .. } => "entry_fingerprint_mismatch",
        Self::EntryNotNode { .. } => "entry_not_node",
        Self::EntryOutsideView { .. } => "entry_outside_view",
        Self::InvalidNodeReference { .. } => "node_reference_syntax",
        Self::InvalidFieldAssignment { .. } => "field_assignment_syntax",
        Self::InvalidPayload { .. } => "payload_syntax",
        Self::InvalidAlias { .. } => "alias_syntax",
        Self::InvalidQueryPredicate { .. } => "query_predicate_syntax",
        Self::QuerySelection { .. } => "query_selection",
        Self::InvalidTypeDeclaration { .. } => "type_declaration",
        Self::TypeDeclarationNotFound { .. } => "type_declaration_not_found",
        Self::AmbiguousTypeDeclaration { .. } => "type_declaration_ambiguous",
        Self::InvalidTypePatch { .. } => "type_patch",
        Self::TypePatchNoOp { .. } => "type_patch_no_op",
        Self::GraphvizGeneration { .. } => "graphviz_generation",
        Self::GraphvizTarget { .. } => "graphviz_target",
        Self::GraphvizTargetIo { .. } => "graphviz_target_io",
        Self::GraphvizProcessSpawn { .. } => "graphviz_process_spawn",
        Self::GraphvizProcessIo { .. } => "graphviz_process_io",
        Self::GraphvizProcessFailed { .. } => "graphviz_process_failed",
        Self::Io(_) => "io",
        }
    }
}
