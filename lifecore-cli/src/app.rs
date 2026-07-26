use std::path::PathBuf;

use lifecore::Body;
use serde::Serialize;

use crate::{
    cli::OutputFormat,
    messages::{DismissalStatusRecord, MessageRecord},
    reference::AliasStatus,
    transaction::TransactionResult,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ViewSummary {
    pub end: usize,
    pub canonical_end: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub through: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_through: Option<String>,
}

impl ViewSummary {
    pub fn empty() -> Self {
        Self {
            end: 0,
            canonical_end: 0,
            through: None,
            display_through: None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct EntryRecord {
    pub index: usize,
    pub locator: String,
    pub display_locator: String,
    pub kind: String,
    pub type_name: String,
    pub aliases: Vec<String>,
    pub temporary_aliases: Vec<String>,
    pub fresh: bool,
    pub provisional: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fields: Option<Body>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<String>,
    pub circumstance: CircumstanceRecord,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CircumstanceRecord {
    Mock,
    Session {
        when_1h: String,
        hostname: String,
        username: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PredicateResult {
    pub predicate: String,
    pub matches: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QueryExplanationRecord {
    pub candidate: EntryRecord,
    pub combinator: String,
    pub predicates: Vec<PredicateResult>,
    pub matches: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AnalyzerRecord {
    pub name: String,
    pub config: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AliasRecord {
    pub name: String,
    pub locator: String,
    pub display_locator: String,
    pub status: AliasStatus,
    pub temporary: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct AliasChangeRecord {
    pub action: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locator: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct TypeRecord {
    pub index: usize,
    pub name: String,
    pub locator: String,
    pub display_locator: String,
    pub provisional: bool,
    pub fields: Vec<String>,
    pub outgoing: Vec<String>,
    pub incoming: Vec<String>,
    pub allows_override: Vec<String>,
    pub command: String,
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
    Dismissals {
        format: OutputFormat,
        items: Vec<DismissalStatusRecord>,
    },
    DismissalChanged {
        format: OutputFormat,
        action: String,
        id: String,
        emitted: bool,
    },
    Analyzers {
        format: OutputFormat,
        view: ViewSummary,
        items: Vec<AnalyzerRecord>,
    },
    Aliases {
        format: OutputFormat,
        view: ViewSummary,
        items: Vec<AliasRecord>,
    },
    AliasChanged {
        format: OutputFormat,
        alias: AliasChangeRecord,
    },
    SidecarAttachment {
        action: String,
        path: PathBuf,
    },
    SidecarDump {
        format: OutputFormat,
        view: ViewSummary,
        items: Vec<crate::sidecar::SidecarDumpItem>,
    },
    SidecarCommitted {
        format: OutputFormat,
        result: TransactionResult,
    },
    Types {
        format: OutputFormat,
        view: ViewSummary,
        items: Vec<TypeRecord>,
        detail: bool,
    },
    Transaction {
        format: OutputFormat,
        result: TransactionResult,
    },
    Entries {
        format: OutputFormat,
        view: ViewSummary,
        items: Vec<EntryRecord>,
        detail: bool,
    },
    QueryExplanation {
        format: OutputFormat,
        view: ViewSummary,
        explanation: Box<QueryExplanationRecord>,
    },
    GraphvizArtifact {
        target: PathBuf,
        artifact_kind: String,
        view: ViewSummary,
    },
}

impl CommandOutcome {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text(text.into())
    }

    pub fn format(&self) -> OutputFormat {
        match self {
        Self::Text(_) => OutputFormat::Lines,
        Self::Initialized { format, .. }
        | Self::Messages { format, .. }
        | Self::Dismissals { format, .. }
        | Self::DismissalChanged { format, .. }
        | Self::Analyzers { format, .. }
        | Self::Aliases { format, .. }
        | Self::AliasChanged { format, .. }
        | Self::SidecarDump { format, .. }
        | Self::SidecarCommitted { format, .. }
        | Self::Types { format, .. }
        | Self::Transaction { format, .. }
        | Self::Entries { format, .. }
        | Self::QueryExplanation { format, .. } => *format,
        Self::SidecarAttachment { .. } | Self::GraphvizArtifact { .. } => OutputFormat::Lines,
        }
    }
}
