use clap::error::ErrorKind;
use std::{io, path::Path};

use crate::cli::OutputFormat;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{message}")]
    Usage { message: String, exit_code: u8 },

    #[error("operation '{operation}' is unavailable in implementation stage 2")]
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
        | Self::AmbiguousMessageId { format, .. } => format,
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
        | Self::AmbiguousMessageId { format, .. } => *format,
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
        Self::Io(_) => "io",
        }
    }
}
