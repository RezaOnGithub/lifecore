use std::path::PathBuf;

use serde::Serialize;

use crate::{cli::OutputFormat, messages::MessageRecord};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ViewSummary {
    pub end: usize,
}

#[derive(Debug, Eq, PartialEq)]
pub enum CommandOutcome {
    Text(String),
    Initialized {
        format: OutputFormat,
        path: PathBuf,
        uuid: String,
        view: ViewSummary,
        messages: Vec<MessageRecord>,
    },
    Messages {
        format: OutputFormat,
        view: ViewSummary,
        items: Vec<MessageRecord>,
        detail: bool,
    },
}

impl CommandOutcome {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn format(&self) -> OutputFormat {
        match self {
        Self::Text(_) => OutputFormat::Lines,
        Self::Initialized { format, .. } | Self::Messages { format, .. } => *format,
        }
    }
}
